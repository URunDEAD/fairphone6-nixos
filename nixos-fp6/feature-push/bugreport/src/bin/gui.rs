use fp6_bugreport::{bundle, config::Options};
use qmetaobject::prelude::*;
use std::sync::mpsc::{self, Receiver};

#[derive(QObject, Default)]
struct Backend {
    base: qt_base_class!(trait QObject),
    busy: qt_property!(bool; NOTIFY busy_changed),
    busy_changed: qt_signal!(),
    message: qt_property!(QString; NOTIFY message_changed),
    message_changed: qt_signal!(),
    result: Option<Receiver<Result<String, String>>>,
    preview: bool,
    start: qt_method!(
        fn start(&mut self, since: QString, sections: QString, privileged: bool, screenshot: bool) {
            if self.busy || self.preview {
                return;
            }
            let options = Options {
                since: since.to_string(),
                sections: sections
                    .to_string()
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect(),
                privileged,
                screenshot,
            };
            if let Err(e) = options.validate() {
                self.message = e.into();
                self.message_changed();
                return;
            }
            self.busy = true;
            self.busy_changed();
            self.message = "Collecting locally… Keep this window open.".into();
            self.message_changed();
            let (tx, rx) = mpsc::channel();
            self.result = Some(rx);
            std::thread::spawn(move || {
                let _ = tx.send(bundle::create(&options).map(|p| p.display().to_string()));
            });
        }
    ),
    poll: qt_method!(
        fn poll(&mut self) {
            let result = self.result.as_ref().map(|rx| rx.try_recv());
            let message = match result {
                Some(Ok(Ok(path))) => Some(format!(
                    "Saved: {path}\nRead summary.txt and review the bundle before sharing."
                )),
                Some(Ok(Err(e))) => Some(format!("Could not create report: {e}")),
                Some(Err(mpsc::TryRecvError::Disconnected)) => {
                    Some("Collection stopped unexpectedly. No report was published.".into())
                }
                _ => None,
            };
            if let Some(message) = message {
                self.result = None;
                self.busy = false;
                self.busy_changed();
                self.message = message.into();
                self.message_changed();
            }
        }
    ),
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let preview_path = args
        .iter()
        .position(|a| a == "--preview")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_default();
    let details = args.iter().any(|a| a == "--preview-details");
    let capture = args.iter().any(|a| a == "--preview-capture");
    let backend = QObjectBox::new(Backend {
        preview: !preview_path.is_empty(),
        ..Default::default()
    });
    let mut engine = QmlEngine::new();
    engine.set_object_property("backend".into(), backend.pinned());
    engine.set_property("previewPath".into(), QString::from(preview_path).into());
    engine.set_property("previewDetails".into(), details.into());
    engine.set_property("previewCapture".into(), capture.into());
    engine.load_file(
        option_env!("FP6_QML_PATH")
            .unwrap_or(concat!(env!("CARGO_MANIFEST_DIR"), "/qml/Main.qml"))
            .into(),
    );
    engine.exec();
}
