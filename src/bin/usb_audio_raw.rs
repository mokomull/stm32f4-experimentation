#![no_main]
#![no_std]

use panic_itm as _;

use stm32f407g_disc::entry;
use stm32f4xx_hal::prelude::*;
use usb_device::prelude::*;

static mut USB_BUF: [u32; 128] = [0; 128];

#[entry]
fn main() -> ! {
    let peripherals = stm32f407g_disc::Peripherals::take().unwrap();
    let core_peripherals = cortex_m::Peripherals::take().unwrap();

    let rcc = peripherals.RCC.constrain();
    let clocks = rcc
        .cfgr
        .use_hse(8.mhz())
        .sysclk(168.mhz())
        .require_pll48clk()
        .freeze();

    let porta = peripherals.GPIOA.split();

    let usb = stm32f4xx_hal::otg_fs::USB {
        usb_global: peripherals.OTG_FS_GLOBAL,
        usb_device: peripherals.OTG_FS_DEVICE,
        usb_pwrclk: peripherals.OTG_FS_PWRCLK,
        pin_dp: porta.pa12.into_alternate_af10(),
        pin_dm: porta.pa11.into_alternate_af10(),
    };

    let bus = stm32f4xx_hal::otg_fs::UsbBus::new(usb, unsafe { &mut USB_BUF });
    let mut serial = usbd_serial::SerialPort::new(&bus);
    let mut device = UsbDeviceBuilder::new(&bus, UsbVidPid(0x1337, 0xd00d))
        .manufacturer("Matt Mullins")
        .product("STM32F4 experiment")
        .build();

    let mut send = false;

    loop {
        device.poll(&mut [&mut serial]);
        let mut buf = [0; 32];
        if let Ok(_count) = serial.read(&mut buf) {
            if buf[0] == b'A' {
                send = true;
            }
            if buf[0] == b'B' {
                send = false;
            }
        }

        if send {
            let _ = serial.write(&[b'A'; 128]);
            let _ = serial.flush();
        }
    }
}
