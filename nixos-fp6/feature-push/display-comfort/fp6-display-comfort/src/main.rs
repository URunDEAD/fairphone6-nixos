// SPDX-License-Identifier: MIT
//! fp6-display-comfort: user service of the Lomiri session.
//!
//!  * Night light: computes the current colour temperature from the schedule
//!    (always / sunset-sunrise / custom, with gradual ramps) and writes it to
//!    com.fairphone6.display-comfort night-light-current; Lomiri's colour
//!    filter (ShellScreen.qml) follows that key.
//!  * Dark theme schedule: switches UITK (theme.ini, live in running apps),
//!    GTK (dconf org.gnome.desktop.interface color-scheme / gtk-theme and
//!    gtk-3.0/settings.ini) and optionally the shell (com.lomiri.Shell
//!    light-mode) when the schedule state changes.
//!  * Scale: mirrors interface-scale / font-scale into
//!    ~/.config/fp6-display-comfort/session.env (read by lomiri-session at
//!    login) and applies the GTK text scale at session start.
//!  * Location: optional GeoClue lookup (city accuracy, only answers while
//!    Location is on) every few hours for sunset/sunrise.
//!
//! CLI: `run` (service), `status` (print the computed state once).

mod files;
mod schedule;
mod sun;

use gio::prelude::*;
use schedule::{LocalTime, Schedule};
use std::cell::RefCell;
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

const SCHEMA: &str = "com.fairphone6.display-comfort";
const GEOCLUE_INTERVAL_S: i64 = 6 * 3600;
const GEOCLUE_TIMEOUT_S: u64 = 90;

struct GLibTime;
impl LocalTime for GLibTime {
    fn midnight(&self, t: i64, offset: i32) -> i64 {
        let Ok(dt) = glib::DateTime::from_unix_local(t) else { return t };
        let (y, m, d) = dt.ymd();
        let Ok(day) = glib::DateTime::from_local(y, m, d, 0, 0, 0.0) else { return t };
        day.add_days(offset).map(|x| x.to_unix()).unwrap_or(t)
    }
}

fn now() -> i64 {
    glib::real_time() / 1_000_000
}

fn config_dir() -> PathBuf {
    glib::user_config_dir()
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// Where sunset/sunrise is computed for, and a description.
fn location(s: &gio::Settings) -> (f64, f64, String) {
    let (lat, lon) = (s.double("latitude"), s.double("longitude"));
    if s.boolean("use-location") && s.int64("located-time") > 0 {
        let (la, lo) = (s.double("located-latitude"), s.double("located-longitude"));
        return (la, lo, format!("location {la:.1}, {lo:.1}"));
    }
    (lat, lon, format!("fixed {lat:.2}, {lon:.2}"))
}

fn night_schedule(s: &gio::Settings) -> Schedule {
    match s.string("night-light-schedule").as_str() {
        "always" => Schedule::Always,
        "custom" => custom(s, "night-light-from", "night-light-to", 22 * 60, 7 * 60),
        _ => {
            let (lat, lon, _) = location(s);
            Schedule::Sun { lat, lon }
        }
    }
}

fn custom(s: &gio::Settings, from: &str, to: &str, df: u32, dt: u32) -> Schedule {
    Schedule::Custom {
        from_min: schedule::parse_hhmm(&s.string(from)).unwrap_or(df),
        to_min: schedule::parse_hhmm(&s.string(to)).unwrap_or(dt),
    }
}

/// None = "off" (leave the theme alone).
fn dark_schedule(s: &gio::Settings) -> Option<Schedule> {
    match s.string("dark-theme-mode").as_str() {
        "on" => Some(Schedule::Always),
        "follow-night-light" => Some(night_schedule(s)),
        "sunset-sunrise" => {
            let (lat, lon, _) = location(s);
            Some(Schedule::Sun { lat, lon })
        }
        "custom" => Some(custom(s, "dark-theme-from", "dark-theme-to", 21 * 60, 7 * 60)),
        _ => None,
    }
}

struct Computed {
    temperature: i32,
    factor: f64,
    dark: Option<bool>,
    next_s: i64,
    status: String,
}

fn compute(s: &gio::Settings, t: i64) -> Computed {
    let tz = GLibTime;
    let ramp = s.int("transition-minutes").clamp(0, 120) as i64 * 60;
    let nsched = night_schedule(s);
    let niv = schedule::intervals(&nsched, t, &tz);
    let enabled = s.boolean("night-light-enabled");
    let factor = if enabled { schedule::factor(&niv, t, ramp) } else { 0.0 };
    let temperature = schedule::temperature(factor, s.int("night-light-temperature"));
    let ramping = factor > 0.0 && factor < 1.0;

    let dsched = dark_schedule(s);
    let div = dsched.as_ref().map(|d| schedule::intervals(d, t, &tz));
    let dark = div.as_ref().map(|iv| schedule::inside(iv, t));

    let mut next = 600;
    if enabled {
        if ramping {
            // ~60 steps per ramp, at least 10 s apart.
            next = next.min((ramp / 60).max(10));
        } else if let Some(e) = schedule::next_edge(&niv, t, ramp) {
            next = next.min(e.max(1));
        }
    }
    if let Some(iv) = &div {
        if let Some(e) = schedule::next_edge(iv, t, 0) {
            next = next.min(e.max(1));
        }
    }

    let fmt = |u: i64| {
        glib::DateTime::from_unix_local(u)
            .ok()
            .and_then(|d| d.format("%H:%M").ok())
            .map(|g| g.to_string())
            .unwrap_or_default()
    };
    let mut status = String::new();
    if enabled {
        // The current or next night window.
        let window = niv.iter().find(|&&(_, e)| e > t).copied();
        let where_ = match nsched {
            Schedule::Sun { .. } => format!(" (sunset to sunrise, {})", location(s).2),
            _ => String::new(),
        };
        let forever = |u: i64| u > t + 400 * 86400;
        status = match window {
            _ if factor >= 1.0 && window.is_none_or(|w| forever(w.1)) => format!("On, {temperature} K"),
            Some((_, en)) if factor >= 1.0 => format!("On until {}, {temperature} K{where_}", fmt(en)),
            Some(_) if factor > 0.0 => format!("Changing, {temperature} K{where_}"),
            Some((st, _)) if st > t => format!("Off until {}{where_}", fmt(st)),
            _ => format!("Off{where_}"),
        };
    }
    Computed { temperature, factor, dark, next_s: next, status }
}

fn shell_settings() -> Option<gio::Settings> {
    let src = gio::SettingsSchemaSource::default()?;
    src.lookup("com.lomiri.Shell", true)?;
    Some(gio::Settings::new("com.lomiri.Shell"))
}

fn dconf_write(key: &str, value: Option<&str>) {
    let mut cmd = Command::new("dconf");
    match value {
        Some(v) => cmd.args(["write", key, v]),
        None => cmd.args(["reset", key]),
    };
    match cmd.status() {
        Ok(st) if st.success() => {}
        Ok(st) => eprintln!("fp6-display-comfort: dconf {key}: {st}"),
        Err(e) => eprintln!("fp6-display-comfort: dconf: {e}"),
    }
}

fn apply_dark(s: &gio::Settings, dark: bool) {
    let cfg = config_dir();
    let theme = if dark { files::THEME_DARK } else { files::THEME_LIGHT };
    // UITK apps watch theme.ini and switch live.
    let _ = files::edit_ini(&cfg.join("lomiri-ui-toolkit/theme.ini"), "General", "theme", Some(theme))
        .map_err(|e| eprintln!("fp6-display-comfort: theme.ini: {e}"));
    // GTK: GSettings (apps with gsettings-desktop-schemas: GTK3 gtk-theme,
    // libadwaita color-scheme, Firefox follows the GTK theme) and settings.ini.
    dconf_write(
        "/org/gnome/desktop/interface/color-scheme",
        Some(if dark { "'prefer-dark'" } else { "'default'" }),
    );
    dconf_write("/org/gnome/desktop/interface/gtk-theme", Some(if dark { "'Adwaita-dark'" } else { "'Adwaita'" }));
    let _ = files::edit_ini(
        &cfg.join("gtk-3.0/settings.ini"),
        "Settings",
        "gtk-application-prefer-dark-theme",
        if dark { Some("true") } else { None },
    );
    if s.boolean("dark-theme-shell") {
        if let Some(sh) = shell_settings() {
            if sh.boolean("light-mode") == dark {
                let _ = sh.set_boolean("light-mode", !dark);
            }
        }
    }
    let _ = s.set_boolean("dark-theme-active", dark);
    eprintln!("fp6-display-comfort: dark theme {}", if dark { "on" } else { "off" });
}

/// session.env for the next login.
fn write_scale_file(s: &gio::Settings) {
    let content = files::session_env(s.int("interface-scale"), s.int("font-scale"));
    if let Err(e) = files::write_if_changed(&config_dir().join("fp6-display-comfort/session.env"), &content) {
        eprintln!("fp6-display-comfort: session.env: {e}");
    }
}

/// GTK text scale, applied once at session start (= what this login uses).
fn apply_gtk_font_scale(pct: i32) {
    let pct = pct.clamp(80, 150);
    if pct == 100 {
        dconf_write("/org/gnome/desktop/interface/text-scaling-factor", None);
    } else {
        dconf_write("/org/gnome/desktop/interface/text-scaling-factor", Some(&format!("{:.2}", pct as f64 / 100.0)));
    }
    let dpi = (96 * 1024 * pct / 100).to_string();
    let _ = files::edit_ini(
        &config_dir().join("gtk-3.0/settings.ini"),
        "Settings",
        "gtk-xft-dpi",
        if pct == 100 { None } else { Some(dpi.as_str()) },
    );
}

/// Blocking GeoClue lookup (own thread). City accuracy; GeoClue only has a
/// source while the user's Location switch is on, otherwise this times out.
fn geoclue_lookup() -> Result<(f64, f64), String> {
    let e = |x: glib::Error| x.to_string();
    let bus = gio::bus_get_sync(gio::BusType::System, gio::Cancellable::NONE).map_err(e)?;
    let call = |path: &str, iface: &str, method: &str, args: Option<&glib::Variant>| {
        bus.call_sync(
            Some("org.freedesktop.GeoClue2"),
            path,
            iface,
            method,
            args,
            None,
            gio::DBusCallFlags::NONE,
            10_000,
            gio::Cancellable::NONE,
        )
    };
    let set = |path: &str, prop: &str, v: glib::Variant| {
        call(
            path,
            "org.freedesktop.DBus.Properties",
            "Set",
            Some(&("org.freedesktop.GeoClue2.Client", prop, v).to_variant()),
        )
    };
    let get = |path: &str, iface: &str, prop: &str| -> Result<glib::Variant, glib::Error> {
        let r = call(path, "org.freedesktop.DBus.Properties", "Get", Some(&(iface, prop).to_variant()))?;
        Ok(r.child_value(0).as_variant().unwrap_or(r))
    };
    let r = call("/org/freedesktop/GeoClue2/Manager", "org.freedesktop.GeoClue2.Manager", "GetClient", None)
        .map_err(e)?;
    let client: String = r
        .child_value(0)
        .get::<glib::variant::ObjectPath>()
        .map(|p| p.as_str().to_string())
        .ok_or("GetClient: no path")?;
    set(&client, "DesktopId", "fp6-display-comfort".to_variant()).map_err(e)?;
    set(&client, "RequestedAccuracyLevel", 4u32.to_variant()).map_err(e)?; // CITY
    call(&client, "org.freedesktop.GeoClue2.Client", "Start", None).map_err(e)?;
    let mut result = Err("no location (Location off?)".to_string());
    for _ in 0..(GEOCLUE_TIMEOUT_S / 3) {
        std::thread::sleep(Duration::from_secs(3));
        let loc = get(&client, "org.freedesktop.GeoClue2.Client", "Location").map_err(e)?;
        let path = loc.get::<glib::variant::ObjectPath>().map(|p| p.as_str().to_string()).unwrap_or_default();
        if !path.is_empty() && path != "/" {
            let lat = get(&path, "org.freedesktop.GeoClue2.Location", "Latitude").map_err(e)?.get::<f64>();
            let lon = get(&path, "org.freedesktop.GeoClue2.Location", "Longitude").map_err(e)?.get::<f64>();
            if let (Some(lat), Some(lon)) = (lat, lon) {
                result = Ok((round1(lat), round1(lon)));
            }
            break;
        }
    }
    let _ = call(&client, "org.freedesktop.GeoClue2.Client", "Stop", None);
    result
}

struct State {
    settings: gio::Settings,
    timer: Option<glib::SourceId>,
    applied_dark: Option<bool>,
    dark_mode: String,
    geoclue_busy: bool,
    geoclue_last_try: i64,
}

fn tick(st: &Rc<RefCell<State>>) {
    let t = now();
    let (next, maybe_geo) = {
        let mut me = st.borrow_mut();
        let s = me.settings.clone();
        let c = compute(&s, t);
        if s.int("night-light-current") != c.temperature {
            let _ = s.set_int("night-light-current", c.temperature);
        }
        if s.string("status").as_str() != c.status {
            let _ = s.set_string("status", &c.status);
        }
        let mode = s.string("dark-theme-mode").to_string();
        if mode != me.dark_mode {
            me.dark_mode = mode;
            me.applied_dark = None; // re-evaluate on a mode change
        }
        if let Some(d) = c.dark {
            if me.applied_dark != Some(d) {
                apply_dark(&s, d);
                me.applied_dark = Some(d);
            }
        }
        let _ = c.factor;
        let geo = s.boolean("use-location")
            && (s.boolean("night-light-enabled") || c.dark.is_some())
            && !me.geoclue_busy
            && t - s.int64("located-time") > GEOCLUE_INTERVAL_S
            && t - me.geoclue_last_try > GEOCLUE_INTERVAL_S / 6;
        if geo {
            me.geoclue_busy = true;
            me.geoclue_last_try = t;
        }
        (c.next_s, geo)
    };
    if maybe_geo {
        start_geoclue(st);
    }
    schedule_tick(st, next);
}

fn schedule_tick(st: &Rc<RefCell<State>>, secs: i64) {
    let weak = Rc::downgrade(st);
    let id = glib::timeout_add_seconds_local(secs.clamp(1, 600) as u32, move || {
        if let Some(st) = weak.upgrade() {
            st.borrow_mut().timer = None;
            tick(&st);
        }
        glib::ControlFlow::Break
    });
    if let Some(old) = st.borrow_mut().timer.replace(id) {
        old.remove();
    }
}

fn start_geoclue(st: &Rc<RefCell<State>>) {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(geoclue_lookup());
    });
    let weak = Rc::downgrade(st);
    glib::timeout_add_seconds_local(3, move || {
        let Some(st) = weak.upgrade() else { return glib::ControlFlow::Break };
        match rx.try_recv() {
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                st.borrow_mut().geoclue_busy = false;
                glib::ControlFlow::Break
            }
            Ok(r) => {
                st.borrow_mut().geoclue_busy = false;
                match r {
                    Ok((lat, lon)) => {
                        let s = st.borrow().settings.clone();
                        eprintln!("fp6-display-comfort: GeoClue location {lat:.1}, {lon:.1}");
                        let _ = s.set_double("located-latitude", lat);
                        let _ = s.set_double("located-longitude", lon);
                        let _ = s.set_int64("located-time", now());
                        tick(&st);
                    }
                    Err(e) => eprintln!("fp6-display-comfort: GeoClue: {e}"),
                }
                glib::ControlFlow::Break
            }
        }
    });
}

fn run() {
    let settings = gio::Settings::new(SCHEMA);
    let st = Rc::new(RefCell::new(State {
        settings: settings.clone(),
        timer: None,
        applied_dark: None,
        dark_mode: settings.string("dark-theme-mode").to_string(),
        geoclue_busy: false,
        geoclue_last_try: 0,
    }));
    // At login: publish what lomiri-session read for this session, then
    // write the file for the next one.
    let env_path = config_dir().join("fp6-display-comfort/session.env");
    let (gu, pct) = files::parse_session_env(&std::fs::read_to_string(&env_path).unwrap_or_default());
    let _ = settings.set_int("session-interface-scale", gu);
    let _ = settings.set_int("session-font-scale", pct);
    write_scale_file(&settings);
    apply_gtk_font_scale(pct);

    let weak = Rc::downgrade(&st);
    settings.connect_changed(None, move |s, key| {
        // Ignore our own state keys.
        if matches!(key, "night-light-current" | "status" | "dark-theme-active" | "located-time"
            | "located-latitude" | "located-longitude" | "greyscale-active"
            | "session-interface-scale" | "session-font-scale")
        {
            return;
        }
        if matches!(key, "interface-scale" | "font-scale") {
            write_scale_file(s);
            return;
        }
        if let Some(st) = weak.upgrade() {
            if key == "use-location" {
                st.borrow_mut().geoclue_last_try = 0;
            }
            tick(&st);
        }
    });
    // Clock jumps / time zone changes: the timer caps at 10 minutes anyway;
    // also re-check on resume-like gaps.
    tick(&st);
    glib::MainLoop::new(None, false).run();
}

fn status() {
    let s = gio::Settings::new(SCHEMA);
    let c = compute(&s, now());
    println!("night-light-current {}", c.temperature);
    println!("factor {:.3}", c.factor);
    println!("dark {:?}", c.dark);
    println!("next {} s", c.next_s);
    println!("status {}", c.status);
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("run") => run(),
        Some("status") => status(),
        _ => {
            eprintln!("usage: fp6-display-comfort run|status");
            std::process::exit(2);
        }
    }
}
