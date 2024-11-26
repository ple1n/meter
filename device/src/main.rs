#![feature(impl_trait_in_assoc_type)]
#![no_std]
#![no_main]

use core::future::pending;

use cortex_m::asm;

use embassy_futures::{join, select};
use embassy_sync::{
    blocking_mutex::raw::{CriticalSectionRawMutex, ThreadModeRawMutex},
    channel::{Channel, Receiver, Sender},
    pipe::{Reader, Writer},
};
#[cfg(not(feature = "defmt"))]
use panic_halt as _;

use embassy_usb::{
    class::cdc_acm::{CdcAcmClass, State},
    UsbDevice,
};

use defmt::{info, unwrap, warn};
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
use postcard_rpc::{
    define_dispatch,
    header::VarHeader,
    server::{
        impls::embassy_usb_v0_3::{
            dispatch_impl::{WireRxBuf, WireRxImpl, WireSpawnImpl, WireStorage, WireTxImpl},
            PacketBuffers,
        }, Dispatch, Server, ServerError, WireRx
    },
};
use serde::{Deserialize, Serialize};
use static_cell::{ConstStaticCell, StaticCell};

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
    loop {
        device.run_until_suspend().await;
        device.wait_resume().await;
        info!("resume")
    }
}

type AppDriver = usb::Driver<'static, peripherals::USB>;
type AppTx = WireTxImpl<ThreadModeRawMutex, AppDriver>;
type AppRx = WireRxImpl<AppDriver>;
type BufStorage = PacketBuffers<1024, 1024>;
type AppStorage = WireStorage<ThreadModeRawMutex, AppDriver, 256, 256, 64, 256>;
type AppServer = Server<AppTx, AppRx, WireRxBuf, MyApp>;

pub struct Context {}

static PBUFS: ConstStaticCell<BufStorage> = ConstStaticCell::new(BufStorage::new());
static STORAGE: AppStorage = AppStorage::new();

use common::*;

define_dispatch! {
    app: MyApp;
    spawn_fn: spawn_fn;
    tx_impl: AppTx;
    spawn_impl: WireSpawnImpl;
    context: Context;

    endpoints: {
        list: ENDPOINT_LIST;

        | EndpointTy                | kind      | handler                       |
        | ----------                | ----      | -------                       |
        | PingEndpoint              | blocking  |   ping_handler                |
    };
    topics_in: {
        list: TOPICS_IN_LIST;

        | TopicTy                   | kind      | handler                       |
        | ----------                | ----      | -------                       |
    };
    topics_out: {
        list: TOPICS_OUT_LIST;
    };
}

fn ping_handler(_context: &mut Context, _header: VarHeader, rqst: u32) -> u32 {
    info!("ping");
    
    rqst
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
    config.device_class = 0xEF;
    config.device_sub_class = 0x02;
    config.device_protocol = 0x01;
    config.composite_with_iads = true;
    {
        // BluePill board has a pull-up resistor on the D+ line.
        // Pull the D+ pin down to send a RESET condition to the USB bus.
        // This forced reset is needed only for development, without it host
        // will not reset your device when you upload new firmware.
        let _dp = Output::new(&mut p.PA12, Level::Low, Speed::Low);
        Timer::after_millis(10).await;
    }

    let driver = Driver::new(p.USB, Irqs, p.PA12, p.PA11);
    let pbufs = PBUFS.take();
    let context = Context {};
    let dispatcher = MyApp::new(context, spawner.into());
    let vkk = dispatcher.min_key_len();
    let (device, tx_impl, rx_impl) = STORAGE.init(driver, config, pbufs.tx_buf.as_mut_slice());
    let mut server: AppServer = Server::new(
        &tx_impl,
        rx_impl,
        pbufs.rx_buf.as_mut_slice(),
        dispatcher,
        vkk,
    );
    spawner.must_spawn(usb_task(device));
    
    loop {
        info!("wait usb");
        let x = server.run().await;
        match x {
            ServerError::TxFatal(s) => {
                info!("tx error {:?}", s as usize)
            },
            ServerError::RxFatal(r) => {
                info!("rx error")
            }
        }
        Timer::after_secs(1).await;
        
    }
    
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

    // info!("exit");
}
