//! qmetaobject generates the necessary Qt/C++ FFI. No handwritten unsafe code.
use fp6_captions::{
    service::{Options, Snapshot},
    BUS, PATH,
};
use qmetaobject::prelude::*;
use std::sync::{mpsc, Arc, Mutex};
#[derive(QObject, Default)]
struct Backend {
    base: qt_base_class!(trait QObject),
    shared: Arc<Mutex<String>>,
    commands: Option<mpsc::SyncSender<(String, String)>>,
    snapshot: qt_method!(
        fn snapshot(&self) -> QString {
            self.shared.lock().unwrap().clone().into()
        }
    ),
    command: qt_method!(
        fn command(&self, action: String, options: String) -> QString {
            match self
                .commands
                .as_ref()
                .and_then(|tx| tx.try_send((action, options)).ok())
            {
                Some(()) => "".into(),
                None => "Service busy; try again".into(),
            }
        }
    ),
    save: qt_method!(
        fn save(&self) -> QString {
            let result = (|| -> anyhow::Result<String> {
                let s: Snapshot = serde_json::from_str(&self.shared.lock().unwrap())?;
                let path = fp6_captions::storage::save_visible(&s.text)?;
                Ok(format!("Saved visible text: {}", path.display()))
            })();
            result.unwrap_or_else(|e| e.to_string()).into()
        }
    ),
}
fn main() {
    let preview = std::env::args().any(|a| a.starts_with("--preview="));
    let shared = Arc::new(Mutex::new(
        serde_json::to_string(&Snapshot {
            state: "idle".into(),
            message: "Captions are off".into(),
            ..Default::default()
        })
        .unwrap(),
    ));
    let (tx, rx) = mpsc::sync_channel::<(String, String)>(4);
    if !preview {
        let state = shared.clone();
        std::thread::spawn(move || loop {
            let result = (|| -> anyhow::Result<()> {
                let conn = zbus::blocking::connection::Builder::session()?
                    .method_timeout(std::time::Duration::from_secs(3))
                    .build()?;
                let p = zbus::blocking::Proxy::new(&conn, BUS, PATH, BUS)?;
                loop {
                    match rx.recv_timeout(std::time::Duration::from_millis(200)) {
                        Ok((action, options)) => {
                            let r: Result<(), zbus::Error> = if action == "Start" {
                                let o: Options = serde_json::from_str(&options)?;
                                o.validate()?;
                                p.call("Start", &(options,))
                            } else {
                                p.call("Stop", &())
                            };
                            if let Err(e) = r {
                                *state.lock().unwrap() = serde_json::to_string(&Snapshot {
                                    state: "error".into(),
                                    message: e.to_string(),
                                    ..Default::default()
                                })
                                .unwrap();
                                continue;
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    let s: String = p.call("Snapshot", &())?;
                    *state.lock().unwrap() = s;
                }
            })();
            if result.is_ok() {
                break;
            }
            *state.lock().unwrap() = serde_json::to_string(&Snapshot {
                state: "error".into(),
                message: "Caption service unavailable; start the user service".into(),
                ..Default::default()
            })
            .unwrap();
            std::thread::sleep(std::time::Duration::from_secs(1));
        });
    }
    let backend = QObjectBox::new(Backend {
        shared,
        commands: Some(tx),
        ..Default::default()
    });
    let mut engine = QmlEngine::new();
    qmetaobject::qtcore::core_application::QCoreApplication::set_organization_name(
        "Fairphone6".into(),
    );
    qmetaobject::qtcore::core_application::QCoreApplication::set_application_name(
        "LiveCaptions".into(),
    );
    engine.set_object_property("backend".into(), backend.pinned());
    engine.load_data(include_str!("../../qml/Captions.qml").into());
    engine.exec();
}
