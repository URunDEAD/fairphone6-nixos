//! Phone status and settings switches (the indicators' org.gtk.Actions,
//! exactly what the quick-settings tiles use), location (GeoClue2; the
//! assistant's desktop id is allow-listed by the module), clock.

use super::{arg_str, schema, spec, with_enum, Confirm, Ctx, Output, Tool, ToolSpec};
use crate::dbusutil::{call, gaction_activate, gaction_set_state, gaction_state};
use crate::permissions::Resource;
use crate::settings::Settings;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use zbus::zvariant::Value as ZV;

pub fn tools() -> Vec<Arc<dyn Tool>> {
    vec![Arc::new(Status), Arc::new(SettingsGet), Arc::new(SettingsSet), Arc::new(Location)]
}

/// (name, bus, path, action, activate takes the new state as parameter)
pub const SWITCHES: &[(&str, &str, &str, &str, bool)] = &[
    ("wifi", "com.lomiri.indicator.network", "/com/lomiri/indicator/network", "wifi.enable", false),
    ("mobile_data", "com.lomiri.indicator.network", "/com/lomiri/indicator/network", "mobiledata.enabled", false),
    ("airplane", "com.lomiri.indicator.network", "/com/lomiri/indicator/network", "airplane.enabled", false),
    ("hotspot", "com.lomiri.indicator.network", "/com/lomiri/indicator/network", "hotspot.enable", false),
    ("bluetooth", "org.ayatana.indicator.bluetooth", "/org/ayatana/indicator/bluetooth", "bluetooth-enabled", true),
    ("flashlight", "org.ayatana.indicator.power", "/org/ayatana/indicator/power", "flashlight", true),
    ("dnd", "com.lomiri.indicator.dnd", "/com/lomiri/indicator/dnd", "dnd", false),
    ("silent", "org.ayatana.indicator.sound", "/org/ayatana/indicator/sound", "silent-mode", true),
    ("rotation_lock", "org.ayatana.indicator.display", "/org/ayatana/indicator/display", "rotation-lock", true),
    ("location", "com.lomiri.indicator.location", "/com/lomiri/indicator/location", "location-detection-enabled", false),
    ("battery_saver", "com.fairphone6.indicator.battery", "/com/fairphone6/indicator/battery", "saver", false),
];
const VOLUME: (&str, &str, &str) = ("org.ayatana.indicator.sound", "/org/ayatana/indicator/sound", "volume");
const NAMES: &[&str] = &["wifi", "mobile_data", "airplane", "hotspot", "bluetooth", "flashlight", "dnd", "silent", "rotation_lock", "location", "battery_saver", "volume"];

fn label(name: &str) -> &'static str {
    match name {
        "wifi" => "Wi-Fi",
        "mobile_data" => "Mobile data",
        "airplane" => "Flight mode",
        "hotspot" => "Hotspot",
        "bluetooth" => "Bluetooth",
        "flashlight" => "Flashlight",
        "dnd" => "Do Not Disturb",
        "silent" => "Silent mode",
        "rotation_lock" => "Rotation lock",
        "location" => "Location",
        "battery_saver" => "Battery saver",
        "volume" => "Volume",
        _ => "Setting",
    }
}

fn battery() -> Value {
    let base = std::path::Path::new("/sys/class/power_supply");
    for e in std::fs::read_dir(base).into_iter().flatten().flatten() {
        let p = e.path();
        if std::fs::read_to_string(p.join("type")).map(|t| t.trim() == "Battery").unwrap_or(false) {
            let read = |f: &str| std::fs::read_to_string(p.join(f)).map(|s| s.trim().to_string()).unwrap_or_default();
            return json!({"percent": read("capacity").parse::<u32>().ok(), "status": read("status")});
        }
    }
    json!(null)
}

struct Status;
#[async_trait]
impl Tool for Status {
    fn spec(&self) -> ToolSpec {
        spec("phone_status", "Phone", "Phone status", "Current time, battery level and charging state.", schema(&[]), false, false)
    }
    fn resources(&self, _a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        Ok(vec![])
    }
    async fn run(&self, _a: Value, _ctx: &Ctx) -> Result<Output, String> {
        Ok(Output::json(&json!({"time": crate::agent::chrono_like_now(), "battery": battery()})))
    }
}

struct SettingsGet;
#[async_trait]
impl Tool for SettingsGet {
    fn spec(&self) -> ToolSpec {
        spec("settings_get", "Settings", "Read phone settings", "Read the switches: Wi-Fi, mobile data, flight mode, hotspot, Bluetooth, flashlight, Do Not Disturb, silent, rotation lock, location, battery saver, volume.", schema(&[]), false, false)
    }
    fn resources(&self, _a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        Ok(vec![Resource::new("settings")])
    }
    async fn run(&self, _a: Value, ctx: &Ctx) -> Result<Output, String> {
        let conn = ctx.session()?;
        let mut out = serde_json::Map::new();
        for (name, bus, path, action, _) in SWITCHES {
            let v = gaction_state(conn, bus, path, action).await.map(|s| s["state"].clone()).unwrap_or(json!("unavailable"));
            out.insert(name.to_string(), v);
        }
        let vol = gaction_state(conn, VOLUME.0, VOLUME.1, VOLUME.2).await.map(|s| s["state"].clone()).unwrap_or(json!("unavailable"));
        out.insert("volume".into(), vol);
        Ok(Output::json(&Value::Object(out)))
    }
}

struct SettingsSet;
#[async_trait]
impl Tool for SettingsSet {
    fn spec(&self) -> ToolSpec {
        spec(
            "settings_set",
            "Settings",
            "Change a phone setting",
            "Switch a setting on/off (value true/false), or set volume (value 0-100).",
            {
                let mut s = with_enum(schema(&[("setting", "string", "Which setting", true), ("value", "string", "true/false, or 0-100 for volume", true)]), "setting", NAMES);
                // Models send booleans and numbers as often as strings.
                s["properties"]["value"]["type"] = json!(["boolean", "number", "string"]);
                s
            },
            true,
            false,
        )
    }
    fn resources(&self, a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        parse_value(a)?;
        Ok(vec![Resource::new("settings")])
    }
    fn confirm(&self, a: &Value) -> Option<Confirm> {
        let (name, v) = parse_value(a).ok()?;
        let shown = match v {
            SetValue::Bool(b) => if b { "On" } else { "Off" }.to_string(),
            SetValue::Level(l) => format!("{l} %"),
        };
        Some(Confirm { title: format!("Change {}?", label(&name)), fields: vec![(label(&name).into(), shown)], action: "Change".into(), destructive: false })
    }
    async fn run(&self, a: Value, ctx: &Ctx) -> Result<Output, String> {
        let conn = ctx.session()?;
        let (name, v) = parse_value(&a)?;
        match v {
            SetValue::Level(l) => {
                gaction_set_state(conn, VOLUME.0, VOLUME.1, VOLUME.2, ZV::from(l as f64 / 100.0)).await?;
                Ok(Output::text(format!("Volume set to {l} %.")))
            }
            SetValue::Bool(want) => {
                let (_, bus, path, action, with_param) = SWITCHES.iter().find(|s| s.0 == name).ok_or("unknown setting")?;
                let cur = gaction_state(conn, bus, path, action).await?;
                if cur["enabled"] == false {
                    return Err(format!("{} cannot be changed right now", label(&name)));
                }
                if cur["state"].as_bool() == Some(want) {
                    return Ok(Output::text(format!("{} is already {}.", label(&name), if want { "on" } else { "off" })));
                }
                if *with_param {
                    gaction_activate(conn, bus, path, action, Some(ZV::from(want))).await?;
                } else {
                    gaction_activate(conn, bus, path, action, None).await?;
                }
                Ok(Output::text(format!("{} turned {}.", label(&name), if want { "on" } else { "off" })))
            }
        }
    }
}

#[derive(Debug, PartialEq)]
enum SetValue {
    Bool(bool),
    Level(u32),
}

fn parse_value(a: &Value) -> Result<(String, SetValue), String> {
    let name = arg_str(a, "setting")?.to_string();
    if !NAMES.contains(&name.as_str()) {
        return Err(format!("unknown setting {name}"));
    }
    let raw = match a.get("value") {
        Some(Value::String(s)) => s.to_lowercase(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => n.to_string(),
        _ => return Err("missing value".into()),
    };
    if name == "volume" {
        let l: f64 = raw.trim_end_matches('%').trim().parse().map_err(|_| "volume must be 0-100".to_string())?;
        if !(0.0..=100.0).contains(&l) {
            return Err("volume must be 0-100".into());
        }
        return Ok((name, SetValue::Level(l.round() as u32)));
    }
    let b = match raw.as_str() {
        "true" | "on" | "1" | "yes" => true,
        "false" | "off" | "0" | "no" => false,
        _ => return Err("value must be true or false".into()),
    };
    Ok((name, SetValue::Bool(b)))
}

struct Location;
#[async_trait]
impl Tool for Location {
    fn spec(&self) -> ToolSpec {
        spec("location_get", "Location", "Where am I", "The phone's current position (latitude, longitude, accuracy) from GPS/network; needs Location switched on.", schema(&[]), false, false)
    }
    fn resources(&self, _a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        Ok(vec![Resource::new("location")])
    }
    async fn run(&self, _a: Value, ctx: &Ctx) -> Result<Output, String> {
        use futures_util::StreamExt;
        let sys = ctx.system()?;
        const GC: &str = "org.freedesktop.GeoClue2";
        let r = call(sys, GC, "/org/freedesktop/GeoClue2/Manager", "org.freedesktop.GeoClue2.Manager", "GetClient", &()).await?;
        let client: zbus::zvariant::OwnedObjectPath = r.body().deserialize().map_err(|e| e.to_string())?;
        let props = zbus::fdo::PropertiesProxy::builder(sys).destination(GC).map_err(|e| e.to_string())?.path(client.clone()).map_err(|e| e.to_string())?.build().await.map_err(|e| e.to_string())?;
        let iface = zbus::names::InterfaceName::from_static_str_unchecked("org.freedesktop.GeoClue2.Client");
        props.set(iface.clone(), "DesktopId", ZV::from("com.fairphone6.Assistant")).await.map_err(|e| e.to_string())?;
        props.set(iface.clone(), "RequestedAccuracyLevel", ZV::from(8u32)).await.map_err(|e| e.to_string())?;
        let cproxy = zbus::Proxy::new(sys, GC, client.clone(), "org.freedesktop.GeoClue2.Client").await.map_err(|e| e.to_string())?;
        let mut updates = cproxy.receive_signal("LocationUpdated").await.map_err(|e| e.to_string())?;
        cproxy.call_method("Start", &()).await.map_err(|e| format!("location: {e} (is Location switched on?)"))?;
        let msg = tokio::time::timeout(std::time::Duration::from_secs(45), updates.next()).await.map_err(|_| "no position fix within 45 s".to_string())?.ok_or("no position")?;
        let (_old, new): (zbus::zvariant::OwnedObjectPath, zbus::zvariant::OwnedObjectPath) = msg.body().deserialize().map_err(|e| e.to_string())?;
        let lp = zbus::Proxy::new(sys, GC, new, "org.freedesktop.GeoClue2.Location").await.map_err(|e| e.to_string())?;
        let lat: f64 = lp.get_property("Latitude").await.map_err(|e| e.to_string())?;
        let lon: f64 = lp.get_property("Longitude").await.map_err(|e| e.to_string())?;
        let acc: f64 = lp.get_property("Accuracy").await.unwrap_or(0.0);
        let _ = cproxy.call_method("Stop", &()).await;
        Ok(Output::json(&json!({"latitude": lat, "longitude": lon, "accuracy_m": acc.round(), "map": format!("geo:{lat:.6},{lon:.6}")})))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn values() {
        assert_eq!(parse_value(&json!({"setting": "wifi", "value": "off"})).unwrap().1, SetValue::Bool(false));
        assert_eq!(parse_value(&json!({"setting": "volume", "value": "40%"})).unwrap().1, SetValue::Level(40));
        assert!(parse_value(&json!({"setting": "volume", "value": 140})).is_err());
        assert!(parse_value(&json!({"setting": "root", "value": true})).is_err());
        let c = SettingsSet.confirm(&json!({"setting": "flashlight", "value": true})).unwrap();
        assert_eq!(c.fields[0], ("Flashlight".to_string(), "On".to_string()));
    }
}
