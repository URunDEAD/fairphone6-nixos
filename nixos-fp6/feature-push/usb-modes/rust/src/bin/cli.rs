//! fp6-usbmodes-cli: command line client for fp6-usbmoded.
//!   fp6-usbmodes-cli status
//!   fp6-usbmodes-cli set ncm,mtp        ("" = charge only, keepFunctions stay)
//!   fp6-usbmodes-cli power source|sink
//!   fp6-usbmodes-cli prefs system|charge|last|ask [remember]
//!   fp6-usbmodes-cli locked true|false  (normally the session agent)
//! FP6_USBMODES_BUS=session uses the session bus (tests).
//! Emergency without D-Bus (root): fp6-usbmoded --restore-network

use fp6_usbmodes::{BUS_NAME, IFACE, PATH};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let conn = if std::env::var("FP6_USBMODES_BUS").as_deref() == Ok("session") {
        zbus::blocking::Connection::session()
    } else {
        zbus::blocking::Connection::system()
    };
    let conn = conn.unwrap_or_else(|e| {
        eprintln!("D-Bus: {e}");
        std::process::exit(1);
    });
    let arg = |i: usize| args.get(i).cloned().unwrap_or_default();
    let call = |m: &str| -> zbus::Result<zbus::message::Message> {
        match m {
            "SetFunctions" => {
                let list: Vec<String> = arg(1).split(',').filter(|x| !x.is_empty()).map(String::from).collect();
                conn.call_method(Some(BUS_NAME), PATH, Some(IFACE), m, &(list,))
            }
            "SetPowerRole" => conn.call_method(Some(BUS_NAME), PATH, Some(IFACE), m, &(arg(1),)),
            "SetPreferences" => {
                let mode = if arg(1).is_empty() { "system".to_string() } else { arg(1) };
                conn.call_method(Some(BUS_NAME), PATH, Some(IFACE), m, &(mode, arg(2) == "remember"))
            }
            "SetSessionLocked" => conn.call_method(Some(BUS_NAME), PATH, Some(IFACE), m, &(arg(1) != "false",)),
            _ => conn.call_method(Some(BUS_NAME), PATH, Some(IFACE), m, &()),
        }
    };
    let method = match args.first().map(|s| s.as_str()) {
        Some("status") | None => "GetStatus",
        Some("set") => "SetFunctions",
        Some("power") => "SetPowerRole",
        Some("prefs") => "SetPreferences",
        Some("locked") => "SetSessionLocked",
        Some(x) => {
            eprintln!("unknown command {x}; usage: status | set LIST | power source|sink | prefs MODE [remember] | locked BOOL");
            std::process::exit(2);
        }
    };
    match call(method) {
        Ok(m) => {
            if let Ok(s) = m.body().deserialize::<String>() {
                match serde_json::from_str::<serde_json::Value>(&s) {
                    Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap_or(s)),
                    Err(_) => println!("{s}"),
                }
            }
        }
        Err(e) => {
            eprintln!("{method}: {e}");
            std::process::exit(1);
        }
    }
}
