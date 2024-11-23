#![feature(impl_trait_in_assoc_type)]
#![no_std]
#![no_main]

use core::future::pending;

use common::{LinkFrame, PID, VID};
use cortex_m::asm;

use embassy_futures::{join, select};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::{Channel, Receiver, Sender},
    pipe::{Reader, Writer},
};
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
use ppproto::pppos::PPPoSAction;
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

const LINK_CAP: usize = 4;

pub mod rpc;

/// link layer networking running on PPP
#[embassy_executor::task]
async fn data_link(
    class: CdcAcmClass<'static, UsbDriver>,
    sx_down: Sender<'static, CriticalSectionRawMutex, LinkFrame, LINK_CAP>,
    rx_up: Receiver<'static, CriticalSectionRawMutex, LinkFrame, LINK_CAP>,
) -> ! {
    let (mut sx, mut rx) = class.split();
    loop {
        info!("wait for usb");

        rx.wait_connection().await;
        let conf = ppproto::Config {
            password: b"p",
            username: b"u",
        };

        let mut proto = ppproto::pppos::PPPoS::new(conf);
        const BUF_SIZE: usize = 256;
        let mut tbuf = [0; BUF_SIZE];
        let mut rbuf = [0; BUF_SIZE];

        let mut up_buf = [0; BUF_SIZE];
        // pointer
        let mut consumed = 0usize;
        let mut data_end = 0usize;

        let mut read_buf = [0; BUF_SIZE];

        proto.open().unwrap();

        loop {
            match select::select(rx.read_packet(&mut read_buf), rx_up.receive()).await {
                select::Either::First(result) => match result {
                    Ok(size) => {
                        info!("read {}", size);
                        consumed = 0;
                        data_end = size;
                    }
                    Err(e) => {
                        warn!("{:?}", &e);
                        break;
                    }
                },
                select::Either::Second(packet) => {
                    let bytes = postcard::to_slice(&packet, &mut up_buf[..]);
                    match bytes {
                        Ok(byt) => {
                            proto.send(&byt, &mut tbuf).unwrap();
                        }
                        Err(e) => {
                            warn!("sending; encode error");
                        }
                    }
                }
            }
            loop {
                if (&read_buf[consumed..data_end]).len() > 0 {
                    info!("cons {} {}", consumed, data_end);
                    info!("cons {:?}", &read_buf[consumed..data_end]);
                    let n = proto.consume(&read_buf[consumed..data_end], &mut rbuf);
                    consumed += n;
                }
                
                match proto.poll(&mut tbuf, &mut rbuf) {
                    PPPoSAction::Transmit(n) => {
                        unwrap!(sx.write_packet(&tbuf[..n]).await);
                        tbuf.fill(0);
                    }
                    PPPoSAction::Received(n) => {
                        info!("recv {:?}", &n);
                        let de: Result<LinkFrame, postcard::Error> = postcard::from_bytes(&rbuf[n]);
                        match de {
                            Ok(data) => {
                                sx_down.send(data).await;
                            }
                            Err(e) => {
                                warn!("decode err");
                            }
                        }
                    }
                    PPPoSAction::None => {
                        info!("ppp:none {}", proto.status());
                        break;
                    },
                }
            }
        }
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    use embassy_stm32::{bind_interrupts, peripherals, usb, Config};

    let mut config = Config::default();
    {
        use embassy_stm32::rcc::*;

        config.rcc.hse = Some(Hse {
            freq: Hertz(8_000_000),
            // Oscillator for bluepill, Bypass for nucleos.
            mode: HseMode::Oscillator,
        });
        config.rcc.pll = Some(Pll {
            src: PllSource::HSE,
            prediv: PllPreDiv::DIV1,
            mul: PllMul::MUL9,
        });
        config.rcc.sys = Sysclk::PLL1_P;
        config.rcc.ahb_pre = AHBPrescaler::DIV1;
        config.rcc.apb1_pre = APBPrescaler::DIV2;
        config.rcc.apb2_pre = APBPrescaler::DIV1;
    }
    let mut p = embassy_stm32::init(config);

    let mut led = Output::new(p.PC13, Level::High, Speed::Low);
    let mut i2conf = i2c::Config::default();

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

    let mut config = embassy_usb::Config::new(VID, PID);
    config.manufacturer = Some("Plein");
    config.product = Some("meter");

    {
        // BluePill board has a pull-up resistor on the D+ line.
        // Pull the D+ pin down to send a RESET condition to the USB bus.
        // This forced reset is needed only for development, without it host
        // will not reset your device when you upload new firmware.
        let _dp = Output::new(&mut p.PA12, Level::Low, Speed::Low);
        Timer::after_millis(10).await;
    }

    let driver = Driver::new(p.USB, Irqs, p.PA12, p.PA11);
    static CONF_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static BOS_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static CTRLBUF: StaticCell<[u8; 128]> = StaticCell::new();

    static STATE: StaticCell<State> = StaticCell::new();

    let mut builder = Builder::new(
        driver,
        config,
        &mut CONF_DESC.init([0; 256])[..],
        &mut BOS_DESC.init([0; 256])[..],
        &mut [], // no msos descriptors
        &mut CTRLBUF.init([0; 128])[..],
    );

    // Create classes on the builder.
    let class =
        embassy_usb::class::cdc_acm::CdcAcmClass::new(&mut builder, STATE.init(State::new()), 64);
    let usb = builder.build();

    unwrap!(spawner.spawn(usb_task(usb)));

    // packets to host
    static LINK_UP: StaticCell<Channel<CriticalSectionRawMutex, LinkFrame, LINK_CAP>> =
        StaticCell::new();
    // packets from host to device
    static LINK_DOWN: StaticCell<Channel<CriticalSectionRawMutex, LinkFrame, LINK_CAP>> =
        StaticCell::new();

    let link_up = LINK_UP.init(Channel::new());
    let link_down = LINK_DOWN.init(Channel::new());

    unwrap!(spawner.spawn(data_link(class, link_down.sender(), link_up.receiver())));

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
        let re = link_down.receive().await;

        info!("{:?}", &re);
    }

    // info!("exit");
}
