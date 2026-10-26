// SPDX-License-Identifier: GPL-3.0-only
//! fp6-screenrecord-indicator: "Record screen" switch in Lomiri's status-bar
//! menus (an Ayatana indicator: org.gtk.Menus + org.gtk.Actions).
//!
//! It holds no capture rights of its own. The switch only asks the shell
//! (com.fp6.ScreenRecorder, exported by Lomiri's ScreenRecorder item) to show
//! its "Record screen" confirmation; the recording starts after the user
//! taps Start there. Switching it off stops a running recording. The state
//! follows the shell's RecordingChanged signal.
//!
//! The indicator is hidden from the collapsed status bar (the shell draws its
//! own red dot while recording) and shows up in the expanded menus.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use zbus::message::Type as MessageType;
use zbus::zvariant::{OwnedValue, Signature, Value};
use zbus::{interface, MatchRule};

const BUS_NAME: &str = "com.lomiri.indicator.screenrecord";
const ACTIONS_PATH: &str = "/com/lomiri/indicator/screenrecord";
const MENU_PHONE: &str = "/com/lomiri/indicator/screenrecord/phone";
const MENU_DESKTOP: &str = "/com/lomiri/indicator/screenrecord/desktop";
const SHELL_NAME: &str = "com.fp6.ScreenRecorder";
const SHELL_PATH: &str = "/com/fp6/ScreenRecorder";
const SHELL_IFACE: &str = "com.fp6.ScreenRecorder";

type Item = HashMap<String, OwnedValue>;
type ActionDescription = (bool, Signature, Vec<OwnedValue>);

fn owned<'a, T: Into<Value<'a>>>(v: T) -> OwnedValue {
    OwnedValue::try_from(v.into()).expect("plain values have no fds")
}

struct State {
    recording: AtomicBool,
    conn: OnceLock<zbus::Connection>,
}

impl State {
    fn root_state(&self) -> OwnedValue {
        let recording = self.recording.load(Ordering::SeqCst);
        let icon: Value = Value::new((
            "themed",
            Value::new(vec!["media-record", "media-record-symbolic", "camera-video"]),
        ));
        let mut state: HashMap<&str, Value> = HashMap::new();
        state.insert("title", Value::new("Screen recording"));
        state.insert("icon", icon);
        state.insert(
            "accessible-desc",
            Value::new(if recording { "Screen recording in progress" } else { "Screen recording off" }),
        );
        // Collapsed status bar: never (the shell has its own red dot);
        // the expanded menus list hidden indicators anyway.
        state.insert("visible", Value::new(false));
        owned(state)
    }

    fn describe(&self, name: &str) -> Option<ActionDescription> {
        let unit = Signature::Unit;
        match name {
            "root" => Some((true, unit, vec![self.root_state()])),
            "recording" => Some((true, unit, vec![owned(self.recording.load(Ordering::SeqCst))])),
            _ => None,
        }
    }

    async fn request(&self, on: bool) {
        let Some(conn) = self.conn.get() else { return };
        let method = if on { "RequestStart" } else { "Stop" };
        if let Err(e) = conn
            .call_method(Some(SHELL_NAME), SHELL_PATH, Some(SHELL_IFACE), method, &())
            .await
        {
            eprintln!("fp6-screenrecord-indicator: {method}: {e}");
        }
    }
}

struct Actions(Arc<State>);

#[interface(name = "org.gtk.Actions")]
impl Actions {
    fn list(&self) -> Vec<String> {
        vec!["root".into(), "recording".into()]
    }

    fn describe(&self, name: &str) -> zbus::fdo::Result<ActionDescription> {
        self.0
            .describe(name)
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs(format!("no action {name}")))
    }

    fn describe_all(&self) -> HashMap<String, ActionDescription> {
        ["root", "recording"]
            .iter()
            .filter_map(|n| self.0.describe(n).map(|d| (n.to_string(), d)))
            .collect()
    }

    async fn activate(&self, name: &str, _parameter: Vec<OwnedValue>, _platform_data: HashMap<String, OwnedValue>) {
        if name == "recording" {
            let on = !self.0.recording.load(Ordering::SeqCst);
            self.0.request(on).await;
        }
    }

    async fn set_state(&self, name: &str, value: OwnedValue, _platform_data: HashMap<String, OwnedValue>) {
        if name == "recording" {
            if let Ok(on) = bool::try_from(&value) {
                if on != self.0.recording.load(Ordering::SeqCst) {
                    self.0.request(on).await;
                }
            }
        }
    }
}

struct Menus;

#[interface(name = "org.gtk.Menus")]
impl Menus {
    fn start(&self, groups: Vec<u32>) -> Vec<(u32, u32, Vec<Item>)> {
        if !groups.contains(&0) {
            return Vec::new();
        }
        let mut root: Item = HashMap::new();
        root.insert("action".into(), owned("indicator.root"));
        root.insert("x-ayatana-type".into(), owned("org.ayatana.indicator.root"));
        root.insert(":submenu".into(), owned((0u32, 1u32)));
        let mut switch: Item = HashMap::new();
        switch.insert("label".into(), owned("Record screen"));
        switch.insert("action".into(), owned("indicator.recording"));
        switch.insert("x-ayatana-type".into(), owned("org.ayatana.indicator.switch"));
        vec![(0, 0, vec![root]), (0, 1, vec![switch])]
    }

    fn end(&self, _groups: Vec<u32>) {}
}

fn emit_state(conn: &zbus::blocking::Connection, state: &State) {
    let mut changes: HashMap<String, OwnedValue> = HashMap::new();
    changes.insert("recording".into(), owned(state.recording.load(Ordering::SeqCst)));
    changes.insert("root".into(), state.root_state());
    let body = (
        Vec::<String>::new(),
        HashMap::<String, bool>::new(),
        changes,
        HashMap::<String, ActionDescription>::new(),
    );
    if let Err(e) = conn.emit_signal(None::<()>, ACTIONS_PATH, "org.gtk.Actions", "Changed", &body) {
        eprintln!("fp6-screenrecord-indicator: Changed: {e}");
    }
}

fn set_recording(conn: &zbus::blocking::Connection, state: &State, on: bool) {
    if state.recording.swap(on, Ordering::SeqCst) != on {
        emit_state(conn, state);
    }
}

fn query_shell(conn: &zbus::blocking::Connection) -> bool {
    conn.call_method(Some(SHELL_NAME), SHELL_PATH, Some(SHELL_IFACE), "IsRecording", &())
        .ok()
        .and_then(|reply| reply.body().deserialize::<bool>().ok())
        .unwrap_or(false)
}

fn main() -> zbus::Result<()> {
    let state = Arc::new(State { recording: AtomicBool::new(false), conn: OnceLock::new() });
    let conn = zbus::blocking::connection::Builder::session()?
        .serve_at(ACTIONS_PATH, Actions(state.clone()))?
        .serve_at(MENU_PHONE, Menus)?
        .serve_at(MENU_DESKTOP, Menus)?
        .name(BUS_NAME)?
        .build()?;
    let _ = state.conn.set(conn.inner().clone());

    let signals = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .interface(SHELL_IFACE)?
        .member("RecordingChanged")?
        .path(SHELL_PATH)?
        .build();
    let owners = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .sender("org.freedesktop.DBus")?
        .interface("org.freedesktop.DBus")?
        .member("NameOwnerChanged")?
        .arg(0, SHELL_NAME)?
        .build();
    let mut shell_signals = zbus::blocking::MessageIterator::for_match_rule(signals, &conn, Some(16))?;
    let owner_iter = zbus::blocking::MessageIterator::for_match_rule(owners, &conn, Some(16))?;
    {
        let conn = conn.clone();
        let state = state.clone();
        std::thread::spawn(move || {
            for msg in owner_iter.flatten() {
                if let Ok((_, _, new_owner)) = msg.body().deserialize::<(String, String, String)>() {
                    let on = !new_owner.is_empty() && query_shell(&conn);
                    set_recording(&conn, &state, on);
                }
            }
        });
    }
    set_recording(&conn, &state, query_shell(&conn));
    eprintln!("fp6-screenrecord-indicator: ready");
    for msg in shell_signals.by_ref().flatten() {
        if let Ok(on) = msg.body().deserialize::<bool>() {
            set_recording(&conn, &state, on);
        }
    }
    Ok(())
}
