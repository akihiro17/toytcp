use anyhow::Result;
use std::{env, io, net::Ipv4Addr, str};
use toytcp::tcp::TCP;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let addr: Ipv4Addr = args[1].parse()?;
    let port: u16 = args[2].parse()?;

    echo_client(addr, port)?;
    return Ok(());
}

fn echo_client(addr: Ipv4Addr, port: u16) -> Result<()> {
    let tcp = TCP::new();
    let _ = tcp.connect(addr, port)?;
    Ok(())
}