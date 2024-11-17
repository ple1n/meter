#![feature(impl_trait_in_assoc_type)]
#![no_std]
#![no_main]

use core::future::pending;

use cortex_m::asm;

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
#[cfg(not(feature = "defmt"))]
use panic_halt as _;

use embassy_usb::{
    class::cdc_acm::{CdcAcmClass, State},
    UsbDevice,
};

use defmt::*;

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
use serde::{Deserialize, Serialize};
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

use embassy_sync::pipe;
const QUEUE_CAP: usize = 4;

/// link layer networking
#[embassy_executor::task]
async fn data_link(
    mut class: CdcAcmClass<'static, UsbDriver>,
    sx_down: pipe::Writer<'static, CriticalSectionRawMutex, QUEUE_CAP>,
    rx_up: pipe::Reader<'static, CriticalSectionRawMutex, QUEUE_CAP>
) -> ! {
    let conf = ppproto::Config {
        password: b"p",
        username: b"u",
    };
    let mut proto = ppproto::pppos::PPPoS::new(conf);
    proto.open().unwrap();
    crate::todo!()
}

#[derive(Serialize, Deserialize)]
enum LinkData {
    Ping,
}

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

    let config = embassy_usb::Config::new(0xc0de, 1);
    let driver = Driver::new(p.USB, Irqs, p.PA12, p.PA11);
    static CONF_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static BOS_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static CTRLBUF: StaticCell<[u8; 7]> = StaticCell::new();

    static STATE: StaticCell<State> = StaticCell::new();

    let mut builder = Builder::new(
        driver,
        config,
        &mut CONF_DESC.init([0; 256])[..],
        &mut BOS_DESC.init([0; 256])[..],
        &mut [], // no msos descriptors
        &mut CTRLBUF.init([0; 7])[..],
    );

    // Create classes on the builder.
    let class =
        embassy_usb::class::cdc_acm::CdcAcmClass::new(&mut builder, STATE.init(State::new()), 64);
    let usb = builder.build();

    unwrap!(spawner.spawn(usb_task(usb)));

    use embassy_sync::pipe::Pipe;
    type QueueType = LinkData;
    // packets to host
    static LINK_UP: StaticCell<Pipe<CriticalSectionRawMutex, QUEUE_CAP>> = StaticCell::new();
    // from host
    static LINK_DOWN: StaticCell<Pipe<CriticalSectionRawMutex, QUEUE_CAP>> = StaticCell::new();

    let (rx_up, sx_up) = LINK_UP.init(Pipe::new()).split();
    let (rx_down, sx_down) = LINK_DOWN.init(Pipe::new()).split();

    unwrap!(spawner.spawn(data_link(class, sx_down, rx_up)));

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

    loop {
        led.toggle();
        Timer::after_secs(1).await;
    }

    // info!("exit");
}
