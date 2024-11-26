use std::time::Duration;

use anyhow::Result;
use common::{heapless, PingEndpoint, PID, VID};
use futures::pending;
use postcard_rpc::host_client::HostClient;
use postcard_rpc::{header::VarSeqKind, standard_icd::WireError};
use serialport::{SerialPortInfo, SerialPortType};
use tokio::{spawn, sync::mpsc, time::sleep};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<()> {
    // let ports = serialport::available_ports()?;

    // let mut path = None;
    // for port in ports {
    //     match port.port_type {
    //         SerialPortType::UsbPort(p) => {
    //             println!("{:?}", &p);
    //             if p.vid == VID && p.pid == PID {
    //                 println!("path determined to be {}", &port.port_name);
    //                 path = Some(port.port_name);
    //                 break;
    //             } else {
    //                 continue;
    //             }
    //         }
    //         _ => (),
    //     }
    // }

    // if let Some(p) = path {
    //     let host = RPCHost::new(&p, HostHandler)?;
    //     let (sx, rx) = mpsc::channel(4);
    //     spawn(host.run(rx));

    //     let mut ch = sx.call(LinkFrame::new(common::LinkVerb::Echo(
    //         "test".try_into().unwrap(),
    //     ))).await?;
    //     let mut ch = sx.call(LinkFrame::new(common::LinkVerb::Echo(
    //         "test2".try_into().unwrap(),
    //     ))).await?;

    //     pending!()

    // }
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::TRACE)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    let h: HostClient<WireError> =
        HostClient::new_raw_nusb(|d| d.product_id() == PID, "error", 8, VarSeqKind::Seq1);

    let rx = h.send_resp::<PingEndpoint>(&1).await;

    dbg!(&rx);

    h.wait_closed().await;

    Ok(())
}
