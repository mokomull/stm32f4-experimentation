#![no_main]
#![no_std]

use cortex_m_rt::entry;
use panic_itm as _;

use stm32f4xx_hal::prelude::*;

use stm32f4xx_hal::stm32;

use stm32::spi1::i2scfgr;

// approximate!  This will actually run a fraction of a percent slow, due to I2S clocking
// constraints.
const SAMPLE_RATE: usize = 48_000;

// fixed address of the DAC on the I2C bus
const ADDRESS: u8 = 0x94 >> 1;

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
    let portd = peripherals.GPIOD.split();

    let _signal_in = porta.pa0.into_analog();

    let _mck = portc.pc7.into_alternate_af6();
    let _sck = portc.pc10.into_alternate_af6();
    let _sd = portc.pc12.into_alternate_af6();
    let _ws = porta.pa4.into_alternate_af6();

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

        rcc.apb1enr.modify(|_r, w| {
            w.dacen().set_bit();
            w.spi3en().set_bit()
        });
        rcc.ahb1enr.modify(|_r, w| {
            w.dma1en().set_bit();
            w.dma2en().set_bit()
        });
        rcc.apb2enr.modify(|_r, w| w.adc1en().set_bit())
    }

    let spi = peripherals.SPI3;
    spi.i2scfgr.write(|w| {
        w.i2smod().set_bit();
        w.i2scfg().variant(i2scfgr::I2SCFG_A::MASTERTX);
        w.i2sstd().variant(i2scfgr::I2SSTD_A::MSB);
        w.ckpol().set_bit();
        w.datlen().variant(i2scfgr::DATLEN_A::SIXTEENBIT);
        w.chlen().variant(i2scfgr::CHLEN_A::SIXTEENBIT)
    });
    // 86MHz / (3 * 2 + 1) = 12.2857MHz MCK
    // 12.2857MHz / 8 [fixed in hardware] = 1.53571MHz bit clock
    // 1.53571MHz / (2 channels * 16 bits per sample) = 47.9911k samples per sec
    spi.i2spr.write(|w| {
        w.mckoe().set_bit();
        unsafe { w.i2sdiv().bits(3) };
        w.odd().set_bit()
    });
    spi.i2scfgr.modify(|_r, w| w.i2se().set_bit());

    let mut audio_reset = portd.pd4.into_push_pull_output();
    audio_reset.set_high().unwrap();
    let mut i2c = stm32f4xx_hal::i2c::I2c::i2c1(
        peripherals.I2C1,
        (
            portb.pb6.into_alternate_af4_open_drain(),
            portb.pb9.into_alternate_af4_open_drain(),
        ),
        50.khz(),
        clocks,
    );

    set_dac_register(&mut i2c, 0x04, 0xaf); // headphone channels ON, speaker channels OFF
    set_dac_register(&mut i2c, 0x05, 0x80); // auto = 1, everything else 0
    set_dac_register(&mut i2c, 0x06, 0x00); // I2S slave, not inverted, not DSP mode, left justified format
    set_dac_register(&mut i2c, 0x07, 0x00); // leave Interface Control 2 alone

    // section 4.11 from the CS43L22 datasheet
    set_dac_register(&mut i2c, 0x00, 0x99);
    set_dac_register(&mut i2c, 0x47, 0x80);
    set_dac_register(&mut i2c, 0x32, 0x80);
    set_dac_register(&mut i2c, 0x32, 0x00);

    // step 6 of 4.9 of CS43L22 datasheet
    set_dac_register(&mut i2c, 0x02, 0x9e);

    let adc = peripherals.ADC1;

    // buffer that can hold a second of data
    let mut buffer = [0u16; SAMPLE_RATE];

    // run the ADC on TIM2_TRGO (that is: at TIMER_RATE samples/sec) and have it request DMA.
    // only want one measurement each clock, from channel 0
    adc.sqr1
        .write(|w| w.l().bits(0 /* datasheet says "0b0000: 1 conversion" */));
    adc.sqr3.write(|w| unsafe { w.sq1().bits(0) });
    // run the ADC clock at pclk2 (84MHz) / 8 = 10.5MHz.  That's now within the datasheet tolerance
    // for the ADC, and plenty fast to take one sample 48k times per second.
    peripherals.ADC_COMMON.ccr.modify(|_r, w| w.adcpre().div8());
    // set up the ADC for 12-bit samples, continuous
    adc.cr1.write(|w| w.res().twelve_bit());
    adc.cr2.write(|w| {
        w.exten().disabled();
        w.align().right();
        w.dma().disabled();
        w.cont().continuous();
        w.adon().enabled()
    });
    // and kick off the ADC
    adc.cr2.modify(|_r, w| w.swstart().start());

    for i in (0..buffer.len()).cycle() {
        // wait for the SPI peripheral to need another sample
        while !spi.sr.read().txe().bit() {}

        // give it another sample
        spi.dr.write(|w| w.dr().bits(buffer[i]));

        // and while we're waiting for that to get clocked-out, grab the current value from the ADC
        let new_sample = adc.dr.read().data().bits();

        // wait for the SPI peripheral to need the same sample for the other channel
        while !spi.sr.read().txe().bit() {}
        spi.dr.write(|w| w.dr().bits(buffer[i]));

        // write that sample 800ms into the future
        buffer[(i + (SAMPLE_RATE * 800 / 1000)) % buffer.len()] = new_sample;
    }

    panic!("cycle() should never complete");
}

use embedded_hal::blocking::i2c;

fn set_dac_register<I>(i2c: &mut I, register: u8, value: u8)
where
    I: i2c::Write,
    I::Error: core::fmt::Debug,
{
    i2c.write(ADDRESS, &[register, value]).unwrap();
}
