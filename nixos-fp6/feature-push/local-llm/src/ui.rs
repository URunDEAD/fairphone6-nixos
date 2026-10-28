use crate::{BUS, OBJECT};
use qmetaobject::prelude::*;
use std::{
    ffi::CStr,
    sync::mpsc::{self, Receiver, Sender},
    time::Duration,
};

enum Update {
    Snapshot(String),
    Message(String),
}
#[derive(Default, QObject)]
pub struct Backend {
    base: qt_base_class!(trait QObject),
    snapshot: qt_property!(QString; NOTIFY changed),
    message: qt_property!(QString; NOTIFY changed),
    preview: qt_property!(bool),
    changed: qt_signal!(),
    sender: Option<Sender<(String, String)>>,
    receiver: Option<Receiver<Update>>,
    poll: qt_method!(
        fn poll(&mut self) {
            if self.preview {
                return;
            }
            if self.receiver.is_none() {
                let (commands_tx, commands_rx) = mpsc::channel();
                let (updates_tx, updates_rx) = mpsc::channel();
                self.sender = Some(commands_tx);
                self.receiver = Some(updates_rx);
                std::thread::spawn(move || worker(commands_rx, updates_tx));
            }
            if let Some(rx) = &self.receiver {
                while let Ok(update) = rx.try_recv() {
                    match update {
                        Update::Snapshot(s) => self.snapshot = s.into(),
                        Update::Message(s) => self.message = s.into(),
                    }
                    self.changed();
                }
            }
        }
    ),
    act: qt_method!(
        fn act(&mut self, verb: QString, argument: QString) {
            if self.preview {
                return;
            }
            self.poll();
            if let Some(tx) = &self.sender {
                let _ = tx.send((verb.to_string(), argument.to_string()));
            }
        }
    ),
}
fn worker(commands: Receiver<(String, String)>, updates: Sender<Update>) {
    loop {
        let operation = match commands.recv_timeout(Duration::from_secs(1)) {
            Ok(v) => Some(v),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(_) => return,
        };
        let result = (|| -> anyhow::Result<String> {
            let connection = zbus::blocking::Connection::session()?;
            let proxy = zbus::blocking::Proxy::new(&connection, BUS, OBJECT, BUS)?;
            if let Some((verb, argument)) = operation {
                match verb.as_str() {
                    "Download" | "Delete" | "SetActive" => {
                        proxy.call::<_, _, ()>(verb.as_str(), &(argument,))?
                    }
                    "Unload" => proxy.call::<_, _, ()>("Unload", &())?,
                    "SetContext" => {
                        proxy.call::<_, _, ()>("SetContextSize", &(argument.parse::<u32>()?,))?
                    }
                    "Configure" => {
                        let v: crate::Settings = serde_json::from_str(&argument)?;
                        proxy.call::<_, _, ()>(
                            "Configure",
                            &(v.idle_seconds, v.use_gpu, v.battery_saver),
                        )?;
                    }
                    _ => anyhow::bail!("unknown settings operation"),
                }
                let _ = updates.send(Update::Message(String::new()));
            }
            let status: String = proxy.call("Status", &())?;
            let models: String = proxy.call("ListModels", &())?;
            Ok(format!("{{\"status\":{status},\"models\":{models}}}"))
        })();
        let update = match result {
            Ok(s) => Update::Snapshot(s),
            Err(e) => Update::Message(format!("{e:#}")),
        };
        if updates.send(update).is_err() {
            return;
        }
    }
}
#[derive(Default, QObject)]
struct LocalLlmPlugin {
    base: qt_base_class!(trait QQmlExtensionPlugin),
    plugin: qt_plugin!("org.qt-project.Qt.QQmlExtensionInterface/1.0"),
}
impl QQmlExtensionPlugin for LocalLlmPlugin {
    fn register_types(&mut self, uri: &CStr) {
        qml_register_type::<Backend>(uri, 1, 0, cstr::cstr!(LocalLlmBackend));
    }
}
