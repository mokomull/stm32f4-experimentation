#![deny(unsafe_code)]
#![no_main]
#![no_std]

use stm32f407g_disc::entry;
use panic_itm as _;

use cortex_m::iprintln;
// use cortex_m_rt::entry;

#[entry]
fn main() -> ! {
    let peripherals = cortex_m::Peripherals::take().unwrap();
    let mut itm = peripherals.ITM;

    iprintln!(&mut itm.stim[0], "Hello, world!");

    loop {}
}
