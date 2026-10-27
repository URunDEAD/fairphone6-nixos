use anyhow::{Result, bail};
use serde::Serialize;
use std::{net::Ipv4Addr, path::Path};

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct LanInterface {
    pub name: String,
    pub address: Ipv4Addr,
    pub netmask: Ipv4Addr,
}
impl LanInterface {
    pub fn contains(&self, peer: Ipv4Addr) -> bool {
        let mask = u32::from(self.netmask);
        peer.is_private() && (u32::from(peer) & mask) == (u32::from(self.address) & mask)
    }
}
pub fn interfaces() -> Result<Vec<LanInterface>> {
    let mut result = Vec::new();
    for iface in if_addrs::get_if_addrs()? {
        let sys = Path::new("/sys/class/net").join(&iface.name);
        if !(sys.join("wireless").exists() || sys.join("phy80211").exists()) {
            continue;
        }
        // IFF_UP is stable in sysfs; IFF_RUNNING is not exposed by all drivers.
        let flags = std::fs::read_to_string(sys.join("flags"))?;
        let flags = u32::from_str_radix(flags.trim().trim_start_matches("0x"), 16)?;
        if flags & 1 == 0 {
            continue;
        }
        if std::fs::read_to_string(sys.join("carrier")).is_ok_and(|v| v.trim() == "0") {
            continue;
        }
        if let if_addrs::IfAddr::V4(v4) = iface.addr
            && v4.ip.is_private()
            && !v4.netmask.is_unspecified()
        {
            result.push(LanInterface {
                name: iface.name,
                address: v4.ip,
                netmask: v4.netmask,
            });
        }
    }
    result.sort_by(|a, b| a.name.cmp(&b.name).then(a.address.cmp(&b.address)));
    Ok(result)
}
pub fn select(name: &str) -> Result<LanInterface> {
    let candidates: Vec<_> = interfaces()?
        .into_iter()
        .filter(|i| i.name == name)
        .collect();
    if candidates.len() != 1 {
        bail!("Choose a Wi-Fi/hotspot interface with exactly one private IPv4 address");
    }
    Ok(candidates[0].clone())
}

/// Bind both the chosen device and its address. Linux permits the initial
/// SO_BINDTODEVICE assignment without capabilities on modern kernels. If the
/// platform rejects it, fail closed instead of exposing another interface.
pub fn listener(interface: &LanInterface, port: u16) -> Result<tokio::net::TcpListener> {
    use socket2::{Domain, Protocol, Socket, Type};
    let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))?;
    socket.bind_device(Some(interface.name.as_bytes()))?;
    socket.set_reuse_address(true)?;
    socket.set_nonblocking(true)?;
    socket.bind(&std::net::SocketAddr::from((interface.address, port)).into())?;
    socket.listen(64)?;
    Ok(tokio::net::TcpListener::from_std(socket.into())?)
}
