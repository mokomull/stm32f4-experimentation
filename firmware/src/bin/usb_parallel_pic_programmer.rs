#![no_main]
#![no_std]

use panic_itm as _;

use stm32f407g_disc::entry;
use stm32f4xx_hal::prelude::*;

use usb_device::bus::{UsbBus, UsbBusAllocator};
use usb_device::class::UsbClass;
use usb_device::class_prelude::*;
use usb_device::prelude::*;

static mut USB_BUF: [u32; 128] = [0; 128];

#[entry]
fn main() -> ! {
    let peripherals = stm32f407g_disc::Peripherals::take().unwrap();
    let _core_peripherals = cortex_m::Peripherals::take().unwrap();

    let rcc = peripherals.RCC.constrain();
    let clocks = rcc
        .cfgr
        .use_hse(8.mhz())
        .sysclk(168.mhz())
        .require_pll48clk()
        .freeze();

    let porta = peripherals.GPIOA.split();

    let usb = stm32f4xx_hal::otg_fs::USB {
        hclk: clocks.hclk(),
        usb_global: peripherals.OTG_FS_GLOBAL,
        usb_device: peripherals.OTG_FS_DEVICE,
        usb_pwrclk: peripherals.OTG_FS_PWRCLK,
        pin_dp: porta.pa12.into_alternate_af10(),
        pin_dm: porta.pa11.into_alternate_af10(),
    };

    let bus = stm32f4xx_hal::otg_fs::UsbBus::new(usb, unsafe { &mut USB_BUF });
    let mut parallel = ParallelPort::new(&bus);
    let mut device = UsbDeviceBuilder::new(&bus, UsbVidPid(0x1337, 0xd00d))
        .manufacturer("Matt Mullins")
        .product("STM32F4 experiment")
        .build();

    loop {
        device.poll(&mut [&mut parallel]);
    }
}

struct ParallelPort<'a, B>
where
    B: UsbBus,
{
    interface: InterfaceNumber,
    ep_out: EndpointOut<'a, B>,
    ep_in: EndpointIn<'a, B>,
}

impl<'a, B> ParallelPort<'a, B>
where
    B: UsbBus,
{
    pub fn new(allocator: &'a UsbBusAllocator<B>) -> Self {
        let interface = allocator.interface();
        let ep_out = allocator.bulk::<usb_device::endpoint::Out>(8);
        let ep_in = allocator.bulk::<usb_device::endpoint::In>(8);
        Self {
            interface,
            ep_out,
            ep_in,
        }
    }
}

impl<'a, B> UsbClass<B> for ParallelPort<'a, B>
where
    B: UsbBus,
{
    fn get_configuration_descriptors(
        &self,
        writer: &mut DescriptorWriter,
    ) -> usb_device::Result<()> {
        writer.interface(self.interface, 0x07, 0x01, 0x01 /* unidirectional */)?;
        writer.endpoint(&self.ep_out)?;
        writer.endpoint(&self.ep_in)?;
        Ok(())
    }
}
