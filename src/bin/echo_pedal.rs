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
    let clocks = rcc.cfgr.use_hse(8.mhz()).sysclk(167_500.khz()).freeze();

    let _itm = &mut core_peripherals.ITM.stim[0];

    let porta = peripherals.GPIOA.split();
    let portb = peripherals.GPIOB.split();
    let portc = peripherals.GPIOC.split();

    let _audio_sck = portb.pb10.into_alternate_af5();
    let _audio_sd_out = portc.pc3.into_alternate_af5();
    let _audio_sd_in = portc.pc2.into_alternate_af6();
    let _audio_ws = portb.pb9.into_alternate_af5();

    let control_sck = portc.pc10.into_alternate_af6();
    let control_mosi = portc.pc12.into_alternate_af6();
    let _control_nss = porta.pa4.into_alternate_af6();
    let mut control_csb = portc.pc11.into_push_pull_output();

    let mut clock_indicator = portb.pb12.into_push_pull_output();
    let mut channel_indicator = porta.pa2.into_push_pull_output();

    control_csb.set_high().unwrap();

    // enable the DAC peripheral
    unsafe {
        let rcc = &*stm32::RCC::ptr();

        rcc.apb1enr.modify(|_r, w| w.spi2en().set_bit());
        rcc.ahb1enr.modify(|_r, w| {
            w.dma1en().set_bit();
            w.dma2en().set_bit()
        });
        rcc.apb2enr.modify(|_r, w| w.adc1en().set_bit());

        rcc.plli2scfgr.write(|w| {
            w.plli2sn().bits(60);
            w.plli2sr().bits(5)
        });
        rcc.cr.modify(|_r, w| w.plli2son().set_bit());
        while !rcc.cr.read().plli2srdy().bit() {}

        rcc.cfgr.modify(|_r, w| w.mco2().plli2s());
    }

    let _mco2 = portc.pc9.into_alternate_af0();

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
    control.set_register(0x6 /* power down */, 0b0_0111_0010);

    // disable input mute, set to 0dB gain
    control.set_register(0x0 /* left line in */, 0b0_0001_0111);
    // control.set_register(0x1 /* right line in */, 0b0_0001_0111);

    // sidetone off; DAC selected; bypass off; line input selected; mic muted; mic boost off
    control.set_register(0x4 /* analogue audio path */, 0b0_0001_0010);

    // disable DAC mute, deemphasis for 48k
    control.set_register(0x5 /* digital audio path */, 0b0_0000_0110);

    // nothing inverted, master, 24-bits, MSB format
    control.set_register(0x7 /* digital audio interface */, 0b0_0100_1001);

    // no clock division, USB mode, 48k
    control.set_register(0x8 /* sampling control */, 0b0_00_0000_01);

    // set active
    control.set_register(0x9 /* active */, 0x1);

    // enable output
    control.set_register(0x6 /* power down */, 0b0_0110_0010);

    let audio_tx = peripherals.SPI2;
    audio_tx.i2scfgr.write(|w| {
        w.i2smod().set_bit();
        w.i2scfg().variant(i2scfgr::I2SCFG_A::SLAVETX);
        w.i2sstd().variant(i2scfgr::I2SSTD_A::MSB);
        w.ckpol().clear_bit();
        w.datlen().variant(i2scfgr::DATLEN_A::TWENTYFOURBIT);
        w.chlen().variant(i2scfgr::CHLEN_A::THIRTYTWOBIT)
    });
    // I2S must be enabled while WS is low, according to errata ES0182 rev 13, 2.7.1
    unsafe {
        // safety: I do what I want.

        // WS is PB9
        let gpiob = &*stm32::GPIOB::ptr();
        // watch a whole cycle of low
        while !gpiob.idr.read().idr9().bit() {}
        // then a whole cycle of high
        while gpiob.idr.read().idr9().bit() {}
        // then I guess it'll be low long enough for me to get to the enabling below.
    }
    audio_tx.i2scfgr.modify(|_r, w| w.i2se().set_bit());

    loop {
        let sr = audio_tx.sr.read();
        if sr.txe().bit_is_set() {
            clock_indicator.toggle().unwrap();

            if sr.chside().bit_is_clear() {
                channel_indicator.set_low().unwrap();
            } else {
                channel_indicator.set_high().unwrap();
            }

            audio_tx.dr.write(|w| w.dr().bits(1234));
        }
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

    // buffer that can hold a half second of data
    let mut top_buffer = [0u16; SAMPLE_RATE / 2];
    let mut bot_buffer = [0u16; SAMPLE_RATE / 2];

    assert_eq!(top_buffer.len(), bot_buffer.len());

    let mut rx_i = (SAMPLE_RATE * 400 / 1000) % top_buffer.len();
    let mut tx_i = 0;

    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    enum Channel {
        LeftTop,
        LeftBot,
        RightTop,
        RightBot,
    }
    use Channel::*;

    let mut to_rx = LeftTop;
    let mut to_tx = LeftTop;

    loop {
        // dispatch the receive and transmit actions as they're ready

        let rx = audio_rx.sr.read();
        assert!(rx.ovr().is_no_overrun());
        if rx.rxne().bit_is_set() {
            match to_rx {
                LeftTop => {
                    top_buffer[rx_i] = audio_rx.dr.read().dr().bits();
                    to_rx = LeftBot;
                }
                LeftBot => {
                    bot_buffer[rx_i] = audio_rx.dr.read().dr().bits();
                    // receiving samples from the codec is what drives the indexing forward
                    rx_i = (rx_i + 1) % top_buffer.len();
                    to_rx = RightTop;
                }
                RightTop => {
                    // do nothing with the right channel
                    audio_rx.dr.read();
                    to_rx = RightBot;
                }
                RightBot => {
                    // do nothing with the right channel
                    audio_rx.dr.read();
                    to_rx = LeftTop;
                }
            }
        }
        core::mem::drop(rx);

        let tx = audio_tx.sr.read();
        assert!(tx.udr().is_no_underrun());
        if tx.txe().bit_is_set() {
            match to_tx {
                LeftTop => {
                    audio_tx.dr.write(|w| w.dr().bits(top_buffer[tx_i]));
                    to_tx = LeftBot;
                }
                LeftBot => {
                    audio_tx.dr.write(|w| w.dr().bits(bot_buffer[tx_i]));
                    tx_i = (tx_i + 1) % top_buffer.len();
                    to_tx = RightTop;
                }
                RightTop => {
                    // send nothing to the right channel
                    audio_tx.dr.write(|w| w.dr().bits(0));
                    to_tx = RightBot;
                }
                RightBot => {
                    // send nothing to the right channel
                    audio_tx.dr.write(|w| w.dr().bits(0));
                    to_tx = LeftTop;
                }
            }
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
