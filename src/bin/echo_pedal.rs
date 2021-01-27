#![no_main]
#![no_std]

use cortex_m_rt::entry;
use panic_itm as _;

use stm32f407g_disc::spi::{self, NoMiso};
use stm32f4xx_hal::prelude::*;

use stm32f4xx_hal::stm32;

use stm32::spi1::i2scfgr;

// approximate!  This will actually run a fraction of a percent slow, due to I2S clocking
// constraints.
const SAMPLE_RATE: usize = 48_000;

#[entry]
fn main() -> ! {
    let peripherals = stm32f407g_disc::Peripherals::take().unwrap();
    let mut core_peripherals = cortex_m::Peripherals::take().unwrap();

    let rcc = peripherals.RCC.constrain();
    let clocks = rcc.cfgr.use_hse(8.mhz()).sysclk(168.mhz()).freeze();

    let _itm = &mut core_peripherals.ITM.stim[0];

    let porta = peripherals.GPIOA.split();
    let portb = peripherals.GPIOB.split();
    let portc = peripherals.GPIOC.split();

    let _audio_mck = portc.pc6.into_alternate_af5();
    let _audio_sck = portb.pb10.into_alternate_af5();
    let _audio_sd_out = portc.pc3.into_alternate_af5();
    let _audio_sd_in = portc.pc2.into_alternate_af6();
    let _audio_ws = portb.pb9.into_alternate_af5();

    let control_sck = portc.pc10.into_alternate_af6();
    let control_mosi = portc.pc12.into_alternate_af6();
    let _control_nss = porta.pa4.into_alternate_af6();
    let mut control_csb = portc.pc11.into_push_pull_output();

    control_csb.set_high().unwrap();

    // enable the DAC peripheral
    unsafe {
        let rcc = &*stm32::RCC::ptr();

        // enable the I2S PLL: the VCO input should be 2MHz since we have an 8MHz crystal -- but that's
        // going to depend on what freeze() chose above.
        // 2MHz * 129 / 3 = 86 MHz
        rcc.plli2scfgr.write(|w| {
            w.plli2sn().bits(129);
            w.plli2sr().bits(3)
        });
        rcc.cr.modify(|_r, w| w.plli2son().set_bit());
        while !rcc.cr.read().plli2srdy().bit() {}

        rcc.apb1enr.modify(|_r, w| w.spi2en().set_bit());
        rcc.ahb1enr.modify(|_r, w| {
            w.dma1en().set_bit();
            w.dma2en().set_bit()
        });
        rcc.apb2enr.modify(|_r, w| w.adc1en().set_bit())
    }

    let audio_rx = peripherals.I2S2EXT;
    audio_rx.i2scfgr.write(|w| {
        w.i2smod().set_bit();
        w.i2scfg().slave_rx();
        w.i2sstd().msb();
        w.ckpol().clear_bit();
        w.datlen().twenty_four_bit();
        w.chlen().thirty_two_bit()
    });
    audio_rx.i2scfgr.modify(|_r, w| w.i2se().set_bit());

    let audio_tx = peripherals.SPI2;
    audio_tx.i2scfgr.write(|w| {
        w.i2smod().set_bit();
        w.i2scfg().variant(i2scfgr::I2SCFG_A::MASTERTX);
        w.i2sstd().variant(i2scfgr::I2SSTD_A::MSB);
        w.ckpol().clear_bit();
        w.datlen().variant(i2scfgr::DATLEN_A::TWENTYFOURBIT);
        w.chlen().variant(i2scfgr::CHLEN_A::THIRTYTWOBIT)
    });
    // 86MHz / (3 * 2 + 1) = 12.2857MHz MCK
    // 12.2857MHz / 8 [fixed in hardware] = 1.53571MHz bit clock
    // 1.53571MHz / (2 channels * 16 bits per sample) = 47.9911k samples per sec
    audio_tx.i2spr.write(|w| {
        w.mckoe().set_bit();
        unsafe { w.i2sdiv().bits(3) };
        w.odd().set_bit()
    });
    audio_tx.i2scfgr.modify(|_r, w| w.i2se().set_bit());

    let control_spi = stm32f4xx_hal::spi::Spi::spi3(
        peripherals.SPI3,
        (control_sck, NoMiso, control_mosi),
        spi::Mode {
            phase: spi::Phase::CaptureOnSecondTransition,
            polarity: spi::Polarity::IdleHigh,
        },
        200.khz().into(),
        clocks,
    );
    let mut control = Control {
        spi: control_spi,
        not_cs: control_csb,
        delay: stm32f4xx_hal::delay::Delay::new(core_peripherals.SYST, clocks),
    };

    control.set_register(0xf /* reset */, 0);
    control.set_register(0x6 /* power down */, 0b0_0111_0011);

    // sidetone off; DAC selected; bypass off; line input selected; mic muted; mic boost off
    control.set_register(0x4 /* analogue audio path */, 0b0_0001_0010);

    // disable DAC mute, deemphasis for 48k
    control.set_register(0x5 /* digital audio path */, 0b0_0000_0110);

    // nothing inverted, slave, 24-bits, MSB format
    control.set_register(0x7 /* digital audio interface */, 0b0_0000_1001);

    // no clock division, normal mode, 48k
    control.set_register(0x8 /* sampling control */, 0b0_00_0000_00);

    // set active
    control.set_register(0x9 /* active */, 0x1);

    // enable output
    control.set_register(0x6 /* power down */, 0b0_0110_0011);

    // buffer that can hold a half second of data
    let mut top_buffer = [0u16; 16];
    let mut bot_buffer = [0u16; 16];

    assert_eq!(top_buffer.len(), bot_buffer.len());

    let mut last_txside = false;
    let mut last_rxside = false;

    let mut rx_i = 0;
    let mut tx_i = top_buffer.len() - 2;

    loop {
        // dispatch the receive and transmit actions as they're ready

        let rx = audio_rx.sr.read();
        if rx.rxne().bit_is_set() {
            if rx.chside().is_left() {
                if rx.chside().bit() != last_rxside {
                    top_buffer[rx_i] = audio_rx.dr.read().dr().bits();
                } else {
                    bot_buffer[rx_i] = audio_rx.dr.read().dr().bits();
                    // receiving samples from the codec is what drives the indexing forward
                    rx_i = (rx_i + 1) % top_buffer.len();

                    // DO NOT COMMIT: stop after the buffer
                    assert_ne!(rx_i, 0);
                }
            } else {
                // do nothing with the right channel
                audio_rx.dr.read();
            }

            last_rxside = rx.chside().bit();
        }
        core::mem::drop(rx);

        let tx = audio_tx.sr.read();
        if tx.txe().bit_is_set() {
            // TODO: I have no idea which part is lying to me, but experimentally, I
            // accidentally probed the "wrong" output pin and it started working (ish).
            if tx.chside().is_right() {
                if tx.chside().bit() != last_txside {
                    audio_tx.dr.write(|w| w.dr().bits(top_buffer[tx_i]));
                } else {
                    audio_tx.dr.write(|w| w.dr().bits(bot_buffer[tx_i]));
                    tx_i = (tx_i + 1) % top_buffer.len();
                }
            } else {
                // send nothing to the right channel
                audio_tx.dr.write(|w| w.dr().bits(0));
            }

            last_txside = tx.chside().bit();
        }
    }
}

struct Control<SPI, GPIO, DELAY> {
    spi: SPI,
    not_cs: GPIO,
    delay: DELAY,
}

impl<SPI, GPIO, DELAY> Control<SPI, GPIO, DELAY>
where
    SPI: embedded_hal::blocking::spi::Write<u8>,
    SPI::Error: core::fmt::Debug,
    GPIO: embedded_hal::digital::v2::OutputPin,
    GPIO::Error: core::fmt::Debug,
    DELAY: embedded_hal::blocking::delay::DelayUs<u8>,
{
    fn set_register(&mut self, register: u8, value: u16) {
        self.not_cs.set_low().unwrap();

        embedded_hal::blocking::spi::Write::write(
            &mut self.spi,
            &[
                (register << 1) | ((value & 0x100) >> 8) as u8,
                (value & 0xff) as u8,
            ],
        )
        .expect("SPI write failed");

        self.not_cs.set_high().unwrap();

        // t_CSH is minimum 20ns per the datasheet, so 1µs should be fine
        self.delay.delay_us(1);
    }
}
