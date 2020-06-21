#![no_main]
#![no_std]

use panic_itm as _;
use stm32f407g_disc::entry;

use stm32f407g_disc::hal::prelude::*;

use stm32f407g_disc::hal::gpio::{gpiod::PD13, Floating, Input};

#[entry]
fn main() -> ! {
    let peripherals = stm32f407g_disc::Peripherals::take().unwrap();
    let core_peripherals = cortex_m::Peripherals::take().unwrap();

    let rcc = peripherals.RCC.constrain();
    let clocks = rcc.cfgr.use_hse(8.mhz()).sysclk(168.mhz()).freeze();

    let gpiod = peripherals.GPIOD.split();
    let pin = gpiod.pd13.into_floating_input();

    let gpioa = peripherals.GPIOA.split();
    let input = gpioa.pa0.into_floating_input();

    loop {
        if input.is_high().unwrap() {
            let mut pin = pin.into_push_pull_output();
            pin.set_high().unwrap();
            loop {} // can't re-run the outer loop once we've moved out of pin, so let's get stuck here
        }
    }
}
