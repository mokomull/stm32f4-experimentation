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

    let audio = peripherals.SPI2;
    audio.i2scfgr.write(|w| {
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
    audio.i2spr.write(|w| {
        w.mckoe().set_bit();
        unsafe { w.i2sdiv().bits(3) };
        w.odd().set_bit()
    });
    audio.i2scfgr.modify(|_r, w| w.i2se().set_bit());

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

    let sines = [
        0, 1094932, 2171131, 3210180, 4194303, 5106660, 5931640, 6655129, 7264746, 7750062,
        8102772, 8316841, 8388607, 8316841, 8102772, 7750062, 7264746, 6655129, 5931640, 5106660,
        4194303, 3210180, 2171131, 1094932, 0, 15682284, 14606085, 13567036, 12582913, 11670556,
        10845576, 10122087, 9512470, 9027154, 8674444, 8460375, 8388609, 8460375, 8674444, 9027154,
        9512470, 10122087, 10845576, 11670556, 12582913, 13567036, 14606085, 15682284,
    ];

    loop {
        for sample in &sines {
            for _channel in 0..2 {
                while !audio.sr.read().txe().bit() {}
                audio
                    .dr
                    .write(|w| w.dr().bits(((*sample & 0xffff00) >> 8) as u16));
                while !audio.sr.read().txe().bit() {}
                audio
                    .dr
                    .write(|w| w.dr().bits(((*sample & 0xff) << 8) as u16));
            }
        }
    }

    // buffer that can hold a second of data
    let mut buffer = [0u16; SAMPLE_RATE];

    for i in (0..buffer.len()).cycle() {
        // wait for the SPI peripheral to need another sample
        while !audio.sr.read().txe().bit() {}

        // give it another sample
        audio.dr.write(|w| w.dr().bits(buffer[i]));

        // and while we're waiting for that to get clocked-out, grab the current value from the ADC
        // TODO: actually read samples from the codec
        // let new_sample = adc.dr.read().data().bits();
        let new_sample = 0;

        // wait for the SPI peripheral to need the same sample for the other channel
        while !audio.sr.read().txe().bit() {}
        audio.dr.write(|w| w.dr().bits(buffer[i]));

        // write that sample 800ms into the future
        buffer[(i + (SAMPLE_RATE * 800 / 1000)) % buffer.len()] = new_sample;
    }

    panic!("cycle() should never complete");
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
