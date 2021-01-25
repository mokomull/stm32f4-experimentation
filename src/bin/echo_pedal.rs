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
        w.ckpol().set_bit();
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

    let mut control = stm32f4xx_hal::spi::Spi::spi3(
        peripherals.SPI3,
        (control_sck, NoMiso, control_mosi),
        spi::Mode {
            phase: spi::Phase::CaptureOnSecondTransition,
            polarity: spi::Polarity::IdleHigh,
        },
        200.khz().into(),
        clocks,
    );

    set_codec_register(&mut control, &mut control_csb, 0x9 /* active */, 0x1);

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

fn set_codec_register<SPI, GPIO>(spi: &mut SPI, not_cs: &mut GPIO, register: u8, value: u8)
where
    SPI: embedded_hal::blocking::spi::Write<u8>,
    SPI::Error: core::fmt::Debug,
    GPIO: embedded_hal::digital::v2::OutputPin,
    GPIO::Error: core::fmt::Debug,
{
    not_cs.set_low().unwrap();

    embedded_hal::blocking::spi::Write::write(spi, &[register, value]).expect("SPI write failed");

    not_cs.set_high().unwrap();
}
