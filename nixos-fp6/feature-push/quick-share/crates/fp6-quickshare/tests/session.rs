//! Exercise the real daemon over a private session bus and encrypted loopback TCP.
use fp6_quickshare::{QuickShareProxy, Transfer};
use futures_util::StreamExt;
use quickshare_core::{ChannelAction, Config, Core, Device, OutboundPayload, State};
use std::{
    process::{Child, Command, Stdio},
    time::Duration,
};
struct Daemon(Child);
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn session_e2e() {
    if std::env::var_os("FP6_PRIVATE_TEST_BUS").is_none() {
        let status = Command::new("dbus-run-session")
            .arg(format!(
                "--config-file={}/../../tests/session-bus.conf",
                env!("CARGO_MANIFEST_DIR")
            ))
            .arg("--")
            .arg(std::env::current_exe().unwrap())
            .args(["--exact", "session_e2e", "--nocapture"])
            .env("FP6_PRIVATE_TEST_BUS", "1")
            .status()
            .expect("dbus-run-session must be installed");
        assert!(status.success());
        return;
    }
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut daemon = Daemon(
            Command::new(env!("CARGO_BIN_EXE_fp6-quickshare-daemon"))
                .args([
                    "--interfaces",
                    "lo",
                    "--port",
                    &port.to_string(),
                    "--download-dir",
                    tmp.path().join("received").to_str().unwrap(),
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
        );
        let connection = zbus::Connection::session().await.unwrap();
        let bus = zbus::fdo::DBusProxy::new(&connection).await.unwrap();
        for _ in 0..200 {
            if bus
                .name_has_owner(fp6_quickshare::BUS.try_into().unwrap())
                .await
                .unwrap()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let proxy = QuickShareProxy::new(&connection).await.unwrap();
        assert_eq!(proxy.visibility().await.unwrap(), "off");
        assert!(proxy.accept("nonexistent").await.is_err());
        proxy.set_visibility("everyone-for-10-min").await.unwrap();
        let sender = Core::start(Config {
            name: "Synthetic sender".into(),
            directory: tmp.path().join("sender"),
            port: 0,
            interfaces: vec!["lo".into()],
            max_bytes: 1024 * 1024,
        })
        .await
        .unwrap();
        let device = Device {
            id: "synthetic".into(),
            name: "Synthetic receiver".into(),
            address: format!("127.0.0.1:{port}"),
        };
        let file = tmp.path().join("fixture.bin");
        let data = vec![42u8; 600_000];
        std::fs::write(&file, &data).unwrap();
        let mut incoming = proxy.receive_incoming_request().await.unwrap();
        let mut finished = proxy.receive_finished().await.unwrap();
        sender
            .send(
                &device,
                OutboundPayload::Files(vec![file.to_str().unwrap().into()]),
                "Synthetic sender".into(),
            )
            .await
            .unwrap();
        let signal = tokio::time::timeout(Duration::from_secs(10), incoming.next())
            .await
            .unwrap()
            .unwrap();
        let request = signal.args().unwrap();
        let id = request.transfer_id.to_owned();
        assert_eq!(request.sender, "Synthetic sender");
        assert_eq!(request.pin.len(), 4);
        assert!(!tmp.path().join("received/fixture.bin").exists());
        proxy.accept(&id).await.unwrap();
        assert!(proxy.accept(&id).await.is_err());
        let signal = tokio::time::timeout(Duration::from_secs(10), finished.next())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(signal.args().unwrap().status, "finished");
        assert_eq!(
            std::fs::read(tmp.path().join("received/fixture.bin")).unwrap(),
            data
        );
        let transfers: Vec<Transfer> =
            serde_json::from_str(&proxy.list_transfers().await.unwrap()).unwrap();
        assert_eq!(
            transfers.iter().find(|t| t.id == id).unwrap().bytes,
            data.len() as u64
        );
        assert!(proxy.cancel(&id).await.is_err());
        // A second incoming request is explicitly rejected through D-Bus.
        sender
            .send(
                &device,
                OutboundPayload::Text("synthetic text".into()),
                "Synthetic sender".into(),
            )
            .await
            .unwrap();
        let signal = tokio::time::timeout(Duration::from_secs(10), incoming.next())
            .await
            .unwrap()
            .unwrap();
        let request = signal.args().unwrap();
        proxy.reject(request.transfer_id).await.unwrap();
        let signal = tokio::time::timeout(Duration::from_secs(10), finished.next())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(signal.args().unwrap().status, "rejected");
        // Visibility off rejects new connections; a request cannot be approved twice.
        proxy.set_visibility("off").await.unwrap();
        assert!(proxy.accept(request.transfer_id).await.is_err());
        let _ = sender.action("nonexistent", ChannelAction::CancelTransfer);
        assert!(quickshare_core::terminal(&State::Finished));
        sender.stop().await;
        daemon.0.kill().unwrap();
        daemon.0.wait().unwrap();
    });
}
