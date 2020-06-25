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
    let mut pin = gpiod.pd9.into_push_pull_output();
    pin.set_high().unwrap();

    let gpioa = peripherals.GPIOA.split();
    let input = gpioa.pa0.into_floating_input();

    while input.is_low().unwrap() {}
    pin.into_floating_input();
    loop {}
}
