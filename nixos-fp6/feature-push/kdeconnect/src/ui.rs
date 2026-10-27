// qmetaobject's derive macros generate the Qt FFI glue. All handwritten Rust
// uses safe APIs; the audited FFI boundary is confined to this UI executable.
use lomiri_kdeconnect::app::{BUS_NAME, BUS_PATH};
use qmetaobject::prelude::*;
use qmetaobject::QUrl;
use std::sync::{
    mpsc::{sync_channel, Receiver, SyncSender},
    Arc, Mutex,
};
use std::time::Duration;

#[derive(Default)]
struct Cached {
    snapshot: String,
    clipboard: String,
    error: String,
}
struct Command {
    method: String,
    id: String,
    data: String,
    enabled: bool,
}
#[derive(QObject, Default)]
struct Bridge {
    base: qt_base_class!(trait QObject),
    snapshot: qt_property!(QString; NOTIFY changed),
    clipboard: qt_property!(QString; NOTIFY changed),
    error: qt_property!(QString; NOTIFY changed),
    changed: qt_signal!(),
    poll: qt_method!(fn(&mut self)),
    command: qt_method!(fn(&mut self, method: QString, id: QString, data: QString, enabled: bool)),
    cached: Arc<Mutex<Cached>>,
    sender: Option<SyncSender<Command>>,
}
impl Bridge {
    fn poll(&mut self) {
        if let Ok(cache) = self.cached.lock() {
            self.snapshot = cache.snapshot.clone().into();
            self.clipboard = cache.clipboard.clone().into();
            self.error = cache.error.clone().into();
        }
        self.changed();
    }
    fn command(&mut self, method: QString, id: QString, data: QString, enabled: bool) {
        if let Some(sender) = &self.sender {
            if sender
                .try_send(Command {
                    method: method.to_string(),
                    id: id.to_string(),
                    data: data.to_string(),
                    enabled,
                })
                .is_err()
            {
                self.error = "The service is busy. Try again.".into();
                self.changed();
            }
        }
    }
}
fn worker(cache: Arc<Mutex<Cached>>, rx: Receiver<Command>, agent: bool) {
    loop {
        let connection = zbus::blocking::connection::Builder::session()
            .and_then(|b| b.method_timeout(Duration::from_secs(3)).build());
        let result: anyhow::Result<()> = (|| {
            let connection = connection?;
            let proxy = zbus::blocking::Proxy::new(&connection, BUS_NAME, BUS_PATH, BUS_NAME)?;
            loop {
                match rx.recv_timeout(Duration::from_millis(750)) {
                    Ok(cmd) => {
                        let result: zbus::Result<()> = match cmd.method.as_str() {
                            "Discover" | "StopRinging" => proxy.call(cmd.method.as_str(), &()),
                            "RequestPair" | "AcceptPair" => {
                                proxy.call(cmd.method.as_str(), &(cmd.id, cmd.data))
                            }
                            "Unpair" => proxy.call("Unpair", &(cmd.id,)),
                            "SetPlugin" => {
                                proxy.call("SetPlugin", &(cmd.id, cmd.data, cmd.enabled))
                            }
                            "ping" | "ring" | "share" | "media" => {
                                proxy.call("Action", &(cmd.id, cmd.method, cmd.data))
                            }
                            "ReportClipboard" => proxy.call("ReportClipboard", &(cmd.data,)),
                            _ => continue,
                        };
                        cache.lock().unwrap().error =
                            result.err().map(|e| e.to_string()).unwrap_or_default();
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(_) => return Ok(()),
                }
                if agent {
                    let state: String = proxy.call("ClipboardState", &())?;
                    cache.lock().unwrap().clipboard = state;
                } else {
                    let state: String = proxy.call("Devices", &())?;
                    cache.lock().unwrap().snapshot = state;
                }
            }
        })();
        if let Err(e) = result {
            cache.lock().unwrap().error = format!("Service unavailable: {e}");
        } else {
            return;
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}
fn preview_state() -> String {
    serde_json::json!({"name":"Fairphone 6","id":"synthetic-phone","fingerprint":"A1:B2:C3:D4:E5:F6:00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99","event":"Your devices, connected privately.","devices":[
        {"id":"synthetic-laptop","name":"Studio laptop","online":true,"paired":true,"version":8,"fingerprint":"12:34:56:78:9A:BC:DE:F0:12:34:56:78:9A:BC:DE:F0:12:34:56:78:9A:BC:DE:F0:12:34:56:78:9A:BC:DE:F0","plugins":{"ping":true,"battery":true,"notifications":true,"clipboard":false,"share":true,"findmyphone":false,"media":true,"input":false},"battery":{"charge":76,"charging":false},"pending":null},
        {"id":"synthetic-tablet","name":"Living room tablet","online":false,"paired":true,"plugins":{"ping":true,"battery":true},"pending":null},
        {"id":"synthetic-desktop","name":"Home desktop","online":true,"paired":false,"version":8,"fingerprint":"12:34:56:78:9A:BC:DE:F0:12:34:56:78:9A:BC:DE:F0:12:34:56:78:9A:BC:DE:F0:12:34:56:78:9A:BC:DE:F0","plugins":{},"pending":{"direction":"incoming","code":"A4F8C20B","expires":9999999999i64}}
    ]}).to_string()
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let agent = args.iter().any(|a| a == "--clipboard-agent");
    let preview = args.iter().position(|a| a == "--preview");
    let preview_path = preview
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_default();
    let mode = args
        .iter()
        .position(|a| a == "--page")
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
        .unwrap_or("devices");
    let share = args
        .iter()
        .position(|a| a == "--share")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_default();
    let cached = Arc::new(Mutex::new(Cached {
        snapshot: if preview.is_some() {
            preview_state()
        } else {
            "{\"devices\":[],\"name\":\"Fairphone 6\",\"event\":\"Connecting to service…\"}".into()
        },
        ..Default::default()
    }));
    let (tx, rx) = sync_channel(16);
    if preview.is_none() {
        let cached = cached.clone();
        std::thread::spawn(move || worker(cached, rx, agent));
    }
    let bridge = QObjectBox::new(Bridge {
        cached,
        sender: if preview.is_none() { Some(tx) } else { None },
        ..Default::default()
    });
    bridge.pinned().borrow_mut().poll();
    let mut engine = QmlEngine::new();
    engine.set_object_property("connectBridge".into(), bridge.pinned());
    engine.set_property("previewPath".into(), QString::from(preview_path).into());
    engine.set_property("previewPage".into(), QString::from(mode).into());
    engine.set_property("initialShare".into(), QString::from(share).into());
    engine.set_property("previewMode".into(), preview.is_some().into());
    let file = if agent {
        "Clipboard.qml"
    } else {
        "Launcher.qml"
    };
    if let Ok(dir) = std::env::var("KDECONNECT_QML_DIR") {
        engine.load_file(format!("{dir}/{file}").into());
    } else {
        engine.load_data_as(
            if agent {
                include_str!("../qml/Clipboard.qml")
            } else {
                include_str!("../qml/Launcher.qml")
            }
            .into(),
            QUrl::from(QString::from(format!(
                "file://{}/qml/{file}",
                env!("CARGO_MANIFEST_DIR")
            ))),
        );
    }
    engine.exec();
}
