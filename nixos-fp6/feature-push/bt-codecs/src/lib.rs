pub mod pulse;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

pub const NAME: &str = "com.fairphone6.BluetoothAudio1";
pub const PATH: &str = "/com/fairphone6/BluetoothAudio1";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Profile {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub codec: String,
    pub available: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Device {
    pub id: String,
    pub address: String,
    pub name: String,
    pub profile: String,
    pub codec: String,
    pub bitrate: Option<u32>,
    pub bitrate_source: String,
    pub profiles: Vec<Profile>,
}

// PipeWire exposes codec-specific SPA EnumProfile entries through pipewire-pulse.
// Do not infer availability from the globally installed codec libraries.
pub fn classify(id: &str) -> Option<(String, String)> {
    if id == "a2dp-sink" {
        return Some(("A2DP".into(), "unknown".into()));
    }
    if let Some(codec) = id.strip_prefix("a2dp-sink-") {
        return Some(("A2DP".into(), codec.replace('-', "_")));
    }
    if id == "headset-head-unit" {
        return Some(("HFP".into(), "unknown".into()));
    }
    if let Some(codec) = id.strip_prefix("headset-head-unit-") {
        return Some(("HFP".into(), codec.replace('-', "_")));
    }
    if id.starts_with("a2dp-auto-prefer-") {
        return Some(("A2DP".into(), "auto".into()));
    }
    if id == "bap-sink" || id == "bap-duplex" || id.starts_with("bap-sink-") {
        return Some(("LE Audio".into(), "lc3".into()));
    }
    None
}

pub trait Backend: Send {
    fn devices(&mut self) -> Result<Vec<Device>, String>;
    fn select(&mut self, device: &str, profile: &str) -> Result<(), String>;
}

pub struct Service<B> {
    backend: Mutex<B>,
    preferred: Vec<String>,
}
impl<B: Backend> Service<B> {
    pub fn new(backend: B, preferred: Vec<String>) -> Self {
        Self {
            backend: Mutex::new(backend),
            preferred,
        }
    }
    fn run<T>(&self, f: impl FnOnce(&mut B) -> Result<T, String>) -> zbus::fdo::Result<T> {
        let mut backend = self
            .backend
            .lock()
            .map_err(|_| zbus::fdo::Error::Failed("Backend unavailable".into()))?;
        f(&mut backend).map_err(zbus::fdo::Error::Failed)
    }
}

#[zbus::interface(name = "com.fairphone6.BluetoothAudio1")]
impl<B: Backend + 'static> Service<B> {
    /// JSON schema 1. A status read never selects a profile or changes policy.
    pub fn list_devices(&self) -> zbus::fdo::Result<String> {
        self.run(|b| {
            serde_json::to_string(&serde_json::json!({"schema":1,"devices":b.devices()?}))
                .map_err(|e| e.to_string())
        })
    }
    /// Select the exact current advertised profile ID, never a raw PipeWire property.
    pub fn set_profile(&self, device: &str, profile: &str) -> zbus::fdo::Result<()> {
        self.run(|b| {
            let d = b
                .devices()?
                .into_iter()
                .find(|d| d.id == device)
                .ok_or("Device is no longer connected")?;
            if !d.profiles.iter().any(|p| p.id == profile && p.available) {
                return Err("Profile is unavailable".into());
            }
            if d.profile == profile {
                return Ok(());
            }
            b.select(device, profile)
        })
    }
    /// Explicit per-device choice. WirePlumber owns restoration of the selected profile.
    /// Quality follows the configured order; stable chooses baseline SBC.
    pub fn set_preference(&self, device: &str, preference: &str) -> zbus::fdo::Result<()> {
        if preference != "quality" && preference != "stable" {
            return Err(zbus::fdo::Error::InvalidArgs(
                "Use quality or stable".into(),
            ));
        }
        self.run(|b| {
            let d = b
                .devices()?
                .into_iter()
                .find(|d| d.id == device)
                .ok_or("Device is no longer connected")?;
            if classify(&d.profile).is_some_and(|(kind, _)| kind == "HFP") {
                return Err("Choose a music profile before changing audio quality".into());
            }
            let stable = vec!["sbc".to_owned()];
            let order = if preference == "stable" {
                &stable
            } else {
                &self.preferred
            };
            let p = order
                .iter()
                .find_map(|c| {
                    d.profiles.iter().find(|p| {
                        (p.kind == "A2DP" || p.kind == "LE Audio") && p.available && p.codec == *c
                    })
                })
                .ok_or("No matching available music codec")?;
            if p.id == d.profile {
                return Ok(());
            }
            b.select(device, &p.id)
        })
    }
}
