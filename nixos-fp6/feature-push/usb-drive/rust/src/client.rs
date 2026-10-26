//! D-Bus client of the privileged helper (fp6-usb-drived).

use std::sync::Mutex;
use zbus::blocking::Connection;

pub const BUS_NAME: &str = "org.fairphone6.UsbDrive";
pub const PATH: &str = "/org/fairphone6/UsbDrive";
pub const IFACE: &str = "org.fairphone6.UsbDrive1";

static CONN: Mutex<Option<Connection>> = Mutex::new(None);

fn bus() -> Result<Connection, String> {
    let mut g = CONN.lock().unwrap();
    if let Some(c) = g.as_ref() {
        return Ok(c.clone());
    }
    // Tests: FP6_USB_DRIVE_BUS_ADDRESS (a private bus) or FP6_USB_DRIVE_SESSION_BUS.
    let c = if let Ok(addr) = std::env::var("FP6_USB_DRIVE_BUS_ADDRESS") {
        zbus::blocking::connection::Builder::address(addr.as_str()).and_then(|b| b.build())
    } else if std::env::var_os("FP6_USB_DRIVE_SESSION_BUS").is_some() {
        Connection::session()
    } else {
        Connection::system()
    }
    .map_err(|e| format!("system bus: {e}"))?;
    *g = Some(c.clone());
    Ok(c)
}

fn clean(e: zbus::Error) -> String {
    match e {
        zbus::Error::MethodError(_, Some(m), _) => m,
        zbus::Error::FDO(f) => match *f {
            zbus::fdo::Error::AccessDenied(m) | zbus::fdo::Error::Failed(m) | zbus::fdo::Error::InvalidArgs(m) => m,
            other => other.to_string(),
        },
        other => other.to_string(),
    }
}

fn call<B>(method: &str, body: &B) -> Result<String, String>
where
    B: serde::ser::Serialize + zbus::zvariant::DynamicType,
{
    let c = bus()?;
    let r = c.call_method(Some(BUS_NAME), PATH, Some(IFACE), method, body).map_err(|e| {
        if matches!(e, zbus::Error::InputOutput(_)) {
            CONN.lock().unwrap().take();
        }
        clean(e)
    })?;
    r.body().deserialize::<String>().map_err(|e| e.to_string())
}

pub fn status() -> Result<String, String> {
    call("GetStatus", &())
}

pub fn host(path: &str, mode: &str, exclusive: bool) -> Result<String, String> {
    call("Host", &(path, mode, exclusive))
}

pub fn stop() -> Result<String, String> {
    call("Stop", &())
}
