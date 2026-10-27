//! Mock oFono on an isolated bus: all calls (including held/ringing) block.
use futures_lite::StreamExt;
use std::{
    collections::HashMap,
    process::Command,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    },
};
type Objects = Vec<(
    zbus::zvariant::OwnedObjectPath,
    HashMap<String, zbus::zvariant::OwnedValue>,
)>;
struct Manager(Arc<AtomicU8>);
#[zbus::interface(name = "org.ofono.Manager")]
impl Manager {
    fn get_modems(&self) -> Objects {
        if self.0.load(Ordering::SeqCst) == 2 {
            vec![]
        } else {
            vec![("/modem0".try_into().unwrap(), HashMap::new())]
        }
    }
}
struct Calls(Arc<AtomicU8>);
#[zbus::interface(name = "org.ofono.VoiceCallManager")]
impl Calls {
    fn get_calls(&self) -> zbus::fdo::Result<Objects> {
        match self.0.load(Ordering::SeqCst) {
            0 => Ok(vec![]),
            3 => Err(zbus::fdo::Error::Failed("unknown".into())),
            _ => Ok(vec![("/modem0/call1".try_into().unwrap(), HashMap::new())]),
        }
    }
}
#[test]
fn ofono_gate_fails_closed() {
    if std::env::var_os("CAPTIONS_CALL_TEST").is_none() {
        assert!(Command::new("dbus-run-session")
            .arg("--config-file")
            .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/bus.conf"))
            .arg("--")
            .arg(std::env::current_exe().unwrap())
            .args(["--exact", "ofono_gate_fails_closed"])
            .env("CAPTIONS_CALL_TEST", "1")
            .status()
            .unwrap()
            .success());
        return;
    }
    std::env::set_var(
        "DBUS_SYSTEM_BUS_ADDRESS",
        std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap(),
    );
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        assert!(!fp6_captions::service::calls_clear().await);
        let mode = Arc::new(AtomicU8::new(0));
        let c = zbus::connection::Builder::session()
            .unwrap()
            .name("org.ofono")
            .unwrap()
            .serve_at("/", Manager(mode.clone()))
            .unwrap()
            .serve_at("/modem0", Calls(mode.clone()))
            .unwrap()
            .build()
            .await
            .unwrap();
        let mut events = fp6_captions::service::call_events().await.unwrap();
        c.emit_signal(
            None::<&str>,
            "/modem0",
            "org.ofono.VoiceCallManager",
            "CallAdded",
            &("/modem0/call1",),
        )
        .await
        .unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(1), events.next())
                .await
                .unwrap()
                .is_some()
        );
        assert!(fp6_captions::service::calls_clear().await);
        for m in [1, 2, 3] {
            mode.store(m, Ordering::SeqCst);
            assert!(!fp6_captions::service::calls_clear().await);
        }
        mode.store(0, Ordering::SeqCst);
        assert!(fp6_captions::service::calls_clear().await);
        c.release_name("org.ofono").await.unwrap();
        assert!(!fp6_captions::service::calls_clear().await);
    });
}
