// SPDX-License-Identifier: GPL-3.0-only
//! Session bus API, name com.fairphone6.CallRecorder, object
//! /com/fairphone6/CallRecorder:
//!
//! org.gtk.Actions (for QML: QMenuModel's QDBusActionGroup)
//!   recording       state b; Activate() toggles, SetState(b) starts/stops.
//!                   Enabled while a call is up.
//!   status          state s, JSON {"inCall","recording","armed","startedAt"
//!                   (unix ms, 0),"path","mode","auto","error","announced"}
//!   recordings      state s, JSON array, newest first (library::ui_row)
//!   playing         state s, JSON {} or {"id","which","startedAt"}
//!   delete          parameter s: recording id
//!   play            parameter s: "<id>", "<id>|uplink", "<id>|downlink"
//!   stop-playback, refresh
//!
//! com.fairphone6.CallRecorder1 (scripts, the CLI)
//!   Toggle(), Start(), Stop()   errors as D-Bus errors
//!   Status() -> s               the status JSON
//!   List() -> s                 the recordings JSON

use crate::policy::Command;
use std::collections::HashMap;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use zbus::interface;
use zbus::zvariant::{OwnedValue, Signature, Value};

pub const BUS_NAME: &str = "com.fairphone6.CallRecorder";
pub const PATH: &str = "/com/fairphone6/CallRecorder";
pub const IFACE: &str = "com.fairphone6.CallRecorder1";

pub enum Req {
    Cmd(Command, Option<Sender<Result<(), String>>>),
    Delete(String),
    Play(String),
    StopPlayback,
    Refresh,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ui {
    pub recording: bool,
    pub in_call: bool,
    pub status: String,
    pub recordings: String,
    pub playing: String,
}

pub struct Shared {
    pub ui: Mutex<Ui>,
    pub tx: Mutex<Sender<Req>>,
}

type ActionDescription = (bool, Signature, Vec<OwnedValue>);

fn owned<'a, T: Into<Value<'a>>>(v: T) -> OwnedValue {
    OwnedValue::try_from(v.into()).expect("plain values have no fds")
}

const ACTIONS: &[&str] = &["recording", "status", "recordings", "playing", "delete", "play", "stop-playback", "refresh"];

fn describe(ui: &Ui, name: &str) -> Option<ActionDescription> {
    let s = |x: &str| vec![owned(x.to_string())];
    Some(match name {
        "recording" => (ui.in_call, Signature::Unit, vec![owned(ui.recording)]),
        "status" => (true, Signature::Unit, s(&ui.status)),
        "recordings" => (true, Signature::Unit, s(&ui.recordings)),
        "playing" => (true, Signature::Unit, s(&ui.playing)),
        "delete" | "play" => (true, Signature::Str, Vec::new()),
        "stop-playback" | "refresh" => (true, Signature::Unit, Vec::new()),
        _ => return None,
    })
}

struct Actions {
    shared: Arc<Shared>,
}

impl Actions {
    fn send(&self, r: Req) {
        let _ = self.shared.tx.lock().unwrap().send(r);
    }
}

fn first_str(p: &[OwnedValue]) -> Option<String> {
    p.first().and_then(|v| String::try_from(v.try_clone().ok()?).ok())
}

#[interface(name = "org.gtk.Actions")]
impl Actions {
    fn list(&self) -> Vec<String> {
        ACTIONS.iter().map(|s| s.to_string()).collect()
    }

    fn describe(&self, name: &str) -> zbus::fdo::Result<ActionDescription> {
        describe(&self.shared.ui.lock().unwrap(), name).ok_or_else(|| zbus::fdo::Error::InvalidArgs(format!("no action {name}")))
    }

    fn describe_all(&self) -> HashMap<String, ActionDescription> {
        let ui = self.shared.ui.lock().unwrap();
        ACTIONS.iter().filter_map(|n| describe(&ui, n).map(|d| (n.to_string(), d))).collect()
    }

    fn activate(&self, name: &str, parameter: Vec<OwnedValue>, _platform_data: HashMap<String, OwnedValue>) {
        match name {
            "recording" => self.send(Req::Cmd(Command::Toggle, None)),
            "delete" => {
                if let Some(id) = first_str(&parameter) {
                    self.send(Req::Delete(id))
                }
            }
            "play" => {
                if let Some(spec) = first_str(&parameter) {
                    self.send(Req::Play(spec))
                }
            }
            "stop-playback" => self.send(Req::StopPlayback),
            "refresh" => self.send(Req::Refresh),
            _ => {}
        }
    }

    fn set_state(&self, name: &str, value: OwnedValue, _platform_data: HashMap<String, OwnedValue>) {
        if name == "recording" {
            if let Ok(b) = bool::try_from(&value) {
                self.send(Req::Cmd(if b { Command::Start } else { Command::Stop }, None));
            }
        }
    }
}

struct Recorder {
    shared: Arc<Shared>,
}

impl Recorder {
    fn cmd(&self, c: Command) -> zbus::fdo::Result<()> {
        let (tx, rx) = channel();
        self.shared.tx.lock().unwrap().send(Req::Cmd(c, Some(tx))).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(zbus::fdo::Error::Failed(e)),
            Err(_) => Err(zbus::fdo::Error::Failed("no answer from the recorder".into())),
        }
    }
}

#[interface(name = "com.fairphone6.CallRecorder1")]
impl Recorder {
    fn toggle(&self) -> zbus::fdo::Result<()> {
        self.cmd(Command::Toggle)
    }

    fn start(&self) -> zbus::fdo::Result<()> {
        self.cmd(Command::Start)
    }

    fn stop(&self) -> zbus::fdo::Result<()> {
        self.cmd(Command::Stop)
    }

    fn status(&self) -> String {
        self.shared.ui.lock().unwrap().status.clone()
    }

    fn list(&self) -> String {
        self.shared.ui.lock().unwrap().recordings.clone()
    }
}

pub struct Service {
    pub conn: zbus::blocking::Connection,
    pub shared: Arc<Shared>,
}

impl Service {
    pub fn serve(ui: Ui, tx: Sender<Req>) -> zbus::Result<Service> {
        let shared = Arc::new(Shared { ui: Mutex::new(ui), tx: Mutex::new(tx) });
        let conn = zbus::blocking::connection::Builder::session()?
            .serve_at(PATH, Actions { shared: shared.clone() })?
            .serve_at(PATH, Recorder { shared: shared.clone() })?
            .name(BUS_NAME)?
            .build()?;
        Ok(Service { conn, shared })
    }

    /// Replace the UI state; emits org.gtk.Actions.Changed for what differs.
    pub fn update(&self, new: Ui) {
        let old = {
            let mut ui = self.shared.ui.lock().unwrap();
            std::mem::replace(&mut *ui, new.clone())
        };
        let mut states: HashMap<String, OwnedValue> = HashMap::new();
        let mut enabled: HashMap<String, bool> = HashMap::new();
        if old.recording != new.recording {
            states.insert("recording".into(), owned(new.recording));
        }
        if old.in_call != new.in_call {
            enabled.insert("recording".into(), new.in_call);
        }
        for (name, o, n) in [("status", &old.status, &new.status), ("recordings", &old.recordings, &new.recordings), ("playing", &old.playing, &new.playing)] {
            if o != n {
                states.insert(name.into(), owned(n.clone()));
            }
        }
        if states.is_empty() && enabled.is_empty() {
            return;
        }
        let body = (Vec::<String>::new(), enabled, states, HashMap::<String, ActionDescription>::new());
        if let Err(e) = self.conn.emit_signal(None::<()>, PATH, "org.gtk.Actions", "Changed", &body) {
            eprintln!("Actions.Changed: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptions() {
        let ui = Ui { recording: true, in_call: true, status: "{}".into(), recordings: "[]".into(), playing: "{}".into() };
        let (en, sig, st) = describe(&ui, "recording").unwrap();
        assert!(en);
        assert_eq!(sig, Signature::Unit);
        assert!(bool::try_from(&st[0]).unwrap());
        assert_eq!(describe(&ui, "play").unwrap().1, Signature::Str);
        assert!(describe(&ui, "nope").is_none());
        for a in ACTIONS {
            assert!(describe(&ui, a).is_some(), "{a}");
        }
    }
}
