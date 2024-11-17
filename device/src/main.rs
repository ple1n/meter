#![feature(impl_trait_in_assoc_type)]
#![no_std]
#![no_main]

use core::future::pending;

use cortex_m::asm;
use defmt::*;

use embassy_net::Stack;
#[cfg(not(feature = "defmt"))]
use panic_halt as _;

use embassy_usb::class::cdc_ncm::embassy_net::{Runner, State as NetState, Device};
use embassy_usb::{
    class::cdc_ncm::{CdcNcmClass, State},
    UsbDevice,
};

use embassy_executor::Spawner;
use embassy_stm32::{
    bind_interrupts,
    gpio::{Level, Output, Speed},
    i2c::{self, I2c},
    peripherals,
    time::Hertz,
    usb::{self, Driver},
};
use embassy_time::{Duration, Timer};
use static_cell::StaticCell;
#[cfg(feature = "defmt")]
use {defmt_rtt as _, panic_probe as _};

type UsbDriver = embassy_stm32::usb::Driver<'static, embassy_stm32::peripherals::USB>;

bind_interrupts!(struct Irqs {
    I2C1_EV => i2c::EventInterruptHandler<peripherals::I2C1>;
    I2C1_ER => i2c::ErrorInterruptHandler<peripherals::I2C1>;
    USB_LP_CAN1_RX0 => usb::InterruptHandler<peripherals::USB>;
});

#[embassy_executor::task]
async fn usb_task(mut device: UsbDevice<'static, UsbDriver>) -> ! {
    device.run().await
}

#[embassy_executor::task]
async fn usb_ncm_task(class: Runner<'static, UsbDriver, MTU>) -> ! {
    class.run().await
}

#[embassy_executor::task]
async fn net_task(stack: &'static Stack<Device<'static, MTU>>) -> ! {
    stack.run().await
}


const MTU: usize = 32;

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());
    let mut led = Output::new(p.PC13, Level::High, Speed::Low);
    let mut i2conf = i2c::Config::default();
    i2conf.scl_pullup = true;
    i2conf.sda_pullup = true;

    let mut i2c = I2c::new(
        p.I2C1,
        p.PB6,
        p.PB7,
        Irqs,
        p.DMA1_CH6,
        p.DMA1_CH7,
        Hertz::hz(20),
        i2conf,
    );

    use embassy_stm32::usb::Driver;
    use embassy_usb::Builder;

    use embassy_net::tcp::TcpSocket;
    use embassy_net::{Stack, StackResources};

    // static OUTPUT_BUFFER: StaticCell<[u8; 256]> = StaticCell::new();
    // let ep_out_buffer = &mut OUTPUT_BUFFER.init([0; 256])[..];

    let config = embassy_usb::Config::new(0xc0de, 1);
    let driver = Driver::new(p.USB, Irqs, p.PA12, p.PA11);

    static CONFIG_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static BOS_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static CONTROL_BUF: StaticCell<[u8; 128]> = StaticCell::new();
    let mut builder = Builder::new(
        driver,
        config,
        &mut CONFIG_DESC.init([0; 256])[..],
        &mut BOS_DESC.init([0; 256])[..],
        &mut [], // no msos descriptors
        &mut CONTROL_BUF.init([0; 128])[..],
    );

    // Create classes on the builder.
    static STATE: StaticCell<State> = StaticCell::new();

    let mut mac_host = [0; 6];
    mac_host.copy_from_slice("lab_up".as_bytes());
    let mut mac_device = [0; 6];
    mac_device.copy_from_slice("labdf1".as_bytes());
    let class = CdcNcmClass::new(&mut builder, STATE.init(State::new()), mac_host, 32);

    let usb = builder.build();
    unwrap!(spawner.spawn(usb_task(usb)));

    static NET_STATE: StaticCell<NetState<MTU, 4, 4>> = StaticCell::new();
    let (runner, device) =
        class.into_embassy_net_device::<MTU, 4, 4>(NET_STATE.init(NetState::new()), mac_device);
    unwrap!(spawner.spawn(usb_ncm_task(runner)));

    // Timer::after_millis(5).await;
    // let measure = [0x70u8, 0xac, 0x33, 0x00];
    // loop {
    //     let rx = i2c.write(0, &measure).await;
    //     if rx.is_ok() {
    //         break;
    //     } else {
    //         info!("{}", rx);
    //         Timer::after_millis(1000).await;
    //     }
    // }
    // Timer::after_millis(80).await;
    // let status = [0x71u8];
    // unwrap!(i2c.write(0, &status).await);
    // let mut buf = [0u8; 4];
    // let rx = i2c.read(0, &mut buf).await;
    // info!("read {}, {}", buf, rx);
    let config = embassy_net::Config::dhcpv4(Default::default());
    // Init network stack
    static STACK: StaticCell<Stack<Device<'static, MTU>>> = StaticCell::new();
    static RESOURCES: StaticCell<StackResources<1>> = StaticCell::new();
    let stack = &*STACK.init(Stack::new(
        device,
        config,
        RESOURCES.init(StackResources::<1>::new()),
        1u64,
    ));

    unwrap!(spawner.spawn(net_task(stack)));


    loop {
        led.toggle();
        Timer::after_secs(1).await;
    }

    // info!("exit");
}
