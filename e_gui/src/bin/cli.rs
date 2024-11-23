use std::time::Duration;

use anyhow::Result;
use common::{heapless, LinkFrame, PID, VID};
use futures::pending;
use ppproto::Config;
use serialport::{SerialPortInfo, SerialPortType};
use tokio::{spawn, sync::mpsc, time::sleep};
use ui::rpc::{HostHandler, RPCHost, RPCSending};
use common::rpc::FrameChan;

#[tokio::main]
async fn main() -> Result<()> {
    let ports = serialport::available_ports()?;

    let mut path = None;
    for port in ports {
        match port.port_type {
            SerialPortType::UsbPort(p) => {
                println!("{:?}", &p);
                if p.vid == VID && p.pid == PID {
                    println!("path determined to be {}", &port.port_name);
                    path = Some(port.port_name);
                    break;
                } else {
                    continue;
                }
            }
            _ => (),
        }
    }

    if let Some(p) = path {
        let host = RPCHost::new(&p, HostHandler)?;
        let (sx, rx) = mpsc::channel(4);
        spawn(host.run(rx));

        let mut ch = sx.call(LinkFrame::new(common::LinkVerb::Echo(
            "test".try_into().unwrap(),
        ))).await?;
        let mut ch = sx.call(LinkFrame::new(common::LinkVerb::Echo(
            "test2".try_into().unwrap(),
        ))).await?;

        pending!()

    }


    Ok(())
}
