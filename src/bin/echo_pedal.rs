#![no_main]
#![no_std]

use cortex_m_rt::entry;
use panic_itm as _;

use stm32f4xx_hal::prelude::*;

use stm32f4xx_hal::stm32;

use biquad::Biquad;

// 84MHz, since I suppose the APBx prescaler causes the timer clock to be doubled...
const TIMER_CLOCK_RATE: usize = 84_000_000;
const SAMPLE_RATE: usize = 48_000;
const SAMPLES_TO_AVERAGE: usize = 10;
const TIMER_RATE: usize = SAMPLE_RATE * SAMPLES_TO_AVERAGE;
// the timer won't behave correctly if the sample rate is not an exact integer number of ticks
static_assertions::const_assert_eq!(TIMER_CLOCK_RATE % TIMER_RATE, 0);
// nor if it takes more than 32 bits to represent the delay
static_assertions::const_assert!(TIMER_CLOCK_RATE / TIMER_RATE <= 0xFFFF_FFFF);

#[entry]
fn main() -> ! {
    let peripherals = stm32f407g_disc::Peripherals::take().unwrap();
    let mut core_peripherals = cortex_m::Peripherals::take().unwrap();

    let rcc = peripherals.RCC.constrain();
    let _clocks = rcc.cfgr.use_hse(8.mhz()).sysclk(168.mhz()).freeze();

    let _itm = &mut core_peripherals.ITM.stim[0];

    let porta = peripherals.GPIOA.split();

    // the DAC overrides what was selected in the GPIO module, but the datasheet recommended the pin
    // be switched to analog input.
    let _signal_out = porta.pa4.into_analog();
    let _signal_in = porta.pa0.into_analog();

    // enable the DAC peripheral
    unsafe {
        let rcc = &*stm32::RCC::ptr();
        rcc.apb1enr.modify(|_r, w| {
            w.dacen().set_bit();
            w.tim2en().set_bit()
        });
        rcc.ahb1enr.modify(|_r, w| {
            w.dma1en().set_bit();
            w.dma2en().set_bit()
        });
        rcc.apb2enr.modify(|_r, w| w.adc1en().set_bit())
    }

    let coeffs = biquad::Coefficients::<f32>::from_params(
        biquad::Type::LowPass,
        biquad::Hertz::<f32>::from_hz(TIMER_RATE as f32).unwrap(),
        biquad::Hertz::<f32>::from_hz(20_000.0).unwrap(),
        biquad::Q_BUTTERWORTH_F32,
    )
    .unwrap();
    let mut filter = [biquad::DirectForm2Transposed::<f32>::new(coeffs); 6];

    let dma2 = peripherals.DMA2;
    let timer = peripherals.TIM2;
    let adc = peripherals.ADC1;
    let dac = peripherals.DAC;

    // buffer that can hold a second of data
    let mut buffer = [0u16; SAMPLE_RATE];
    // and ones that can hold just enough from the ADC, to ping-pong between
    let mut adc_buffer = [[0u16; SAMPLES_TO_AVERAGE]; 2];

    // set up DMA2 to read from ADC1 into buffer, in a circular fashion
    let adc_stream = &dma2.st[0];
    // from the ADC
    adc_stream
        .par
        .write(|w| unsafe { w.bits(&adc.dr as *const _ as u32) });
    // to buffer
    adc_stream
        .m0ar
        .write(|w| unsafe { w.bits(&mut adc_buffer[0][0] as *mut _ as u32) });
    // how many samples
    adc_stream
        .ndtr
        .write(|w| w.ndt().bits(SAMPLES_TO_AVERAGE as u16));
    // and let 'er rip!
    adc_stream.cr.write(|w| {
        w.chsel().bits(0);
        // everything is a single sample at a time
        w.mburst().single();
        w.pburst().single();
        // we'll be managing our own buffers, not hardware double-buffered
        w.dbm().disabled();
        // 16 bits at a time
        w.msize().bits16();
        w.psize().bits16();
        // increment memory address, but read from the ADC every time
        w.minc().incremented();
        w.pinc().fixed();
        // again, we'll be managing this ourselves
        w.circ().disabled();
        // we're reading from the ADC
        w.dir().peripheral_to_memory();
        // the DMA controller knows how many bytes to transfer
        w.pfctrl().dma();
        // finally: enable the DMA stream!
        w.en().enabled()
    });

    // subtract one because the timer iterates from zero through (and including) this value.
    timer
        .arr
        .write(|w| w.arr().bits((TIMER_CLOCK_RATE / TIMER_RATE - 1) as u32));
    timer.cr2.write(|w| w.mms().update()); // send a TRGO event when the timer updates
    timer.cr1.write(|w| w.cen().set_bit());

    // run the ADC on TIM2_TRGO (that is: at TIMER_RATE samples/sec) and have it request DMA.
    // only want one measurement each clock, from channel 0
    adc.sqr1
        .write(|w| w.l().bits(0 /* datasheet says "0b0000: 1 conversion" */));
    adc.sqr3.write(|w| unsafe { w.sq1().bits(0) });
    // run the ADC clock at pclk2 (84MHz) / 8 = 10.5MHz.  That's now within the datasheet tolerance
    // for the ADC, and plenty fast to take one sample 48k times per second.
    peripherals.ADC_COMMON.ccr.modify(|_r, w| w.adcpre().div8());
    // set up the ADC for 12-bit samples, DMA
    adc.cr1.write(|w| w.res().twelve_bit());
    adc.cr2.write(|w| {
        w.exten().rising_edge();
        w.extsel().tim2trgo();
        w.align().right();
        w.dds().continuous();
        w.dma().enabled();
        w.adon().enabled()
    });

    // enable the DAC
    dac.cr.write(|w| w.en1().set_bit());

    for i in (0..buffer.len()).cycle() {
        let (this, next) = (i % 2, (i + 1) % 2);

        // wait for the DMA buffer to fill up
        while adc_stream.cr.read().en().bit() {}

        // set up DMA to the next adc_buffer
        adc_stream
            .m0ar
            .write(|w| w.m0a().bits(&adc_buffer[next][0] as *const _ as u32));
        adc_stream
            .ndtr
            .write(|w| w.ndt().bits(SAMPLES_TO_AVERAGE as u16));
        dma2.lifcr.write(|w| {
            w.ctcif0().set_bit();
            w.chtif0().set_bit();
            w.cteif0().set_bit();
            w.cdmeif0().set_bit();
            w.cfeif0().set_bit()
        });
        adc_stream.cr.modify(|_r, w| w.en().enabled());
        // clear the "end of conversion" flag so that the next iteration knows to wait
        adc.sr.modify(|_r, w| w.eoc().clear_bit());
        // in order to reset the "DMA completed" status, you have to turn DMA off
        adc.cr2.modify(|_r, w| w.dma().disabled());
        // and back on again
        adc.cr2.modify(|_r, w| w.dma().enabled());

        let mut new_sample = 0.0;
        for i in &adc_buffer[this] {
            new_sample = *i as f32 / 4096.0;
            for f in &mut filter {
                new_sample = f.run(new_sample);
            }
        }

        // write that sample 800ms into the future
        buffer[(i + (SAMPLE_RATE * 800 / 1000)) % buffer.len()] = (new_sample * 4096.0) as u16;

        // output to the DAC
        dac.dhr12r1
            .write(|w| unsafe { w.dacc1dhr().bits(buffer[i]) });

        // for sanity sake, make sure we didn't take too long computing the average
        if adc.sr.read().ovr().bit() {
            panic!("overran the ADC");
        }
    }

    panic!("cycle() should never complete");
}
