//! libpulse is the supported client API of PipeWire's pulse server. No subprocesses.
use crate::{classify, Backend, Device, Profile};
use libpulse_binding as pa;
use pa::{
    callbacks::ListResult,
    context::{Context, FlagSet, State},
    mainloop::standard::{IterateResult, Mainloop},
};
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

pub struct Pulse;
struct Session {
    context: Context,
    mainloop: Mainloop,
    deadline: Instant,
}
impl Drop for Session {
    fn drop(&mut self) {
        self.context.disconnect();
    }
}
impl Session {
    fn tick(&mut self) -> Result<(), String> {
        if Instant::now() > self.deadline {
            return Err("Audio server timed out; refresh status before retrying".into());
        }
        if matches!(
            self.mainloop.iterate(false),
            IterateResult::Err(_) | IterateResult::Quit(_)
        ) || matches!(self.context.get_state(), State::Failed | State::Terminated)
        {
            return Err("Audio server disconnected".into());
        }
        std::thread::sleep(Duration::from_millis(2));
        Ok(())
    }
    fn wait<T: ?Sized>(&mut self, op: &pa::operation::Operation<T>) -> Result<(), String> {
        while op.get_state() == pa::operation::State::Running {
            self.tick()?;
        }
        if op.get_state() == pa::operation::State::Cancelled {
            return Err("Audio request cancelled".into());
        }
        Ok(())
    }
    fn open() -> Result<Self, String> {
        let mainloop = Mainloop::new().ok_or("Cannot allocate audio mainloop")?;
        let mut context = Context::new(&mainloop, "FP6 Bluetooth audio")
            .ok_or("Cannot allocate audio context")?;
        // Explicit local endpoint: never autospawn PulseAudio or use a network server.
        let runtime = std::env::var("XDG_RUNTIME_DIR").map_err(|_| "No user runtime directory")?;
        context
            .connect(
                Some(&format!("unix:{runtime}/pulse/native")),
                FlagSet::NOAUTOSPAWN,
                None,
            )
            .map_err(|_| "PipeWire Pulse server unavailable")?;
        let mut s = Self {
            context,
            mainloop,
            deadline: Instant::now() + Duration::from_secs(5),
        };
        while s.context.get_state() != State::Ready {
            s.tick()?;
        }
        let server = Rc::new(RefCell::new(String::new()));
        let copy = server.clone();
        let op = s.context.introspect().get_server_info(move |i| {
            *copy.borrow_mut() = i.server_name.as_deref().unwrap_or("").into()
        });
        s.wait(&op)?;
        if !server.borrow().contains("PipeWire") {
            return Err("This helper requires PipeWire Pulse".into());
        }
        Ok(s)
    }
}

// The base profile name denotes the highest priority codec, NOT necessarily SBC.
// Codec descriptions are stable upstream literals even when surrounding prose is translated.
pub fn profile(id: &str, label: &str, available: bool) -> Option<Profile> {
    let (kind, mut codec) = classify(id)?;
    if id == "a2dp-sink" || id == "headset-head-unit" {
        codec = "unknown".into();
        for (suffix, name) in [
            ("SBC", "sbc"),
            ("SBC-XQ", "sbc_xq"),
            ("AAC", "aac"),
            ("AAC-ELD", "aac_eld"),
            ("aptX", "aptx"),
            ("aptX HD", "aptx_hd"),
            ("LDAC", "ldac"),
            ("CVSD", "cvsd"),
            ("MSBC", "msbc"),
            ("LC3-SWB", "lc3_swb"),
        ] {
            if label.ends_with(&format!(" {suffix})")) {
                codec = name.into();
                break;
            }
        }
    }
    Some(Profile {
        id: id.into(),
        label: label.into(),
        kind,
        codec,
        available,
    })
}
impl Backend for Pulse {
    fn devices(&mut self) -> Result<Vec<Device>, String> {
        let mut s = Session::open()?;
        let cards = Rc::new(RefCell::new(Vec::new()));
        let result = cards.clone();
        let failed = Rc::new(RefCell::new(false));
        let err = failed.clone();
        let op = s.context.introspect().get_card_info_list(move |r| match r {
            ListResult::Item(c) => {
                if c.proplist.get_str("device.api").as_deref() != Some("bluez5") {
                    return;
                }
                let profiles: Vec<_> = c
                    .profiles
                    .iter()
                    .filter_map(|p| {
                        profile(
                            p.name.as_deref()?,
                            p.description.as_deref().unwrap_or(""),
                            p.available,
                        )
                    })
                    .collect();
                if profiles.is_empty() {
                    return;
                }
                let active = c
                    .active_profile
                    .as_ref()
                    .and_then(|p| p.name.as_deref())
                    .unwrap_or("off");
                let codec = profiles
                    .iter()
                    .find(|p| p.id == active)
                    .map(|p| p.codec.clone())
                    .unwrap_or_else(|| "unknown".into());
                result.borrow_mut().push((
                    c.index,
                    Device {
                        id: c.name.as_deref().unwrap_or("").into(),
                        address: c
                            .proplist
                            .get_str("api.bluez5.address")
                            .or_else(|| c.proplist.get_str("device.string"))
                            .unwrap_or_default(),
                        name: c
                            .proplist
                            .get_str("device.description")
                            .unwrap_or_else(|| "Bluetooth audio".into()),
                        profile: active.into(),
                        codec,
                        bitrate: None,
                        bitrate_source: "not reported".into(),
                        profiles,
                    },
                ));
            }
            ListResult::Error => *err.borrow_mut() = true,
            ListResult::End => (),
        });
        s.wait(&op)?;
        if *failed.borrow() {
            return Err("Cannot enumerate audio devices".into());
        }
        let result = cards.clone();
        let err = failed.clone();
        let op = s.context.introspect().get_sink_info_list(move |r| match r {
            ListResult::Item(n) => {
                if let Some((_, d)) = result
                    .borrow_mut()
                    .iter_mut()
                    .find(|(i, _)| Some(*i) == n.card)
                {
                    if let Some(c) = n.proplist.get_str("api.bluez5.codec") {
                        d.codec = c;
                        if matches!(d.codec.as_str(), "msbc" | "lc3_swb" | "cvsd") {
                            d.bitrate = Some(64000);
                            d.bitrate_source = "nominal SCO transport rate".into();
                        }
                    }
                    // No PCM rate × sample size calculation: that is not transport bitrate.
                }
            }
            ListResult::Error => *err.borrow_mut() = true,
            ListResult::End => (),
        });
        s.wait(&op)?;
        if *failed.borrow() {
            return Err("Cannot enumerate audio transports".into());
        }
        // A card may survive a disconnect briefly; BlueZ is the authority for connection state.
        let bus = zbus::blocking::connection::Builder::system()
            .map_err(|_| "BlueZ bus unavailable")?
            .method_timeout(Duration::from_secs(3))
            .build()
            .map_err(|_| "BlueZ bus unavailable")?;
        let connected = connected_addresses(&bus)?;
        let devices = cards
            .borrow()
            .iter()
            .filter(|(_, d)| connected.contains(&d.address.to_uppercase()))
            .map(|(_, d)| d.clone())
            .collect();
        Ok(devices)
    }
    fn select(&mut self, device: &str, profile: &str) -> Result<(), String> {
        let mut s = Session::open()?;
        let success = Rc::new(RefCell::new(false));
        let copy = success.clone();
        let op = s.context.introspect().set_card_profile_by_name(
            device,
            profile,
            Some(Box::new(move |ok| *copy.borrow_mut() = ok)),
        );
        s.wait(&op)?;
        if !*success.borrow() {
            return Err("PipeWire rejected the profile; device may have disconnected".into());
        }
        Ok(())
    }
}

/// Read only: never activate bluetoothd as a side effect of opening the page.
pub fn connected_addresses(
    bus: &zbus::blocking::Connection,
) -> Result<std::collections::HashSet<String>, String> {
    let manager = zbus::blocking::fdo::ObjectManagerProxy::builder(bus)
        .destination("org.bluez")
        .map_err(|e| e.to_string())?
        .path("/")
        .map_err(|e| e.to_string())?
        .build()
        .map_err(|e| e.to_string())?;
    let objects: zbus::fdo::ManagedObjects = manager
        .inner()
        .call_with_flags(
            "GetManagedObjects",
            zbus::proxy::MethodFlags::NoAutoStart.into(),
            &(),
        )
        .map_err(|_| "Cannot read BlueZ devices")?
        .ok_or("Missing BlueZ response")?;
    let mut connected = std::collections::HashSet::new();
    for interfaces in objects.values() {
        if let Some(props) = interfaces.get("org.bluez.Device1") {
            let up = props
                .get("Connected")
                .and_then(|v| bool::try_from(v).ok())
                .unwrap_or(false);
            if up {
                if let Some(address) = props.get("Address").and_then(|v| <&str>::try_from(v).ok()) {
                    connected.insert(address.to_uppercase());
                }
            }
        }
    }
    Ok(connected)
}
