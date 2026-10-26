// SPDX-License-Identifier: GPL-3.0-only
//! fp6-privacy-clipboard (user service): clipboard auto-clear and paste
//! notifications for Lomiri, over Wayland ext-data-control-v1.
//!
//! qtmir offers ext_data_control_manager_v1 to this binary only (its real
//! path and its systemd unit are checked; qtmir-data-control-clipboard-
//! helper.patch). Apps get the clipboard only through wl_data_device when
//! they paste (focused), so a paste is the only way an app reads it.
//!
//!  * clipboard-clear-minutes > 0: after that long without a new copy the
//!    selection is cleared (set_selection(NULL)).
//!  * clipboard-read-notify: text selections (all offered types text) are
//!    copied into this helper, which then owns the selection; every paste
//!    reaches it as a `send` with the pasting app's pipe, whose other end
//!    names the app (/proc/*/fd). A notification says who pasted (not for
//!    pastes into the app that copied). Pictures/files are left alone.
//! The Lomiri shell's own text fields use content-hub, not this clipboard.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fp6_privacy::{log, procinfo};
use gio::prelude::*;
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{event_created_child, Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1::{self, ExtDataControlDeviceV1},
    ext_data_control_manager_v1::ExtDataControlManagerV1,
    ext_data_control_offer_v1::{self, ExtDataControlOfferV1},
    ext_data_control_source_v1::{self, ExtDataControlSourceV1},
};

const SCHEMA: &str = "com.fairphone6.privacy";
const MAX_BYTES: usize = 1 << 20;

struct Cfg {
    clear_minutes: AtomicU32,
    notify: AtomicBool,
}

#[derive(Default)]
struct Owned {
    source: Option<ExtDataControlSourceV1>,
    data: Vec<(String, Arc<Vec<u8>>)>,
    owner: String,
}

struct State {
    cfg: Arc<Cfg>,
    seat: Option<wl_seat::WlSeat>,
    manager: Option<ExtDataControlManagerV1>,
    device: Option<ExtDataControlDeviceV1>,
    offers: HashMap<wayland_client::backend::ObjectId, Vec<String>>,
    pending: Option<ExtDataControlOfferV1>,
    selection_at: Option<Instant>,
    owned: Owned,
    notified: HashMap<String, Instant>,
    session: Option<zbus::blocking::Connection>,
}

fn is_text(m: &str) -> bool {
    m.starts_with("text/") || matches!(m, "UTF8_STRING" | "TEXT" | "STRING" | "COMPOUND_TEXT")
}

/// "pipe:[1234]" of one of our fds.
fn pipe_of(fd: i32) -> Option<String> {
    std::fs::read_link(format!("/proc/self/fd/{fd}"))
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
        .filter(|p| p.starts_with("pipe:"))
}

/// The other process holding the pipe (the paster, or the copying app).
fn peer_of(pipe: &str) -> Option<String> {
    let me = std::process::id();
    let entries = std::fs::read_dir("/proc").ok()?;
    for e in entries.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
        if pid == me {
            continue;
        }
        let Ok(fds) = std::fs::read_dir(e.path().join("fd")) else { continue };
        for fd in fds.flatten() {
            if std::fs::read_link(fd.path()).is_ok_and(|l| l.to_string_lossy() == pipe) {
                return Some(procinfo::user_of(Path::new("/proc"), pid).name);
            }
        }
    }
    None
}

/// Read until EOF; None on timeout, error or more than MAX_BYTES.
fn read_timeout(fd: OwnedFd, total: Duration) -> Option<Vec<u8>> {
    let deadline = Instant::now() + total;
    let mut f = File::from(fd);
    let mut out = Vec::new();
    let mut buf = [0u8; 16384];
    loop {
        let left = deadline.checked_duration_since(Instant::now())?;
        let mut pfd = libc::pollfd { fd: f.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        if unsafe { libc::poll(&mut pfd, 1, left.as_millis() as i32 + 1) } <= 0 {
            return None;
        }
        match f.read(&mut buf) {
            Ok(0) => return Some(out),
            Ok(n) => {
                out.extend_from_slice(&buf[..n]);
                if out.len() > MAX_BYTES {
                    return None;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
}

fn pipe() -> Option<(OwnedFd, OwnedFd)> {
    let mut fds = [0i32; 2];
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return None;
    }
    Some(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

impl State {
    fn notify(&mut self, app: &str, owner: &str) {
        if self.notified.get(app).is_some_and(|t| t.elapsed() < Duration::from_secs(10)) {
            return;
        }
        self.notified.insert(app.to_owned(), Instant::now());
        if self.session.is_none() {
            self.session = zbus::blocking::Connection::session().ok();
        }
        let Some(conn) = &self.session else { return };
        let body = if owner.is_empty() || owner == app {
            format!("{app} pasted from the clipboard.")
        } else {
            format!("{app} pasted text copied in {owner}.")
        };
        log(&body);
        let hints: HashMap<&str, zbus::zvariant::Value> = HashMap::from([("urgency", zbus::zvariant::Value::U8(1))]);
        let _ = conn.call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &("fp6-privacy-clipboard", 0u32, "edit-paste", "Clipboard", body.as_str(), Vec::<&str>::new(), hints, 4000i32),
        );
    }

    fn drop_owned(&mut self) {
        if let Some(s) = self.owned.source.take() {
            s.destroy();
        }
        self.owned = Owned::default();
    }

    /// A new selection from another client.
    fn take_selection(&mut self, conn: &Connection, offer: ExtDataControlOfferV1, qh: &QueueHandle<Self>) {
        let mimes = self.offers.remove(&offer.id()).unwrap_or_default();
        self.drop_owned();
        self.selection_at = Some(Instant::now());
        let want = self.cfg.notify.load(Ordering::SeqCst)
            && !mimes.is_empty()
            && mimes.iter().all(|m| is_text(m))
            && mimes.len() <= 12;
        if want {
            let mut data = Vec::new();
            let mut owner = String::new();
            for m in &mimes {
                let Some((r, w)) = pipe() else { break };
                offer.receive(m.clone(), w.as_fd());
                let _ = conn.flush();
                if owner.is_empty() {
                    if let Some(p) = pipe_of(w.as_raw_fd()) {
                        // give the source a moment to receive the fd
                        std::thread::sleep(Duration::from_millis(30));
                        owner = peer_of(&p).unwrap_or_default();
                    }
                }
                drop(w);
                let buf = read_timeout(r, Duration::from_secs(2));
                let ok = buf.is_some();
                let buf = buf.unwrap_or_default();
                if !ok || buf.len() > MAX_BYTES {
                    data.clear();
                    break;
                }
                data.push((m.clone(), Arc::new(buf)));
            }
            if let (false, Some(mgr), Some(dev)) = (data.is_empty(), &self.manager, &self.device) {
                let source = mgr.create_data_source(qh, ());
                for (m, _) in &data {
                    source.offer(m.clone());
                }
                dev.set_selection(Some(&source));
                self.owned = Owned { source: Some(source), data, owner };
            }
        }
        offer.destroy();
    }

    fn tick(&mut self) -> Option<Duration> {
        let minutes = self.cfg.clear_minutes.load(Ordering::SeqCst);
        let at = self.selection_at?;
        if minutes == 0 {
            return None;
        }
        let deadline = at + Duration::from_secs(minutes as u64 * 60);
        let now = Instant::now();
        if now >= deadline {
            if let Some(dev) = &self.device {
                dev.set_selection(None);
                log(format!("clipboard cleared ({minutes} min after the last copy)"));
            }
            self.drop_owned();
            self.selection_at = None;
            return None;
        }
        Some(deadline - now)
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(state: &mut Self, reg: &wl_registry::WlRegistry, event: wl_registry::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
        if let wl_registry::Event::Global { name, interface, .. } = event {
            match interface.as_str() {
                "wl_seat" if state.seat.is_none() => state.seat = Some(reg.bind(name, 1, qh, ())),
                "ext_data_control_manager_v1" => state.manager = Some(reg.bind(name, 1, qh, ())),
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(_: &mut Self, _: &wl_seat::WlSeat, _: wl_seat::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ExtDataControlManagerV1, ()> for State {
    fn event(_: &mut Self, _: &ExtDataControlManagerV1, _: <ExtDataControlManagerV1 as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ExtDataControlDeviceV1, ()> for State {
    fn event(state: &mut Self, _: &ExtDataControlDeviceV1, event: ext_data_control_device_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            ext_data_control_device_v1::Event::DataOffer { id } => {
                state.offers.insert(id.id(), Vec::new());
            }
            ext_data_control_device_v1::Event::Selection { id } => match id {
                Some(offer) => state.pending = Some(offer),
                None => {
                    state.drop_owned();
                    state.selection_at = None;
                }
            },
            ext_data_control_device_v1::Event::PrimarySelection { id } => {
                if let Some(o) = id {
                    state.offers.remove(&o.id());
                    o.destroy();
                }
            }
            ext_data_control_device_v1::Event::Finished => {
                log("data control device finished");
                state.device = None;
            }
            _ => {}
        }
    }

    event_created_child!(State, ExtDataControlDeviceV1, [
        ext_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ExtDataControlOfferV1, ()),
    ]);
}

impl Dispatch<ExtDataControlOfferV1, ()> for State {
    fn event(state: &mut Self, offer: &ExtDataControlOfferV1, event: ext_data_control_offer_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let ext_data_control_offer_v1::Event::Offer { mime_type } = event {
            state.offers.entry(offer.id()).or_default().push(mime_type);
        }
    }
}

impl Dispatch<ExtDataControlSourceV1, ()> for State {
    fn event(state: &mut Self, source: &ExtDataControlSourceV1, event: ext_data_control_source_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            ext_data_control_source_v1::Event::Send { mime_type, fd } => {
                let data = state.owned.data.iter().find(|(m, _)| *m == mime_type).map(|(_, d)| d.clone());
                let pipe = pipe_of(fd.as_raw_fd());
                if let Some(data) = data {
                    let mut f = File::from(fd);
                    std::thread::spawn(move || {
                        let _ = f.write_all(&data);
                    });
                    if let Some(app) = pipe.and_then(|p| peer_of(&p)) {
                        let owner = state.owned.owner.clone();
                        if app != owner {
                            state.notify(&app, &owner);
                        }
                    }
                }
            }
            ext_data_control_source_v1::Event::Cancelled => {
                if state.owned.source.as_ref() == Some(source) {
                    state.owned = Owned::default();
                }
                source.destroy();
            }
            _ => {}
        }
    }
}

fn run(cfg: Arc<Cfg>) -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|e| format!("Wayland: {e}"))?;
    let mut queue = conn.new_event_queue::<State>();
    let qh = queue.handle();
    conn.display().get_registry(&qh, ());
    let mut state = State {
        cfg,
        seat: None,
        manager: None,
        device: None,
        offers: HashMap::new(),
        pending: None,
        selection_at: None,
        owned: Owned::default(),
        notified: HashMap::new(),
        session: None,
    };
    queue.roundtrip(&mut state).map_err(|e| format!("roundtrip: {e}"))?;
    let (Some(mgr), Some(seat)) = (&state.manager, &state.seat) else {
        return Err("the compositor does not offer ext_data_control_manager_v1 to this client".into());
    };
    state.device = Some(mgr.get_data_device(seat, &qh, ()));
    log("watching the clipboard");
    loop {
        queue.dispatch_pending(&mut state).map_err(|e| e.to_string())?;
        if let Some(offer) = state.pending.take() {
            state.take_selection(&conn, offer, &qh);
        }
        let wait = state.tick();
        queue.flush().map_err(|e| e.to_string())?;
        if let Some(guard) = queue.prepare_read() {
            let mut pfd = libc::pollfd { fd: guard.connection_fd().as_raw_fd(), events: libc::POLLIN, revents: 0 };
            // Re-check the settings at least every 30 s (timeout changes).
            let ms = wait.map_or(30_000, |d| d.as_millis().min(30_000) as i32 + 1);
            let r = unsafe { libc::poll(&mut pfd, 1, ms) };
            if r > 0 {
                guard.read().map_err(|e| e.to_string())?;
            }
        }
        if state.device.is_none() {
            return Err("data control device gone".into());
        }
    }
}

fn main() {
    let cfg = Arc::new(Cfg { clear_minutes: AtomicU32::new(0), notify: AtomicBool::new(false) });
    let settings = gio::SettingsSchemaSource::default()
        .and_then(|s| s.lookup(SCHEMA, true))
        .map(|_| gio::Settings::new(SCHEMA));
    let Some(settings) = settings else {
        log(format!("GSettings schema {SCHEMA} is not installed"));
        std::process::exit(1);
    };
    let load = {
        let cfg = cfg.clone();
        move |s: &gio::Settings| {
            cfg.clear_minutes.store(s.uint("clipboard-clear-minutes"), Ordering::SeqCst);
            cfg.notify.store(s.boolean("clipboard-read-notify"), Ordering::SeqCst);
        }
    };
    load(&settings);
    settings.connect_changed(None, move |s, _| load(s));
    {
        let cfg = cfg.clone();
        std::thread::spawn(move || {
            if let Err(e) = run(cfg) {
                log(e);
                std::process::exit(1);
            }
        });
    }
    glib::MainLoop::new(None, false).run();
}
