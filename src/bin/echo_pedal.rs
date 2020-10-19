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
// nor if it takes more than 16 bits to represent the delay
static_assertions::const_assert!(TIMER_CLOCK_RATE / SAMPLE_RATE <= 65536);

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

    // enable the DAC peripheral
    unsafe {
        let rcc = &*stm32::RCC::ptr();
        rcc.apb1enr.modify(|_r, w| {
            w.dacen().set_bit();
            w.tim4en().set_bit()
        });
        rcc.ahb1enr.modify(|_r, w| w.dma1en().set_bit());
    }

    loop {}
}
