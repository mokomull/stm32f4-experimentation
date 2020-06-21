#![no_main]
#![no_std]

use panic_itm as _;
use stm32f407g_disc::entry;

use stm32f407g_disc::hal::prelude::*;

use stm32f407g_disc::hal::gpio::{gpiod::PD13, Output, PushPull};
use stm32f407g_disc::hal::interrupt;

static mut LED: *mut PD13<Output<PushPull>> = 0 as *mut _;

#[entry]
fn main() -> ! {
    let peripherals = stm32f407g_disc::Peripherals::take().unwrap();
    let core_peripherals = cortex_m::Peripherals::take().unwrap();

    let rcc = peripherals.RCC.constrain();
    let clocks = rcc.cfgr.use_hse(8.mhz()).sysclk(168.mhz()).freeze();

    let gpiod = peripherals.GPIOD.split();
    let pin = gpiod.pd13.into_push_pull_output();

    unsafe {
        LED = &pin as *const _ as *mut _;
    }

    peripherals.SYSCFG.exticr1.modify(|_r, w| unsafe {
        w.exti0().bits(0 /* PORTA */)
    });
    peripherals.EXTI.rtsr.modify(|_r, w| w.tr0().set_bit());
    peripherals.EXTI.imr.modify(|_r, w| w.mr0().set_bit());
    unsafe {
        stm32f407g_disc::stm32::NVIC::unmask(interrupt::EXTI0);
    }

    loop {
        cortex_m::asm::wfi();
    }
}

#[interrupt]
fn EXTI0() {
    let pin = unsafe { &mut *LED };
    pin.set_high().unwrap();

    let exti = unsafe { &*stm32f407g_disc::stm32::EXTI::ptr() };
    exti.pr.modify(|_r, w| w.pr0().set_bit());
}
