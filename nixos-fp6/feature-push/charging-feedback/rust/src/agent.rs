//! Session agent: watches power_supply/typec kobject uevents, publishes the
//! charging state as an org.gtk.Actions group on the session bus for
//! Lomiri's QDBusActionGroup, and wakes the display briefly on request
//! (fp6-lomiri-powerd com.fairphone6.Gestures.Peek: display on, off again
//! after the given seconds without input; refused in a pocket or a call).
//!
//!   bus name  com.fairphone6.ChargingFeedback
//!   path      /com/fairphone6/ChargingFeedback
//!   actions   state   (stateful, s: JSON, see model::compose)
//!             wake    (i: seconds) Peek the display
//!             refresh ()            read sysfs now
//!             simulate (s: "plug" | "unplug") emit an event with the
//!                       current readings (phone checks without a cable)
//!
//! Policy (animation, sound, vibration, wake, lock-screen line, Do Not
//! Disturb, silent mode) lives in Lomiri (qml/Components/Fp6Charging*.qml);
//! this process only detects and measures.

use crate::model::{self, Debounce, Event, Inputs, SpeedTracker};
use crate::supply::Supply;
use gio::prelude::*;
use std::cell::RefCell;
use std::os::fd::RawFd;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

pub const BUS_NAME: &str = "com.fairphone6.ChargingFeedback";
pub const PATH: &str = "/com/fairphone6/ChargingFeedback";
/// Re-read this often while plugged in (level, speed, time to full).
const POLL_PLUGGED_S: u32 = 10;
/// Extra reads after a plug-in while the charger negotiates (s).
const AFTER_PLUG: [u64; 5] = [700, 1500, 3000, 5000, 8000];

fn log(m: impl AsRef<str>) {
    eprintln!("fp6-charging-feedback: {}", m.as_ref());
}

pub struct Paths {
    pub sysfs: PathBuf,
    pub battd: PathBuf,
    pub usb: PathBuf,
}

pub struct Agent {
    paths: Paths,
    supply: Supply,
    start: Instant,
    deb: Debounce,
    speed: SpeedTracker,
    plugged_at: Option<f64>,
    seq: i64,
    event: &'static str,
    state: gio::SimpleAction,
    last: String,
    recheck: Option<glib::SourceId>,
    poll: Option<glib::SourceId>,
    system: Option<gio::DBusConnection>,
}

fn now_unix() -> i64 {
    glib::real_time() / 1_000_000
}

impl Agent {
    pub fn new(paths: Paths, state: gio::SimpleAction, system: Option<gio::DBusConnection>) -> Agent {
        let supply = Supply::discover(&paths.sysfs);
        if !supply.found() {
            log(format!("no battery under {}", paths.sysfs.display()));
        }
        Agent { paths, supply, start: Instant::now(), deb: Debounce::default(), speed: SpeedTracker::default(),
                plugged_at: None, seq: 0, event: "", state, last: String::new(), recheck: None, poll: None, system }
    }

    fn t(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    /// One reading; returns the composed state and an event if one fired.
    pub fn read(&mut self, simulate: Option<Event>) -> (String, Option<Event>, Option<f64>) {
        if !self.supply.found() || self.supply.usb.is_none() {
            self.supply = Supply::discover(&self.paths.sysfs);
        }
        let snap = self.supply.read();
        let usb = std::fs::read_to_string(&self.paths.usb).map(|t| model::parse_usb(&t)).unwrap_or_default();
        let battd = std::fs::read_to_string(&self.paths.battd).ok().and_then(|t| model::parse_battd(&t, now_unix()));
        let raw = (snap.usb_online && !usb.source) || snap.wls_online;
        let now = self.t();
        let mut ev = self.deb.feed(raw, now);
        if ev.is_none() {
            ev = simulate;
        }
        match ev {
            Some(Event::Plug) => {
                self.plugged_at = Some(now);
                self.speed.reset();
            }
            Some(Event::Unplug) => self.plugged_at = None,
            None => {}
        }
        if let Some(e) = ev {
            self.seq += 1;
            self.event = e.name();
        }
        let plugged = self.deb.plugged() || simulate == Some(Event::Plug);
        let age = self.plugged_at.map(|t| now - t);
        let speed = if plugged { self.speed.update(&snap, age) } else { self.speed.speed };
        let json = model::compose(&Inputs { snap: &snap, plugged, battd: battd.as_ref(), usb: &usb, speed,
                                            seq: self.seq, event: self.event });
        let due = self.deb.deadline().map(|d| (d - now).max(0.02));
        (json, ev, due)
    }
}

pub fn refresh(this: &Rc<RefCell<Agent>>, simulate: Option<Event>) {
    let (json, ev, due) = this.borrow_mut().read(simulate);
    {
        let mut a = this.borrow_mut();
        if json != a.last {
            a.state.set_state(&json.to_variant());
            a.last = json;
        }
        if let Some(id) = a.recheck.take() {
            id.remove();
        }
    }
    if let Some(e) = ev {
        log(format!("{} ({})", e.name(), this.borrow().last));
    }
    if let Some(secs) = due {
        let t = this.clone();
        let id = glib::timeout_add_local_once(Duration::from_secs_f64(secs), move || {
            t.borrow_mut().recheck = None;
            refresh(&t, None);
        });
        this.borrow_mut().recheck = Some(id);
    }
    if ev == Some(Event::Plug) {
        for ms in AFTER_PLUG {
            let t = this.clone();
            glib::timeout_add_local_once(Duration::from_millis(ms), move || refresh(&t, None));
        }
    }
    // Poll only while plugged in.
    let plugged = this.borrow().deb.plugged();
    let has_poll = this.borrow().poll.is_some();
    if plugged && !has_poll {
        let t = this.clone();
        let id = glib::timeout_add_seconds_local(POLL_PLUGGED_S, move || {
            refresh(&t, None);
            glib::ControlFlow::Continue
        });
        this.borrow_mut().poll = Some(id);
    } else if !plugged && has_poll {
        if let Some(id) = this.borrow_mut().poll.take() {
            id.remove();
        }
    }
}

fn peek(this: &Rc<RefCell<Agent>>, seconds: i32) {
    let Some(sys) = this.borrow().system.clone() else {
        log("wake: no system bus");
        return;
    };
    let secs = seconds.clamp(3, 30);
    sys.call(Some("com.lomiri.Repowerd"), "/com/fairphone6/Gestures", "com.fairphone6.Gestures", "Peek",
             Some(&(secs,).to_variant()), Some(glib::VariantTy::new("(b)").unwrap()), gio::DBusCallFlags::NONE,
             3000, None::<&gio::Cancellable>, move |r| match r {
                 Ok(v) => log(format!("wake {secs} s: {}", if v.child_value(0).get::<bool>() == Some(true) { "display on" } else { "refused (display on, pocket or call)" })),
                 Err(e) => log(format!("wake: {e}")),
             });
}

// ------------------------------------------------------------- uevents
fn uevent_socket() -> Option<RawFd> {
    unsafe {
        let fd = libc::socket(libc::AF_NETLINK, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
                              libc::NETLINK_KOBJECT_UEVENT);
        if fd < 0 {
            return None;
        }
        let mut addr: libc::sockaddr_nl = std::mem::zeroed();
        addr.nl_family = libc::AF_NETLINK as u16;
        addr.nl_groups = 1; // kernel uevents (receivable without privileges)
        if libc::bind(fd, (&addr as *const libc::sockaddr_nl).cast(), std::mem::size_of::<libc::sockaddr_nl>() as u32) < 0 {
            libc::close(fd);
            return None;
        }
        Some(fd)
    }
}

/// Drains the socket; true if a power_supply/typec event was among them.
fn drain(fd: RawFd) -> bool {
    let mut any = false;
    let mut buf = vec![0u8; 8192];
    loop {
        let n = unsafe { libc::recv(fd, buf.as_mut_ptr().cast(), buf.len(), 0) };
        if n <= 0 {
            break;
        }
        any |= buf[..n as usize].split(|b| *b == 0)
            .any(|f| f == b"SUBSYSTEM=power_supply" || f == b"SUBSYSTEM=typec");
    }
    any
}

pub fn run(paths: Paths) -> Result<(), String> {
    let session = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>).map_err(|e| e.to_string())?;
    let system = gio::bus_get_sync(gio::BusType::System, None::<&gio::Cancellable>).ok();

    let group = gio::SimpleActionGroup::new();
    let state = gio::SimpleAction::new_stateful("state", None, &"{}".to_variant());
    group.add_action(&state);
    let agent = Rc::new(RefCell::new(Agent::new(paths, state, system)));

    let wake = gio::SimpleAction::new("wake", Some(glib::VariantTy::INT32));
    let a = agent.clone();
    wake.connect_activate(move |_, p| peek(&a, p.and_then(|v| v.get::<i32>()).unwrap_or(6)));
    group.add_action(&wake);

    let rf = gio::SimpleAction::new("refresh", None);
    let a = agent.clone();
    rf.connect_activate(move |_, _| refresh(&a, None));
    group.add_action(&rf);

    let sim = gio::SimpleAction::new("simulate", Some(glib::VariantTy::STRING));
    let a = agent.clone();
    sim.connect_activate(move |_, p| {
        let ev = match p.and_then(|v| v.get::<String>()).as_deref() {
            Some("unplug") => Event::Unplug,
            _ => Event::Plug,
        };
        refresh(&a, Some(ev));
    });
    group.add_action(&sim);

    refresh(&agent, None);
    session.export_action_group(PATH, &group).map_err(|e| e.to_string())?;
    let _owner = gio::bus_own_name_on_connection(&session, BUS_NAME, gio::BusNameOwnerFlags::NONE,
                                                 |_, _| {}, |_, name| log(format!("lost bus name {name}")));

    match uevent_socket() {
        Some(fd) => {
            let a = agent.clone();
            glib::unix_fd_add_local(fd, glib::IOCondition::IN, move |fd, _| {
                if drain(fd) {
                    refresh(&a, None);
                }
                glib::ControlFlow::Continue
            });
        }
        None => {
            log("no uevent socket, polling every 2 s");
            let a = agent.clone();
            glib::timeout_add_seconds_local(2, move || {
                refresh(&a, None);
                glib::ControlFlow::Continue
            });
        }
    }
    log("ready");
    glib::MainLoop::new(None, false).run();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supply::fake;

    #[test]
    fn plug_cycle_on_fake_sysfs() {
        let r = fake::fp6("agent");
        let paths = Paths { sysfs: r.clone(), battd: r.join("none.json"), usb: r.join("usb.json") };
        let state = gio::SimpleAction::new_stateful("state", None, &"{}".to_variant());
        let mut a = Agent::new(paths, state, None);
        let (j, ev, due) = a.read(None);
        assert!(j.contains(r#""plugged":false"#) && ev.is_none() && due.is_none());

        fake::put(&r, "qcom-battmgr-usb", &[("online", "1"), ("usb_type", "Unknown [DCP]"), ("current_max", "2000000")]);
        fake::put(&r, "qcom-battmgr-bat", &[("status", "Charging"), ("current_now", "1500000")]);
        let (_, ev, due) = a.read(None);
        assert!(ev.is_none());
        assert!(due.unwrap() <= model::PLUG_SETTLE + 0.01);
        std::thread::sleep(Duration::from_secs_f64(model::PLUG_SETTLE + 0.05));
        let (j, ev, _) = a.read(None);
        assert_eq!(ev, Some(Event::Plug));
        assert!(j.contains(r#""plugged":true"#) && j.contains(r#""status":"charging""#) && j.contains(r#""seq":1,"event":"plug""#), "{j}");

        // Reverse charging (usb-modes: phone is the source) is not "plugged in".
        std::fs::write(r.join("usb.json"), r#"{"peer":"device","cable":true,"power":{"role":"source"}}"#).unwrap();
        a.read(None);
        std::thread::sleep(Duration::from_secs_f64(model::UNPLUG_SETTLE + 0.05));
        let (j, ev, _) = a.read(None);
        assert_eq!(ev, Some(Event::Unplug));
        assert!(j.contains(r#""seq":2,"event":"unplug""#));

        let (j, ev, _) = a.read(Some(Event::Plug));
        assert_eq!(ev, Some(Event::Plug));
        assert!(j.contains(r#""plugged":true"#) && j.contains(r#""seq":3"#), "{j}");
        let _ = std::fs::remove_dir_all(r);
    }
}
