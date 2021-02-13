#![no_main]
#![no_std]

use panic_itm as _;
use stm32f407g_disc::entry;

use stm32f407g_disc::hal::prelude::*;

use stm32f407g_disc::stm32::gpioi::{
    moder::MODER15_A::{INPUT, OUTPUT},
    otyper::OT15_A::PUSHPULL,
    pupdr::PUPDR15_A::FLOATING,
};

#[entry]
fn main() -> ! {
    let peripherals = stm32f407g_disc::Peripherals::take().unwrap();
    let core_peripherals = cortex_m::Peripherals::take().unwrap();

    let rcc = peripherals.RCC.constrain();
    let clocks = rcc.cfgr.use_hse(8.mhz()).sysclk(168.mhz()).freeze();

    // GPIO port D is disabled at start-up; GPIO*.split() handled this for us in the past.
    unsafe {
        let rcc = &*stm32f407g_disc::stm32::RCC::ptr();
        rcc.ahb1enr.modify(|_r, w| w.gpioden().set_bit());
    }

    let gpiod = peripherals.GPIOD;
    gpiod.pupdr.write(|w| w.pupdr9().variant(FLOATING));
    gpiod.otyper.write(|w| w.ot9().variant(PUSHPULL));
    gpiod.moder.write(|w| w.moder9().variant(OUTPUT));
    gpiod.odr.write(|w| w.odr9().set_bit());

    let gpioa = peripherals.GPIOA.split();
    let input = gpioa.pa0.into_floating_input();

    while input.is_low().unwrap() {}
    gpiod.moder.write(|w| w.moder9().variant(INPUT));
    loop {}
}
