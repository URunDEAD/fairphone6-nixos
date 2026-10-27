use qmetaobject::*;
use std::cell::RefCell;

#[derive(QObject, Default)]
struct Bridge {
    base: qt_base_class!(trait QObject),
    status: qt_property!(QString; NOTIFY changed),
    error: qt_property!(QString; NOTIFY changed),
    changed: qt_signal!(),
    refresh: qt_method!(
        fn refresh(&mut self) {
            match fp6_netpolicy::daemon::call("status", None) {
                Ok(text) => {
                    self.status = text.into();
                    self.error = "".into();
                }
                Err(error) => {
                    self.error = error.to_string().into();
                }
            }
            self.changed();
        }
    ),
    save: qt_method!(
        fn save(&mut self, json: QString) -> bool {
            match fp6_netpolicy::daemon::call("settings", Some(&json.to_string())) {
                Ok(_) => {
                    self.error = "".into();
                    self.changed();
                    true
                }
                Err(error) => {
                    self.error = error.to_string().into();
                    self.changed();
                    false
                }
            }
        }
    ),
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let preview = args
        .iter()
        .position(|a| a == "--preview")
        .and_then(|i| args.get(i + 1));
    let bridge = RefCell::new(Bridge::default());
    let mut engine = QmlEngine::new();
    engine.set_object_property("backend".into(), unsafe_pin(&bridge));
    engine.set_property(
        "previewFile".into(),
        QString::from(preview.cloned().unwrap_or_default()).into(),
    );
    engine.set_property(
        "previewPage".into(),
        QString::from(std::env::var("NETPOLICY_PREVIEW_PAGE").unwrap_or_default()).into(),
    );
    // The preview has no D-Bus connection and uses an empty, offline model.
    let qml = std::env::var("NETPOLICY_QML")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/qml/Main.qml").into());
    engine.load_file(qml.into());
    engine.exec();
}
// QObjectPinned::new is unsafe because moving a pinned QObject invalidates Qt's
// pointers. The RefCell lives in main until after QmlEngine is dropped.
fn unsafe_pin(value: &RefCell<Bridge>) -> QObjectPinned<'_, Bridge> {
    unsafe { QObjectPinned::new(value) }
}
