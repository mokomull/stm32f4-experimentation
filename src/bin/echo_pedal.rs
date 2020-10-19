#![no_main]
#![no_std]

use cortex_m_rt::entry;
use panic_itm as _;

use stm32f4xx_hal::prelude::*;

use stm32f4xx_hal::stm32;

// 84MHz, since I suppose the APBx prescaler causes the timer clock to be doubled...
const TIMER_CLOCK_RATE: usize = 84_000_000;
const SAMPLE_RATE: usize = 48_000;
// the timer won't behave correctly if the sample rate is not an exact integer number of ticks
static_assertions::const_assert_eq!(TIMER_CLOCK_RATE % SAMPLE_RATE, 0);
// nor if it takes more than 32 bits to represent the delay
static_assertions::const_assert!(TIMER_CLOCK_RATE / SAMPLE_RATE <= 0xFFFF_FFFF);

#[entry]
fn main() -> ! {
    let peripherals = stm32f407g_disc::Peripherals::take().unwrap();
    let mut core_peripherals = cortex_m::Peripherals::take().unwrap();

    let rcc = peripherals.RCC.constrain();
    let clocks = rcc.cfgr.use_hse(8.mhz()).sysclk(168.mhz()).freeze();

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

    let dma1 = peripherals.DMA1;
    let dma2 = peripherals.DMA2;
    let timer = peripherals.TIM2;
    let adc = peripherals.ADC1;
    let dac = peripherals.DAC;

    // buffer that can hold a second of data
    let mut buffer = [0u16; SAMPLE_RATE];

    // set up DMA2 to read from ADC1 into buffer, in a circular fashion
    let adc_stream = &dma2.st[0];
    // from the ADC
    adc_stream
        .par
        .write(|w| unsafe { w.bits(&adc.dr as *const _ as u32) });
    // to buffer
    adc_stream
        .m0ar
        .write(|w| unsafe { w.bits(&mut buffer[0] as *mut _ as u32) });
    // how many samples
    adc_stream.ndtr.write(|w| w.ndt().bits(buffer.len() as u16));
    // and let 'er rip!
    adc_stream.cr.write(|w| {
        w.chsel().bits(0);
        // everything is a single sample at a time
        w.mburst().single();
        w.pburst().single();
        // we want circular, not double-buffered
        w.dbm().disabled();
        // 16 bits at a time
        w.msize().bits16();
        w.psize().bits16();
        // increment memory address, but read from the ADC every time
        w.minc().incremented();
        w.pinc().fixed();
        // again, we want circular
        w.circ().enabled();
        // we're reading from the ADC
        w.dir().peripheral_to_memory();
        // the DMA controller knows how many bytes to transfer
        w.pfctrl().dma();
        // finally: enable the DMA stream!
        w.en().enabled()
    });
    // prevent anything below from touching this stream accidentally
    #[allow(unused_variables)]
    let adc_stream = ();

    // set up DMA1 to read from the buffer into DAC
    // this is the same as above, except:
    //   * peripheral #1, stream 5, channel 7
    //   * register address is different
    //   * direction is flipped
    let dac_stream = &dma1.st[5];
    // to the DAC
    dac_stream
        .par
        .write(|w| unsafe { w.bits(&dac.dhr12r1 as *const _ as u32) });
    // from the buffer
    dac_stream
        .m0ar
        .write(|w| unsafe { w.bits(&mut buffer[0] as *mut _ as u32) });
    // how many samples
    dac_stream.ndtr.write(|w| w.ndt().bits(buffer.len() as u16));
    // and let 'er rip!
    dac_stream.cr.write(|w| {
        w.chsel().bits(7);
        // everything is a single sample at a time
        w.mburst().single();
        w.pburst().single();
        // we want circular, not double-buffered
        w.dbm().disabled();
        // 16 bits at a time
        w.msize().bits16();
        w.psize().bits16();
        // increment memory address, but read from the ADC every time
        w.minc().incremented();
        w.pinc().fixed();
        // again, we want circular
        w.circ().enabled();
        // we're writing to the DAC
        w.dir().memory_to_peripheral();
        // the DMA controller knows how many bytes to transfer
        w.pfctrl().dma();
        // finally: enable the DMA stream!
        w.en().enabled()
    });
    // prevent anything below from touching this stream accidentally too
    #[allow(unused_variables)]
    let dac_stream = ();

    // subtract one because the timer iterates from zero through (and including) this value.
    timer
        .arr
        .write(|w| w.arr().bits((TIMER_CLOCK_RATE / SAMPLE_RATE - 1) as u32));
    timer.cr2.write(|w| w.mms().update()); // send a TRGO event when the timer updates
    timer.cr1.write(|w| w.cen().set_bit());

    // run the ADC on TIM2_TRGO (that is: at SAMPLE_RATE samples/sec) and have it request DMA.
    // only want one measurement each clock, from channel 0
    adc.sqr1
        .write(|w| w.l().bits(0 /* datasheet says "0b0000: 1 conversion" */));
    adc.sqr3.write(|w| unsafe { w.sq1().bits(0) });
    // set up the ADC for 12-bit samples, DMA
    adc.cr1.write(|w| w.res().twelve_bit());
    adc.cr2.write(|w| {
        w.exten().rising_edge();
        w.extsel().tim2trgo();
        w.align().right();
        w.dds().continuous(); // ignore the "last" DMA transfer, since it's circular
        w.dma().enabled();
        w.adon().enabled()
    });

    // sleep for 100ms to set up a delay, before having the DAC dequeue samples
    let mut delay = stm32f4xx_hal::delay::Delay::new(core_peripherals.SYST, clocks);
    delay.delay_ms(100u8);

    // now make the DAC start triggering DMA
    dac.cr.write(|w| {
        w.dmaen1().enabled();
        unsafe {
            w.tsel1().bits(0b100 /* timer 2 TRGO */)
        };
        w.ten1().enabled();
        w.en1().set_bit()
    });

    loop {
        cortex_m::asm::wfi();
    }
}
