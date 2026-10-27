//! These tests always create a private bus, including under plain `cargo test`.
use fp6_callkit::{
    client::{pending, CallsProxy},
    model::*,
    service::{self, NOTIFY, PLATFORM, SHELL},
};
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};
use zbus::Connection;
struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct Fixture {
    _service: Process,
    _bus: Process,
    address: String,
}
impl Fixture {
    async fn new() -> Self {
        Self::with_mode(true).await
    }
    async fn with_mode(demo: bool) -> Self {
        let mut bus = Command::new("dbus-daemon")
            .arg(concat!(
                "--config-file=",
                env!("CARGO_MANIFEST_DIR"),
                "/tests/session.conf"
            ))
            .args(["--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .spawn()
            .expect("dbus-daemon required (use nix-shell)");
        let mut address = String::new();
        BufReader::new(bus.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        let address = address.trim().to_string();
        let mut command = Command::new(env!("CARGO_BIN_EXE_fp6-callsd"));
        if demo {
            command.arg("--demo");
        }
        let service = command
            .env("DBUS_SYSTEM_BUS_ADDRESS", &address)
            .env("DBUS_SESSION_BUS_ADDRESS", &address)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let f = Self {
            _service: Process(service),
            _bus: Process(bus),
            address,
        };
        let conn = f.connect().await;
        for _ in 0..100 {
            if CallsProxy::new(&conn).await.unwrap().calls().await.is_ok() {
                return f;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("broker did not start");
    }
    async fn connect(&self) -> Connection {
        zbus::connection::Builder::address(self.address.as_str())
            .unwrap()
            .build()
            .await
            .unwrap()
    }
}
struct Notify {
    decision: Arc<Mutex<Alert>>,
    delay: bool,
}
#[zbus::interface(name = "com.fairphone6.Notifications1")]
impl Notify {
    async fn should_alert(&self, category: &str) -> (bool, bool) {
        assert_eq!(category, "internet-call");
        if self.delay {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        let a = *self.decision.lock().unwrap();
        (a.sound, a.vibration)
    }
}
struct Platform(Arc<Mutex<Effects>>);
#[zbus::interface(name = "com.fairphone6.CallsPlatform1")]
impl Platform {
    fn apply(&self, effects: &str, lease_ms: u32) {
        assert_eq!(lease_ms, 2500);
        *self.0.lock().unwrap() = serde_json::from_str(effects).unwrap();
    }
}
#[tokio::test]
async fn actual_bus_ownership_controls_effects_history_and_disconnect() {
    let f = Fixture::new().await;
    let app = f.connect().await;
    let other = f.connect().await;
    let shell = f.connect().await;
    shell.request_name(SHELL).await.unwrap();
    let platform = f.connect().await;
    platform.request_name(PLATFORM).await.unwrap();
    let applied = Arc::new(Mutex::new(Effects::default()));
    platform
        .object_server()
        .at("/com/fairphone6/CallsPlatform1", Platform(applied.clone()))
        .await
        .unwrap();
    let notify = f.connect().await;
    notify.request_name(NOTIFY).await.unwrap();
    let decision = Arc::new(Mutex::new(Alert {
        sound: true,
        vibration: true,
    }));
    notify
        .object_server()
        .at(
            "/com/fairphone6/Notifications1",
            Notify {
                decision: decision.clone(),
                delay: false,
            },
        )
        .await
        .unwrap();
    let provider = CallsProxy::new(&app).await.unwrap();
    let stranger = CallsProxy::new(&other).await.unwrap();
    let ui = CallsProxy::new(&shell).await.unwrap();
    let id = provider
        .report_incoming("org.example.Test", "Synthetic private name")
        .await
        .unwrap();
    assert!(stranger.report_ended(id).await.is_err());
    assert!(stranger.commands(id).await.is_err());
    assert!(stranger.snapshot().await.is_err());
    assert!(stranger.history().await.is_err());
    assert!(stranger.set_proximity(true).await.is_err());
    assert!(stranger.control(id, r#"{"kind":"answer"}"#).await.is_err());
    assert_eq!(stranger.calls().await.unwrap(), "[]");
    for _ in 0..100 {
        if applied.lock().unwrap().ringing {
            break;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    assert!(applied.lock().unwrap().ringing);
    *decision.lock().unwrap() = Alert::default();
    for _ in 0..100 {
        if !applied.lock().unwrap().ringing {
            break;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    assert!(!applied.lock().unwrap().ringing);
    let serial = ui.control(id, r#"{"kind":"answer"}"#).await.unwrap();
    assert_eq!(
        pending(&provider, id).await.unwrap().unwrap().serial,
        serial
    );
    assert!(stranger.complete(id, serial, true).await.is_err());
    provider.complete(id, serial, true).await.unwrap();
    ui.set_proximity(true).await.unwrap();
    for _ in 0..100 {
        if applied.lock().unwrap().screen_off {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(applied.lock().unwrap().screen_off);
    let snapshot = ui.snapshot().await.unwrap();
    assert!(!snapshot.contains("Synthetic private name"));
    assert!(
        serde_json::from_str::<Snapshot>(&snapshot)
            .unwrap()
            .platform_ready
    );
    provider.report_ended(id).await.unwrap();
    provider.report_ended(id).await.unwrap();
    let history: Vec<History> = serde_json::from_str(&ui.history().await.unwrap()).unwrap();
    assert_eq!(history.len(), 1);
    ui.clear_history().await.unwrap();
    assert_eq!(ui.history().await.unwrap(), "[]");
    let id = provider
        .report_incoming("org.example.Test", "Synthetic")
        .await
        .unwrap();
    drop(provider);
    app.close().await.unwrap();
    for _ in 0..100 {
        let snapshot: Snapshot = serde_json::from_str(&ui.snapshot().await.unwrap()).unwrap();
        if snapshot
            .calls
            .iter()
            .any(|c| c.id == id && c.state == State::Ended)
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    let snapshot: Snapshot = serde_json::from_str(&ui.snapshot().await.unwrap()).unwrap();
    assert_eq!(
        snapshot
            .calls
            .iter()
            .find(|c| c.id == id)
            .unwrap()
            .reason
            .as_deref(),
        Some("provider_disconnected")
    );
}
#[tokio::test]
async fn dnd_unavailable_and_timeout_are_silent() {
    let f = Fixture::new().await;
    let conn = f.connect().await;
    assert_eq!(service::should_alert(&conn).await, Alert::default());
    conn.request_name(NOTIFY).await.unwrap();
    conn.object_server()
        .at(
            "/com/fairphone6/Notifications1",
            Notify {
                decision: Arc::new(Mutex::new(Alert {
                    sound: true,
                    vibration: true,
                })),
                delay: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(service::should_alert(&conn).await, Alert::default());
    assert!(!service::apply_effects(&conn, &Effects::default()).await);
}

type Props = std::collections::HashMap<String, zbus::zvariant::OwnedValue>;
type Objects = Vec<(zbus::zvariant::OwnedObjectPath, Props)>;
struct OfonoManager;
#[zbus::interface(name = "org.ofono.Manager")]
impl OfonoManager {
    fn get_modems(&self) -> Objects {
        let mut props = Props::new();
        props.insert(
            "Interfaces".into(),
            zbus::zvariant::Value::from(vec!["org.ofono.VoiceCallManager"])
                .try_to_owned()
                .unwrap(),
        );
        vec![("/modem0".try_into().unwrap(), props)]
    }
}
struct VoiceCalls;
#[zbus::interface(name = "org.ofono.VoiceCallManager")]
impl VoiceCalls {
    fn get_calls(&self) -> Objects {
        vec![]
    }
}
#[tokio::test]
async fn ofono_signals_preempt_voip_and_loss_is_unknown() {
    // Both session and system addresses point at our private synthetic bus.
    let f = Fixture::with_mode(false).await;
    let ofono = f.connect().await;
    ofono.object_server().at("/", OfonoManager).await.unwrap();
    ofono
        .object_server()
        .at("/modem0", VoiceCalls)
        .await
        .unwrap();
    ofono.request_name("org.ofono").await.unwrap();
    let shell = f.connect().await;
    shell.request_name(SHELL).await.unwrap();
    let ui = CallsProxy::new(&shell).await.unwrap();
    async fn await_cellular(ui: &CallsProxy<'_>, expected: Cellular) {
        for _ in 0..150 {
            let s: Snapshot = serde_json::from_str(&ui.snapshot().await.unwrap()).unwrap();
            if s.cellular == expected {
                return;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
        panic!("cellular observer did not reach {expected:?}");
    }
    await_cellular(&ui, Cellular::Idle).await;
    let app = f.connect().await;
    let provider = CallsProxy::new(&app).await.unwrap();
    let id = provider
        .report_outgoing("org.example.Synthetic", "Synthetic")
        .await
        .unwrap();
    provider.report_connected(id).await.unwrap();
    let mut props = Props::new();
    props.insert(
        "State".into(),
        zbus::zvariant::Value::from("incoming")
            .try_to_owned()
            .unwrap(),
    );
    let path: zbus::zvariant::OwnedObjectPath = "/modem0/call1".try_into().unwrap();
    ofono
        .emit_signal(
            None::<&str>,
            "/modem0",
            "org.ofono.VoiceCallManager",
            "CallAdded",
            &(path.clone(), props),
        )
        .await
        .unwrap();
    await_cellular(&ui, Cellular::Busy).await;
    let s: Snapshot = serde_json::from_str(&ui.snapshot().await.unwrap()).unwrap();
    assert_eq!(
        s.calls.iter().find(|c| c.id == id).unwrap().state,
        State::Ended
    );
    assert_eq!(
        pending(&provider, id).await.unwrap().unwrap().action,
        Action::End
    );
    ofono
        .emit_signal(
            None::<&str>,
            "/modem0/call1",
            "org.ofono.VoiceCall",
            "PropertyChanged",
            &("State", zbus::zvariant::Value::from("disconnected")),
        )
        .await
        .unwrap();
    await_cellular(&ui, Cellular::Idle).await;
    ofono
        .emit_signal(
            None::<&str>,
            "/modem0",
            "org.ofono.VoiceCallManager",
            "CallRemoved",
            &(path,),
        )
        .await
        .unwrap();
    ofono.close().await.unwrap();
    await_cellular(&ui, Cellular::Unknown).await;
    assert!(provider
        .report_outgoing("org.example.Synthetic", "Synthetic")
        .await
        .is_err());
}
