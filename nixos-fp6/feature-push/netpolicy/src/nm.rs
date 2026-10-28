use crate::{
    config::valid_iface,
    policy::{metered, Metrics},
    runtime::run,
};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use zbus::{
    blocking::{Connection, Proxy},
    zvariant::OwnedObjectPath,
};

const NM: &str = "org.freedesktop.NetworkManager";
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Network {
    pub costly_interfaces: Vec<String>,
    pub tunnel_interfaces: Vec<String>,
    /// NetworkManager modem devices (app network permission: "mobile").
    #[serde(default)]
    pub mobile_interfaces: Vec<String>,
    pub wifi: Option<Wifi>,
    pub connectivity_uri: String,
    pub connectivity_enabled: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Wifi {
    pub interface: String,
    pub gateway: String,
    pub signal: u8,
    pub captive: bool,
}
fn proxy<'a>(bus: &'a Connection, path: &'a str, interface: &'a str) -> zbus::Result<Proxy<'a>> {
    Proxy::new(bus, NM, path, interface)
}
pub fn read(bus: &Connection) -> Result<Network> {
    let manager = proxy(bus, "/org/freedesktop/NetworkManager", NM)?;
    let devices: Vec<OwnedObjectPath> = manager.call("GetDevices", &())?;
    let mut network = Network {
        connectivity_uri: manager
            .get_property("ConnectivityCheckUri")
            .unwrap_or_default(),
        connectivity_enabled: manager
            .get_property("ConnectivityCheckEnabled")
            .unwrap_or(false),
        ..Network::default()
    };
    for path in &devices {
        let device = proxy(bus, path.as_str(), "org.freedesktop.NetworkManager.Device")?;
        let state: u32 = device.get_property("State")?;
        if state != 100 {
            continue;
        }
        let interface: String = device.get_property("IpInterface")?;
        if !valid_iface(&interface) {
            continue;
        }
        let kind: u32 = device.get_property("DeviceType")?;
        if matches!(kind, 16 | 17 | 29) {
            network.tunnel_interfaces.push(interface.clone());
        }
        let cost: u32 = device.get_property("Metered").unwrap_or(0);
        if kind == 8 {
            network.mobile_interfaces.push(interface.clone());
        }
        if kind == 8 || metered(cost) {
            network.costly_interfaces.push(interface.clone());
        }
        if kind == 2 {
            let wireless = proxy(
                bus,
                path.as_str(),
                "org.freedesktop.NetworkManager.Device.Wireless",
            )?;
            let ap: OwnedObjectPath = wireless.get_property("ActiveAccessPoint")?;
            let signal: u8 = proxy(
                bus,
                ap.as_str(),
                "org.freedesktop.NetworkManager.AccessPoint",
            )?
            .get_property("Strength")?;
            let mut gateway = String::new();
            for (property, iface) in [
                ("Ip4Config", "org.freedesktop.NetworkManager.IP4Config"),
                ("Ip6Config", "org.freedesktop.NetworkManager.IP6Config"),
            ] {
                if let Ok(ip) = device.get_property::<OwnedObjectPath>(property) {
                    if ip.as_str() != "/" {
                        gateway = proxy(bus, ip.as_str(), iface)?
                            .get_property("Gateway")
                            .unwrap_or_default();
                        if !gateway.is_empty() {
                            break;
                        }
                    }
                }
            }
            // Per-device connectivity, not the global property (which may describe mobile).
            let v4: u32 = device.get_property("Ip4Connectivity").unwrap_or(0);
            let v6: u32 = device.get_property("Ip6Connectivity").unwrap_or(0);
            network.wifi = Some(Wifi {
                interface,
                gateway,
                signal,
                captive: v4 == 2 || v6 == 2,
            });
        }
    }
    network.costly_interfaces.sort();
    network.costly_interfaces.dedup();
    Ok(network)
}
pub fn parse_ping(text: &str) -> Option<(f64, Option<f64>)> {
    let loss_line = text.lines().find(|l| l.contains("packet loss"))?;
    let before = loss_line.split("% packet loss").next()?;
    let loss = before.split_whitespace().last()?.parse::<f64>().ok()?;
    let avg = text
        .lines()
        .find(|l| l.contains("min/avg/max"))
        .and_then(|l| l.split('=').nth(1))
        .and_then(|s| s.trim().split('/').nth(1))
        .and_then(|s| s.parse().ok());
    Some((loss, avg))
}
pub fn probe(wifi: &Wifi, uri: Option<&str>) -> Result<Metrics> {
    ensure!(valid_iface(&wifi.interface), "invalid probe interface");
    let mut m = Metrics {
        signal: wifi.signal,
        captive: wifi.captive,
        ..Metrics::default()
    };
    if !wifi.gateway.is_empty() {
        // IPv6 link-local gateways use the explicit interface; no broadcast input.
        ensure!(
            wifi.gateway.parse::<std::net::IpAddr>().is_ok(),
            "invalid gateway address"
        );
        match run(
            "ping",
            &[
                "-n",
                "-I",
                &wifi.interface,
                "-c",
                "3",
                "-W",
                "1",
                "-w",
                "4",
                &wifi.gateway,
            ],
            None,
        ) {
            Ok(output) => {
                let (loss, avg) = parse_ping(&output).unwrap_or((100.0, None));
                m.loss_percent = loss;
                m.gateway_ms = avg;
            }
            Err(_) => {
                m.loss_percent = 100.0;
            }
        }
    }
    if let Some(uri) = uri.filter(|u| !u.is_empty()) {
        ensure!(
            uri.starts_with("https://") || uri.starts_with("http://"),
            "invalid probe URL"
        );
        // No redirects, cookies, credentials, or proxy. Bound to Wi-Fi
        // so a healthy mobile default route cannot masquerade as Wi-Fi recovery.
        let result = run(
            "curl",
            &[
                "--disable",
                "--silent",
                "--show-error",
                "--noproxy",
                "*",
                "--interface",
                &format!("if!{}", wifi.interface),
                "--connect-timeout",
                "2",
                "--max-time",
                "4",
                "--max-filesize",
                "4096",
                "--proto",
                "=http,https",
                "--include",
                "--write-out",
                "\nNETPOLICY_RESULT %{http_code} %{time_total}",
                "--url",
                uri,
            ],
            None,
        );
        match result {
            Ok(text) => {
                let (ok, ms) = parse_http(&text);
                m.public_ok = Some(ok);
                m.public_ms = ms;
            }
            Err(_) => m.public_ok = Some(false),
        }
    }
    Ok(m)
}
pub fn parse_http(text: &str) -> (bool, Option<f64>) {
    let Some((response, result)) = text.rsplit_once("\nNETPOLICY_RESULT ") else {
        return (false, None);
    };
    let mut words = result.split_whitespace();
    let code = words.next().unwrap_or("");
    let latency = words
        .next()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|n| n.is_finite() && *n >= 0.0)
        .map(|n| n * 1000.0);
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, b)| b)
        .unwrap_or("");
    let online_header = response
        .lines()
        .take_while(|line| !line.trim().is_empty())
        .any(|line| {
            line.trim()
                .eq_ignore_ascii_case("X-NetworkManager-Status: online")
        });
    (
        code == "204"
            || (code == "200" && (online_header || body.starts_with("NetworkManager is online"))),
        latency,
    )
}
