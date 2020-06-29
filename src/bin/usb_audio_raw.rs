#![no_main]
#![no_std]

use panic_itm as _;

use stm32f407g_disc::entry;
use stm32f4xx_hal::prelude::*;
use stm32f4xx_hal::stm32::spi1::i2scfgr::{CHLEN_A, CKPOL_A, DATLEN_A, I2SCFG_A};
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
        // from the table in the SPI peripheral documentation, for 48ksps, 16bits/sample,
        // N = 192MHz, R = 5
        rcc.plli2scfgr.modify(|_r, w| {
            // the HSE clock is divided to 2MHz.
            // and 48ksps * 32bits is more than I can shovel off of USB.  Drop that down to 24 bits
            // per sample, or 2/3 * 192 = 128MHz.  That's still within the PLL range, yay!
            w.plli2sn().bits(128 / 2);
            w.plli2sr().bits(5)
        });
        rcc.cr.modify(|_r, w| w.plli2son().set_bit());
        while !rcc.cr.read().plli2srdy().bit() {}
    }
    let spi = peripherals.SPI2;
    spi.i2scfgr.write(|w| {
        w.i2smod().set_bit();
        w.i2scfg().variant(I2SCFG_A::MASTERRX);
        w.ckpol().variant(CKPOL_A::IDLEHIGH);
        w.datlen().variant(DATLEN_A::SIXTEENBIT);
        w.chlen().variant(CHLEN_A::SIXTEENBIT)
    });
    // from that same table, I2SDIV = 12, ODD = 1
    spi.i2spr.write(|w| {
        unsafe { w.i2sdiv().bits(12) };
        w.odd().set_bit()
    });
    spi.i2scfgr.modify(|_r, w| w.i2se().set_bit());

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
                let data: u16 = spi.dr.read().dr().bits();
                let bytes = data.to_be_bytes();
                let _ = serial.write(&bytes);
            }
        }
    }
}
