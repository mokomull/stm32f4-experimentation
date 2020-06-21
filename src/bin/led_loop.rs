#![deny(unsafe_code)]
#![no_main]
#![no_std]

use panic_itm as _;
use stm32f407g_disc::entry;

use stm32f407g_disc::hal::prelude::*;
use stm32f407g_disc::led::LedColor::Orange;

#[entry]
fn main() -> ! {
    let peripherals = stm32f407g_disc::Peripherals::take().unwrap();

    let rcc = peripherals.RCC.constrain();
    let clocks = rcc.cfgr.use_hse(8.mhz()).sysclk(168.mhz()).freeze();

    let gpiod = peripherals.GPIOD.split();
    let mut leds = stm32f407g_disc::led::Leds::new(gpiod);
    let led = &mut leds[Orange];

    loop {
        led.on();
        led.off();
    }
}
