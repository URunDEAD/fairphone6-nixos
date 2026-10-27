// qmetaobject generates the Qt FFI boundary; handwritten Rust uses safe APIs.
use fp6_share::{
    model::{Request, Target},
    service::Snapshot,
    BUS, PATH,
};
use qmetaobject::prelude::*;
use std::collections::VecDeque;
use std::sync::{
    mpsc::{sync_channel, Receiver, SyncSender},
    Arc, Mutex,
};
use std::time::Duration;

#[derive(Default)]
struct Cache {
    snapshot: String,
    events: VecDeque<String>,
    error: String,
}
struct Command {
    method: String,
    revision: u64,
    data: String,
    option: String,
}
#[derive(QObject, Default)]
struct Bridge {
    base: qt_base_class!(trait QObject),
    snapshot: qt_property!(QString; NOTIFY changed),
    event: qt_property!(QString; NOTIFY changed),
    error: qt_property!(QString; NOTIFY changed),
    changed: qt_signal!(),
    poll: qt_method!(fn(&mut self)),
    command:
        qt_method!(fn(&mut self, method: QString, revision: u64, data: QString, option: QString)),
    cached: Arc<Mutex<Cache>>,
    sender: Option<SyncSender<Command>>,
}
impl Bridge {
    fn poll(&mut self) {
        {
            let mut cache = self.cached.lock().unwrap();
            self.snapshot = cache.snapshot.clone().into();
            self.event = cache.events.pop_front().unwrap_or_default().into();
            self.error = cache.error.clone().into();
        }
        self.changed();
    }
    fn command(&mut self, method: QString, revision: u64, data: QString, option: QString) {
        if let Some(sender) = &self.sender {
            if sender
                .try_send(Command {
                    method: method.to_string(),
                    revision,
                    data: data.to_string(),
                    option: option.to_string(),
                })
                .is_err()
            {
                self.error = "The service is busy. Try again.".into();
                self.changed();
            }
        }
    }
}
fn worker(cache: Arc<Mutex<Cache>>, rx: Receiver<Command>, initial: Option<Request>) {
    let mut initial = initial;
    loop {
        let result: anyhow::Result<()> = (|| {
            let bus = zbus::blocking::connection::Builder::session()?
                .method_timeout(Duration::from_secs(320))
                .build()?;
            let p = zbus::blocking::Proxy::new(&bus, BUS, PATH, BUS)?;
            if let Err(error) = p.call::<_, _, ()>("Attach", &()) {
                if error.to_string().contains("already open") {
                    cache
                        .lock()
                        .unwrap()
                        .events
                        .push_back(serde_json::json!({"kind": "duplicate"}).to_string());
                    return Ok(());
                }
                return Err(error.into());
            }

            if let Some(r) = initial.take() {
                p.call::<_, _, ()>("Share", &(r.items, r.mime, r.source_app))?;
            }
            loop {
                match rx.recv_timeout(Duration::from_millis(400)) {
                    Ok(c) => {
                        let result: anyhow::Result<Option<String>> = (|| match c.method.as_str() {
                            "Select" => {
                                Ok(Some(p.call("Select", &(c.revision, c.data, c.option))?))
                            }
                            "Share" => {
                                let r: Request = serde_json::from_str(&c.data)?;
                                p.call::<_, _, ()>("Share", &(r.items, r.mime, r.source_app))?;
                                Ok(None)
                            }
                            "Cancel" => {
                                p.call::<_, _, ()>("Cancel", &(c.revision,))?;
                                Ok(None)
                            }
                            "ClearHistory" => {
                                p.call::<_, _, ()>("ClearHistory", &())?;
                                Ok(None)
                            }
                            "QuickJob" => {
                                let status: String = p.call("QuickJob", &(c.data,))?;
                                Ok(Some(serde_json::json!({"kind":"quick_status", "value":serde_json::from_str::<serde_json::Value>(&status)?}).to_string()))
                            }
                            "CancelJob" => {
                                p.call::<_, _, ()>("CancelJob", &(c.option, c.data))?;
                                Ok(None)
                            }
                            "OcrJob" => {
                                let status: String = p.call("OcrJob", &(c.data,))?;
                                Ok(Some(serde_json::json!({"kind":"ocr_status", "value":serde_json::from_str::<serde_json::Value>(&status)?}).to_string()))
                            }
                            _ => Ok(None),
                        })();
                        let mut cache = cache.lock().unwrap();
                        match result {
                            Ok(event) => {
                                cache.error.clear();
                                if let Some(event) = event {
                                    cache.events.push_back(event);
                                }
                            }
                            Err(e) => cache.error = e.to_string(),
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                    Err(_) => {}
                }
                let snapshot: String = p.call("Snapshot", &())?;
                cache.lock().unwrap().snapshot = snapshot;
            }
        })();
        if let Err(error) = result {
            cache.lock().unwrap().error = format!("Service unavailable: {error}");
        } else {
            break;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}
fn preview(mode: &str) -> Snapshot {
    let image = mode != "text";
    let mut targets = vec![
        Target::new(
            "quick",
            "synthetic-laptop",
            "Studio laptop",
            "Nearby · Quick Share",
            true,
        ),
        Target::new(
            "kde",
            "synthetic-tablet",
            "Living room tablet",
            "Paired · KDE Connect",
            true,
        ),
        Target::new(
            "content",
            "synthetic-messages",
            "Messages",
            "App · Content Hub",
            false,
        ),
        Target::new(
            "content",
            "synthetic-notes",
            "Notes",
            "App · Content Hub",
            false,
        ),
        Target::new(
            "web",
            "link",
            "Share via link / QR",
            "Choose Wi-Fi and expiry in WebShare",
            false,
        ),
        Target::new(
            "action",
            "copy",
            "Copy",
            "Copy text, links or file URIs",
            false,
        ),
        Target::new(
            "action",
            "save",
            "Save to Files",
            "Choose a destination",
            false,
        ),
    ];
    if image {
        targets.push(Target::new(
            "action",
            "ocr",
            "Text recognition",
            "Read text from this image · on device",
            false,
        ));
    }
    if mode == "empty" {
        targets.retain(|t| t.id == "action:copy");
    }
    Snapshot {
        revision: 1,
        content_type: if image {
            "pictures".into()
        } else {
            "text".into()
        },
        request: Some(Request {
            items: if image {
                vec!["file:///synthetic/Weekend%20garden.png".into()]
            } else {
                vec!["A little invitation: meet at the garden on Saturday at 10:00.".into()]
            },
            mime: if image {
                "image/png".into()
            } else {
                "text/plain".into()
            },
            source_app: if image {
                "Gallery".into()
            } else {
                "Notes".into()
            },
        }),
        recent: targets.iter().filter(|t| t.direct).cloned().collect(),
        targets,
        busy: false,
        status: "Choose where to share".into(),
    }
}
fn main() {
    if std::env::var_os("APP_ID").is_none() {
        std::env::set_var("APP_ID", "com.fairphone6.Share");
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let preview_index = args.iter().position(|a| a == "--preview");
    let preview_mode = preview_index.is_some();
    let mode = preview_index
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
        .unwrap_or("image");
    let preview_path = args
        .iter()
        .position(|a| a == "--screenshot")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_default();
    let initial = if !preview_mode && !args.is_empty() {
        match Request::from_cli(&args) {
            Ok(r) => Some(r),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(2);
            }
        }
    } else {
        None
    };
    let cached = Arc::new(Mutex::new(Cache {
        snapshot: serde_json::to_string(&if preview_mode {
            preview(mode)
        } else {
            Snapshot::default()
        })
        .unwrap(),
        error: if mode == "error" {
            "The selected device went offline. Choose another target.".into()
        } else {
            String::new()
        },
        ..Default::default()
    }));
    let (tx, rx) = sync_channel(8);
    if !preview_mode {
        let cached = cached.clone();
        std::thread::spawn(move || worker(cached, rx, initial));
    }
    let bridge = QObjectBox::new(Bridge {
        cached,
        sender: if preview_mode { None } else { Some(tx) },
        ..Default::default()
    });
    bridge.pinned().borrow_mut().poll();
    let mut engine = QmlEngine::new();
    engine.set_object_property("shareBridge".into(), bridge.pinned());
    engine.set_property("previewMode".into(), preview_mode.into());
    engine.set_property("previewPath".into(), QString::from(preview_path).into());
    let dir = std::env::var("FP6_SHARE_QML_DIR")
        .unwrap_or_else(|_| format!("{}/qml", env!("CARGO_MANIFEST_DIR")));
    engine.load_file(format!("{dir}/Launcher.qml").into());
    engine.exec();
}
