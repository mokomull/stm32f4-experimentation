#![no_main]
#![no_std]

use panic_itm as _;
use cortex_m_rt::entry;

use stm32f7xx_hal::prelude::*;

use stm32f7xx_hal::device::gpiod::{
    moder::MODER15_A::{INPUT, OUTPUT},
    otyper::OT15_A::PUSHPULL,
    pupdr::PUPDR15_A::FLOATING,
};

#[entry]
fn main() -> ! {
    let peripherals = stm32f7xx_hal::device::Peripherals::take().unwrap();
    let core_peripherals = cortex_m::Peripherals::take().unwrap();

    let rcc = peripherals.RCC.constrain();
    let clocks = rcc.cfgr.sysclk(168.mhz()).freeze();

    // GPIO port D is disabled at start-up; GPIO*.split() handled this for us in the past.
    unsafe {
        let rcc = &*stm32f7xx_hal::device::RCC::ptr();
        rcc.ahb1enr.modify(|_r, w| w.gpioden().set_bit());
    }

    let gpiod = peripherals.GPIOD;
    gpiod.pupdr.write(|w| w.pupdr7().variant(FLOATING));
    gpiod.otyper.write(|w| w.ot7().variant(PUSHPULL));
    gpiod.moder.write(|w| w.moder7().variant(OUTPUT));
    gpiod.odr.write(|w| w.odr7().set_bit());

    let gpioc = peripherals.GPIOC.split();
    let input = gpioc.pc13.into_floating_input();

    while input.is_low().unwrap() {}
    gpiod.moder.write(|w| w.moder7().variant(INPUT));
    loop {}
}
