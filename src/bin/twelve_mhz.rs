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
    let _clocks = rcc.cfgr.use_hse(8.mhz()).sysclk(167_500.khz()).freeze();

    let portc = peripherals.GPIOC.split();
    let _mco2 = portc.pc9.into_alternate_af0();

    unsafe {
        let rcc = &*stm32::RCC::ptr();

        rcc.plli2scfgr.write(|w| {
            w.plli2sn().bits(60);
            w.plli2sr().bits(5)
        });
        rcc.cr.modify(|_r, w| w.plli2son().set_bit());
        while !rcc.cr.read().plli2srdy().bit() {}

        rcc.cfgr.modify(|_r, w| w.mco2().plli2s());
    }

    loop {
        cortex_m::asm::wfi();
    }
}
