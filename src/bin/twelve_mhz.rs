#![no_main]
#![no_std]

use cortex_m_rt::entry;
use panic_itm as _;

use stm32f4xx_hal::prelude::*;
use stm32f4xx_hal::stm32;

#[entry]
fn main() -> ! {
    let peripherals = stm32f407g_disc::Peripherals::take().unwrap();

    let rcc = peripherals.RCC.constrain();
    let _clocks = rcc.cfgr.use_hse(8.mhz()).sysclk(168.mhz()).freeze();

    let portc = peripherals.GPIOC.split();
    let _mco2 = portc.pc9.into_alternate_af0();

    unsafe {
        let rcc = &*stm32::RCC::ptr();
        rcc.cfgr.modify(|_r, w| w.mco2().hse());
    }

    loop {
        cortex_m::asm::wfi();
    }
}
