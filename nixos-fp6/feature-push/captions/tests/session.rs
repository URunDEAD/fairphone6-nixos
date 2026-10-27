//! Private D-Bus only: never accesses the host bus, sound server or phone.
use fp6_captions::{
    service::{Options, Snapshot},
    BUS, PATH,
};
use std::{
    process::{Command, Stdio},
    time::Duration,
};
struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
fn private_bus_fail_closed_and_restart() {
    if std::env::var_os("CAPTIONS_PRIVATE_TEST").is_none() {
        let result = Command::new("dbus-run-session")
            .arg("--config-file")
            .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/bus.conf"))
            .arg("--")
            .arg(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "private_bus_fail_closed_and_restart",
                "--nocapture",
            ])
            .env("CAPTIONS_PRIVATE_TEST", "1")
            .env(
                "DBUS_SYSTEM_BUS_ADDRESS",
                "unix:path=/nonexistent/captions-system-bus",
            )
            .status()
            .expect("dbus-run-session required (use nix-shell)");
        assert!(result.success());
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let binary = env!("CARGO_BIN_EXE_fp6-captions");
    let daemon = || {
        Child(
            Command::new(binary)
                .arg("daemon")
                .env("XDG_DATA_HOME", dir.path())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    };
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let d = daemon();
        let c = zbus::Connection::session().await.unwrap();
        let p = zbus::Proxy::new(&c, BUS, PATH, BUS).await.unwrap();
        for n in 0..100 {
            if p.call::<_, _, String>("Snapshot", &()).await.is_ok() {
                break;
            }
            assert!(n < 99);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let bad = p.call::<_, _, ()>("Start", &("{}",)).await;
        assert!(bad.is_err());
        let _: () = p
            .call(
                "Start",
                &(serde_json::to_string(&Options::default()).unwrap(),),
            )
            .await
            .unwrap();
        let mut stopped = false;
        for _ in 0..150 {
            let json: String = p.call("Snapshot", &()).await.unwrap();
            let s: Snapshot = serde_json::from_str(&json).unwrap();
            assert!(!s.capture_active);
            assert!(s.text.is_empty());
            if s.state == "idle" {
                assert!(s.message.contains("Call status"));
                stopped = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(stopped);
        let o = Options {
            allow_calls: true,
            ..Default::default()
        };
        let _: () = p
            .call("Start", &(serde_json::to_string(&o).unwrap(),))
            .await
            .unwrap();
        for n in 0..100 {
            let json: String = p.call("Snapshot", &()).await.unwrap();
            let s: Snapshot = serde_json::from_str(&json).unwrap();
            assert!(!s.capture_active);
            if s.state == "idle" {
                assert!(s.message.contains("Model missing"));
                break;
            }
            assert!(n < 99);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let _: () = p.call("Stop", &()).await.unwrap();
        drop(d);
        let _d = daemon();
        for n in 0..100 {
            if let Ok(json) = p.call::<_, _, String>("Snapshot", &()).await {
                let s: Snapshot = serde_json::from_str(&json).unwrap();
                assert!(!s.capture_active);
                assert!(s.text.is_empty());
                break;
            }
            assert!(n < 99);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });
}
