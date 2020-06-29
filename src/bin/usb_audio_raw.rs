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
    let mut core_peripherals = cortex_m::Peripherals::take().unwrap();

    let rcc = peripherals.RCC.constrain();
    let clocks = rcc
        .cfgr
        .use_hse(8.mhz())
        .sysclk(168.mhz())
        .require_pll48clk()
        .freeze();

    cortex_m::iprintln!(
        &mut core_peripherals.ITM.stim[0],
        "pclk1 is {}",
        clocks.pclk1().0
    );

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

    // set-up SPI
    let portb = peripherals.GPIOB.split();
    let portc = peripherals.GPIOC.split();
    portb.pb10.into_alternate_af5();
    portc.pc3.into_alternate_af5();
    unsafe {
        let rcc = &*stm32f4xx_hal::stm32::RCC::ptr();
        rcc.apb1enr.modify(|_r, w| w.spi2en().set_bit());
    }
    let spi = peripherals.SPI2;
    spi.cr1.write(|w| {
        w.bidimode().set_bit(); // the only way I can see to use MOSI for input
        w.bidioe().clear_bit(); // and input it
        w.br().bits(0x5); // 42MHz / 64 = 656.25kbit/s
        w.ssm().set_bit(); // no need for hardware slave-select, just pretend it's always asserted
        w.ssi().set_bit();
        w.mstr().set_bit();
        w.cpol().set_bit() // idle high, since the board is wired as the "left" mic
    });
    spi.cr1.modify(|_r, w| w.spe().set_bit());

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
            while spi.sr.read().rxne().bit() {
                let c: u8 = unsafe { core::ptr::read_volatile(&spi.dr as *const _ as *const u8) };
                let _ = serial.write(&[c]);
            }
        }
    }
}
