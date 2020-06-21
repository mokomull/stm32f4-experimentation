#![deny(unsafe_code)]
#![no_main]
#![no_std]

use panic_itm as _;
use stm32f407g_disc::entry;

use stm32f407g_disc::hal::gpio::GpioExt;
use stm32f407g_disc::led::LedColor::Orange;


#[entry]
fn main() -> ! {
    let peripherals = stm32f407g_disc::Peripherals::take().unwrap();

    let gpiod = peripherals.GPIOD.split();
    let mut leds = stm32f407g_disc::led::Leds::new(gpiod);

    loop {
        leds[Orange].on();
        leds[Orange].off();
    }
}
