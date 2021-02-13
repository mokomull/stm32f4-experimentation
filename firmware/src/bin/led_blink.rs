#![no_main]
#![no_std]

use cortex_m_rt::entry;
use panic_itm as _;

use stm32f4xx_hal::prelude::*;

#[entry]
fn main() -> ! {
    let peripherals = stm32f4xx_hal::stm32::Peripherals::take().unwrap();
    let core_peripherals = cortex_m::Peripherals::take().unwrap();

    let rcc = peripherals.RCC.constrain();
    let clocks = rcc.cfgr.sysclk(168.mhz()).freeze();

    let portd = peripherals.GPIOD.split();
    let mut led = portd.pd14.into_push_pull_output();
    let mut delay = stm32f4xx_hal::delay::Delay::new(core_peripherals.SYST, clocks);

    loop {
        led.set_high().unwrap();
        delay.delay_ms(250u32);
        led.set_low().unwrap();
        delay.delay_ms(250u32);
    }
}
