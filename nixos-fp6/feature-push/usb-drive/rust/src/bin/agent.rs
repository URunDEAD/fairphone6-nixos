//! fp6-usb-drive-agent (user session): the persistent "USB Drive" status
//! while an image is hosted. A Lomiri indicator (com.fairphone6.indicator.
//! usbdrive, visible only while hosting) shows the image, mode, host
//! connection and transfer rate, with "Stop sharing" and "Open USB Drive";
//! a notification bubble is shown when hosting starts and when it ends
//! automatically (cable unplugged in exclusive mode, time limit).

use fp6_usbdrive::client::{BUS_NAME, IFACE, PATH};
use gio::prelude::*;
use glib::{Variant, VariantDict};
use serde_json::Value;
use std::cell::RefCell;
use std::rc::Rc;

const IND_NAME: &str = "com.fairphone6.indicator.usbdrive";
const IND_PATH: &str = "/com/fairphone6/indicator/usbdrive";

fn log(m: impl AsRef<str>) {
    eprintln!("fp6-usb-drive-agent: {}", m.as_ref());
}

fn human(b: f64) -> String {
    let u = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b;
    let mut i = 0;
    while v >= 1000.0 && i < u.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 { format!("{v:.0} {}", u[i]) } else { format!("{v:.1} {}", u[i]) }
}

pub fn mode_text(m: &str) -> &'static str {
    match m {
        "cdrom" => "CD-ROM",
        "disk-ro" => "USB disk (read-only)",
        "disk-rw" => "USB disk (read-write)",
        _ => "USB drive",
    }
}

fn lines(st: &Value) -> Vec<String> {
    let mut v = vec![];
    let name = st["name"].as_str().unwrap_or("image");
    v.push(format!("{name}"));
    let mut m = mode_text(st["mode"].as_str().unwrap_or("")).to_string();
    if st["exclusive"] == true {
        m.push_str(" · exclusive");
    }
    v.push(m);
    v.push(if st["hostConnected"] == true { "Computer connected".into() } else { "Waiting for a computer".into() });
    let r = st["readBytes"].as_f64().unwrap_or(0.0);
    let w = st["writtenBytes"].as_f64().unwrap_or(0.0);
    let mut io = format!("Read {}", human(r));
    if w > 0.0 {
        io.push_str(&format!(" · written {}", human(w)));
    }
    if st["active"] == true {
        io.push_str(&format!(" · {}/s", human(st["readRate"].as_f64().unwrap_or(0.0) + st["writeRate"].as_f64().unwrap_or(0.0))));
    }
    v.push(io);
    if st["exclusive"] == true {
        v.push("USB network and ADB are off until you stop".into());
    }
    v
}

fn header(st: &Value) -> Variant {
    let d = VariantDict::new(None);
    d.insert_value("title", &"USB Drive".to_variant());
    let icon = gio::ThemedIcon::from_names(&["stock_usb", "drive-removable-symbolic", "media-optical-symbolic"]);
    if let Some(v) = icon.serialize() {
        d.insert_value("icon", &v);
    }
    let hosting = st["hosting"] == true;
    d.insert_value("accessible-desc", &format!("Sharing {} over USB", st["name"].as_str().unwrap_or("an image")).to_variant());
    d.insert_value("visible", &hosting.to_variant());
    d.end()
}

struct Agent {
    session: gio::DBusConnection,
    system: gio::DBusConnection,
    header: gio::SimpleAction,
    info: gio::Menu,
    shown: Vec<String>,
    last: Value,
    notified_path: String,
}

impl Agent {
    fn notify(&self, summary: &str, body: &str) {
        let hints = VariantDict::new(None);
        let args = ("USB Drive", 0u32, "stock_usb", summary, body, Vec::<String>::new(), hints.end(), 6000i32).to_variant();
        let r = self.session.call_sync(Some("org.freedesktop.Notifications"), "/org/freedesktop/Notifications",
                                       "org.freedesktop.Notifications", "Notify", Some(&args), None,
                                       gio::DBusCallFlags::NONE, 3000, None::<&gio::Cancellable>);
        if let Err(e) = r {
            log(format!("Notify: {e}"));
        }
    }

    fn update(&mut self, st: Value) {
        let hosting = st["hosting"] == true;
        let was = self.last["hosting"] == true;
        if hosting && st["path"].as_str() != Some(&self.notified_path) {
            self.notified_path = st["path"].as_str().unwrap_or("").into();
            let mut body = format!("As {}. The computer on the USB cable sees a new drive.", mode_text(st["mode"].as_str().unwrap_or("")));
            if st["exclusive"] == true {
                body.push_str(" USB network and ADB are off until you stop sharing or unplug the cable.");
            }
            self.notify(&format!("Sharing {}", st["name"].as_str().unwrap_or("image")), &body);
        }
        if !hosting && was {
            self.notified_path.clear();
            let reason = st["lastStopReason"].as_str().unwrap_or("");
            if reason.contains("unplugged") || reason.contains("limit") || reason.contains("helper") {
                self.notify("USB sharing stopped", &format!("{reason}. The normal USB connection is back."));
            }
        }
        self.header.set_state(&header(&st));
        let l = if hosting { lines(&st) } else { vec![] };
        if l != self.shown {
            self.info.remove_all();
            for x in &l {
                self.info.append_item(&gio::MenuItem::new(Some(x), None));
            }
            self.shown = l;
        }
        self.last = st;
    }

    fn refresh(&mut self) {
        let r = self.system.call_sync(Some(BUS_NAME), PATH, IFACE, "GetStatus", None, None,
                                      gio::DBusCallFlags::NO_AUTO_START, 3000, None::<&gio::Cancellable>);
        match r.ok().and_then(|v| v.child_value(0).get::<String>()) {
            Some(s) => self.update(serde_json::from_str(&s).unwrap_or_default()),
            None => self.update(serde_json::json!({"hosting": false})),
        }
    }
}

thread_local! {
    static MENU: RefCell<Option<gio::Menu>> = const { RefCell::new(None) };
}

fn main() {
    let session = match gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>) {
        Ok(c) => c,
        Err(e) => {
            log(format!("session bus: {e}"));
            std::process::exit(1);
        }
    };
    let system = match gio::bus_get_sync(gio::BusType::System, None::<&gio::Cancellable>) {
        Ok(c) => c,
        Err(e) => {
            log(format!("system bus: {e}"));
            std::process::exit(1);
        }
    };
    let actions = gio::SimpleActionGroup::new();
    let head = gio::SimpleAction::new_stateful("phone-header", None, &header(&serde_json::json!({})));
    actions.add_action(&head);
    let stop = gio::SimpleAction::new("stop", None);
    {
        let sys = system.clone();
        stop.connect_activate(move |_, _| {
            sys.call(Some(BUS_NAME), PATH, IFACE, "Stop", None, None, gio::DBusCallFlags::NONE, 30000,
                     None::<&gio::Cancellable>, |r| if let Err(e) = r { log(format!("Stop: {e}")) });
        });
    }
    actions.add_action(&stop);
    let open = gio::SimpleAction::new("open", None);
    open.connect_activate(|_, _| {
        let _ = std::process::Command::new("lomiri-url-dispatcher").arg("application:///fp6-usb-drive.desktop").spawn();
    });
    actions.add_action(&open);

    let info = gio::Menu::new();
    let controls = gio::Menu::new();
    controls.append_item(&gio::MenuItem::new(Some("Stop sharing"), Some("indicator.stop")));
    controls.append_item(&gio::MenuItem::new(Some("Open USB Drive…"), Some("indicator.open")));
    let sub = gio::Menu::new();
    sub.append_section(None, &info);
    sub.append_section(None, &controls);
    let root = gio::MenuItem::new(None, Some("indicator.phone-header"));
    root.set_attribute_value("x-ayatana-type", Some(&"org.ayatana.indicator.root".to_variant()));
    root.set_submenu(Some(&sub));
    let menu = gio::Menu::new();
    menu.append_item(&root);
    MENU.with(|m| *m.borrow_mut() = Some(menu.clone()));

    if let Err(e) = session.export_action_group(IND_PATH, &actions) {
        log(format!("export actions: {e}"));
        std::process::exit(1);
    }
    for profile in ["phone", "phone_greeter", "desktop", "desktop_greeter"] {
        if let Err(e) = session.export_menu_model(&format!("{IND_PATH}/{profile}"), &menu) {
            log(format!("export menu: {e}"));
        }
    }
    let _owner = gio::bus_own_name_on_connection(&session, IND_NAME, gio::BusNameOwnerFlags::NONE,
                                                 |_, _| {}, |_, n| log(format!("lost bus name {n}")));

    let agent = Rc::new(RefCell::new(Agent { session, system: system.clone(), header: head, info, shown: vec![],
                                             last: serde_json::json!({}), notified_path: String::new() }));
    agent.borrow_mut().refresh();
    {
        let a = agent.clone();
        #[allow(deprecated)]
        system.signal_subscribe(Some(BUS_NAME), Some(IFACE), Some("StateChanged"), Some(PATH), None,
                                gio::DBusSignalFlags::NONE, move |_, _, _, _, _, params| {
            if let Some((s,)) = params.get::<(String,)>() {
                a.borrow_mut().update(serde_json::from_str(&s).unwrap_or_default());
            }
        });
    }
    {
        let a = agent.clone();
        let w = gio::bus_watch_name_on_connection(&system, BUS_NAME, gio::BusNameWatcherFlags::NONE,
            move |_, _, _| a.borrow_mut().refresh(),
            {
                let a = agent.clone();
                move |_, _| a.borrow_mut().update(serde_json::json!({"hosting": false}))
            });
        std::mem::forget(w);
    }
    // Rate/byte counters between signals.
    {
        let a = agent.clone();
        glib::timeout_add_seconds_local(5, move || {
            if a.borrow().last["hosting"] == true {
                a.borrow_mut().refresh();
            }
            glib::ControlFlow::Continue
        });
    }
    log("ready");
    glib::MainLoop::new(None, false).run();
}
