use fp6_selftest::report::Report;
use qmetaobject::prelude::*;
use std::{
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver},
};
#[derive(QObject, Default)]
struct Backend {
    base: qt_base_class!(trait QObject),
    busy: qt_property!(bool; NOTIFY changed),
    report_json: qt_property!(QString; NOTIFY changed),
    message: qt_property!(QString; NOTIFY changed),
    changed: qt_signal!(),
    pending: Option<Receiver<Result<Report, ()>>>,
    preview: bool,
    start: qt_method!(
        fn start(&mut self) {
            if self.busy || self.preview {
                return;
            }
            self.busy = true;
            self.message = "Checking this phone…".into();
            self.changed();
            let (tx, rx) = mpsc::channel();
            self.pending = Some(rx);
            std::thread::spawn(move || {
                let exe = std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|p| p.join("fp6-selftest")));
                let result = exe
                    .ok_or(())
                    .and_then(|exe| {
                        Command::new(exe)
                            .arg("--json")
                            .stdin(Stdio::null())
                            .stderr(Stdio::null())
                            .output()
                            .map_err(|_| ())
                    })
                    .and_then(|o| serde_json::from_slice::<Report>(&o.stdout).map_err(|_| ()));
                let _ = tx.send(result);
            });
        }
    ),
    poll: qt_method!(
        fn poll(&mut self) {
            let result = self.pending.as_ref().map(|r| r.try_recv());
            match result {
                Some(Ok(result)) => {
                    self.busy = false;
                    self.pending = None;
                    match result {
                        Ok(report) => {
                            self.message = if report.checks.iter().any(|c| c.id == "report-save") {
                                "Checks finished. The report could not be saved."
                            } else {
                                "Saved in Documents / Self-tests"
                            }
                            .into();
                            self.report_json = report.json().into();
                        }
                        Err(_) => {
                            self.message =
                                "Self-test could not complete. Try again from the terminal.".into();
                        }
                    }
                    self.changed();
                }
                Some(Err(mpsc::TryRecvError::Disconnected)) => {
                    self.busy = false;
                    self.pending = None;
                    self.message = "Self-test stopped unexpectedly.".into();
                    self.changed();
                }
                _ => {}
            }
        }
    ),
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let preview_path = args
        .iter()
        .position(|a| a == "--preview")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_default();
    let preview = !preview_path.is_empty();
    let fixture = if preview && args.iter().any(|a| a == "--results") {
        include_str!("../../tests/fixtures/preview.json")
    } else {
        ""
    };
    let backend = QObjectBox::new(Backend {
        preview,
        report_json: fixture.into(),
        message: if fixture.is_empty() {
            ""
        } else {
            "Synthetic preview · no phone data"
        }
        .into(),
        ..Default::default()
    });
    let mut engine = QmlEngine::new();
    engine.set_object_property("backend".into(), backend.pinned());
    engine.set_property(
        "autoRun".into(),
        (!preview && args.iter().any(|a| a == "fp6-selftest:run")).into(),
    );
    engine.set_property("previewPath".into(), QString::from(preview_path).into());
    engine.load_file(
        std::env::var("FP6_SELFTEST_QML")
            .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/qml/Main.qml").into())
            .into(),
    );
    engine.exec();
}
