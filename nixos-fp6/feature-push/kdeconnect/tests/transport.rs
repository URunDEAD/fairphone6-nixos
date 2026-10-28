use anyhow::Result;
use lomiri_kdeconnect::{
    app::App,
    network,
    protocol::{read_packet, Packet, MAX_PACKET},
    store::Store,
    tls::Credentials,
};
use serde_json::json;
use std::{
    io::{BufRead, BufReader as StdReader},
    process::{Child, Command, Stdio},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
};
use tokio_openssl::SslStream;

struct Bus(Child, String);
impl Bus {
    fn new() -> Self {
        let mut child = Command::new("dbus-daemon")
            .args([
                "--config-file",
                concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/session.conf"),
                "--nofork",
                "--print-address=1",
                "--nopidfile",
            ])
            .stdout(Stdio::piped())
            .spawn()
            .expect("dbus-daemon is needed for isolated tests");
        let mut address = String::new();
        StdReader::new(child.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        Self(child, address.trim().into())
    }
    async fn connection(&self) -> zbus::Connection {
        zbus::connection::Builder::address(self.1.as_str())
            .unwrap()
            .build()
            .await
            .unwrap()
    }
}
impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
async fn raw_peer(
    app: Arc<App>,
    credentials: &Credentials,
    id: &str,
    version: u8,
) -> Result<(
    BufReader<SslStream<TcpStream>>,
    tokio::task::JoinHandle<Result<()>>,
)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let tcp = TcpStream::connect(listener.local_addr()?).await?;
    let handle = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await?;
        network::establish(app, tcp, None).await
    });
    let mut tcp = tcp;
    tcp.write_all(&Packet::identity(id, "Synthetic peer", version).encode()?)
        .await?;
    let mut tls = credentials.handshake(tcp, true).await?;
    if version == 8 {
        tls.write_all(&Packet::identity(id, "Synthetic peer", 8).encode()?)
            .await?;
    }
    let mut reader = BufReader::new(tls);
    if version == 8 {
        let p = read_packet(&mut reader, MAX_PACKET).await?;
        assert_eq!(p.kind, "kdeconnect.identity");
    }
    tokio::time::sleep(Duration::from_millis(40)).await;
    Ok((reader, handle))
}
async fn app(bus: &Bus, dir: &std::path::Path) -> Arc<App> {
    let store = Store::load(dir.into()).unwrap();
    let cert = Credentials::load(dir, &store.id).unwrap();
    App::new(
        store,
        cert,
        bus.connection().await,
        vec![],
        dir.join("downloads"),
    )
}
async fn accept_pair(app: &App, id: &str) -> Result<()> {
    let pin = app
        .inner
        .lock()
        .await
        .devices
        .get(id)
        .map(|d| d.fingerprint.clone())
        .unwrap_or_default();
    app.accept_pair(id, &pin).await
}
async fn request_pair(app: &App, id: &str) -> Result<()> {
    let pin = app
        .inner
        .lock()
        .await
        .devices
        .get(id)
        .map(|d| d.fingerprint.clone())
        .unwrap_or_default();
    app.request_pair(id, &pin).await
}
#[tokio::test]
async fn v7_v8_pairing_tls_and_plugin_boundary() -> Result<()> {
    let bus = Bus::new();
    for version in [7, 8] {
        let dir = tempfile::tempdir()?;
        let app = app(&bus, dir.path()).await;
        let creds = Credentials::generate("synthetic-desktop")?;
        let (mut peer, task) = raw_peer(app.clone(), &creds, "synthetic-desktop", version).await?;
        assert!(!app.allowed("synthetic-desktop", "battery").await);
        assert!(accept_pair(&app, "synthetic-desktop").await.is_err());
        peer.get_mut()
            .write_all(
                &Packet::new(
                    "kdeconnect.battery",
                    json!({"currentCharge":13,"isCharging":false}),
                )
                .encode()?,
            )
            .await?;
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(app.inner.lock().await.devices["synthetic-desktop"]
            .battery
            .is_none());
        peer.get_mut()
            .write_all(
                &Packet::new(
                    "kdeconnect.pair",
                    json!({"pair":true,"timestamp":lomiri_kdeconnect::protocol::now_millis()/1000}),
                )
                .encode()?,
            )
            .await?;
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(!app
            .inner
            .lock()
            .await
            .store
            .peers
            .contains_key("synthetic-desktop"));
        assert!(app
            .accept_pair("synthetic-desktop", "stale-displayed-fingerprint")
            .await
            .is_err());
        accept_pair(&app, "synthetic-desktop").await?;
        assert_eq!(read_packet(&mut peer, MAX_PACKET).await?.body["pair"], true);
        assert!(app.allowed("synthetic-desktop", "battery").await);
        assert!(!app.allowed("synthetic-desktop", "input").await);
        peer.get_mut()
            .write_all(
                &Packet::new(
                    "kdeconnect.battery",
                    json!({"currentCharge":42,"isCharging":true}),
                )
                .encode()?,
            )
            .await?;
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(
            app.inner.lock().await.devices["synthetic-desktop"]
                .battery
                .as_ref()
                .unwrap()["charge"],
            42
        );
        app.set_plugin("synthetic-desktop", "battery", false)
            .await?;
        peer.get_mut()
            .write_all(
                &Packet::new(
                    "kdeconnect.battery",
                    json!({"currentCharge":1,"isCharging":false}),
                )
                .encode()?,
            )
            .await?;
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(
            app.inner.lock().await.devices["synthetic-desktop"]
                .battery
                .as_ref()
                .unwrap()["charge"],
            42
        );
        app.unpair("synthetic-desktop").await?;
        assert!(!app.allowed("synthetic-desktop", "battery").await);
        drop(peer);
        let _ = tokio::time::timeout(Duration::from_secs(2), task).await?;
    }
    Ok(())
}
#[tokio::test]
async fn certificate_substitution_and_cn_mismatch_fail() -> Result<()> {
    let bus = Bus::new();
    let dir = tempfile::tempdir()?;
    let app = app(&bus, dir.path()).await;
    let original = Credentials::generate("synthetic-desktop")?;
    let p = lomiri_kdeconnect::store::Peer::new(
        "Synthetic".into(),
        lomiri_kdeconnect::tls::fingerprint(&original.cert)?,
        8,
    );
    app.inner
        .lock()
        .await
        .store
        .peers
        .insert("synthetic-desktop".into(), p);
    for (cert_id, claimed_id) in [
        ("synthetic-desktop", "synthetic-desktop"),
        ("synthetic-other", "synthetic-desktop"),
    ] {
        let replaced = Credentials::generate(cert_id)?;
        // Version 7 avoids the encrypted identity exchange and also exercises
        // downgrade refusal before any plugin can be reached.
        let (_peer, task) = raw_peer(app.clone(), &replaced, claimed_id, 7).await?;
        assert!(task.await?.is_err());
        assert!(app.inner.lock().await.devices.is_empty());
    }
    Ok(())
}

#[tokio::test]
async fn payload_tls_download_and_truncation_cleanup() -> Result<()> {
    let bus = Bus::new();
    let dir = tempfile::tempdir()?;
    let app = app(&bus, dir.path()).await;
    let creds = Credentials::generate("synthetic-desktop")?;
    let (mut peer, session) = raw_peer(app.clone(), &creds, "synthetic-desktop", 8).await?;
    peer.get_mut()
        .write_all(
            &Packet::new(
                "kdeconnect.pair",
                json!({"pair":true,"timestamp":lomiri_kdeconnect::protocol::now_millis()/1000}),
            )
            .encode()?,
        )
        .await?;
    tokio::time::sleep(Duration::from_millis(50)).await;
    accept_pair(&app, "synthetic-desktop").await?;
    read_packet(&mut peer, MAX_PACKET).await?;
    app.set_plugin("synthetic-desktop", "share", true).await?;
    for (data, size, success) in [
        (b"synthetic payload".as_slice(), 17u64, true),
        (b"short".as_slice(), 100, false),
    ] {
        let mut listener = None;
        for port in 1739..=1764 {
            if let Ok(l) = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await {
                listener = Some(l);
                break;
            }
        }
        let listener = listener.expect("a synthetic transfer port should be available");
        let mut p = Packet::new(
            "kdeconnect.share.request",
            json!({"filename":"synthetic.txt"}),
        );
        p.payload_size = Some(size);
        p.payload_info = Some(json!({"port":listener.local_addr()?.port()}));
        lomiri_kdeconnect::transfer::receive(app.clone(), "synthetic-desktop", &p).await?;
        let (tcp, _) = listener.accept().await?;
        let mut tls = creds.handshake(tcp, true).await?;
        tls.write_all(data).await?;
        tls.shutdown().await?;
        drop(tls);
        for _ in 0..100 {
            if app.transfers.available_permits() == 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(
            app.transfers.available_permits(),
            2,
            "transfer must finish or clean up"
        );
        let files: Vec<_> =
            std::fs::read_dir(&app.downloads)?.collect::<std::io::Result<Vec<_>>>()?;
        assert_eq!(
            files.len(),
            1,
            "partial files must not survive failed transfers"
        );
        if success {
            assert_eq!(std::fs::read(files[0].path())?, data);
        }
    }
    drop(peer);
    let _ = session.await?;
    Ok(())
}

#[tokio::test]
async fn dbus_control_plane_and_cli_use_isolated_session() -> Result<()> {
    let bus = Bus::new();
    let dir = tempfile::tempdir()?;
    let app = app(&bus, dir.path()).await;
    app.bus
        .object_server()
        .at(
            lomiri_kdeconnect::app::BUS_PATH,
            lomiri_kdeconnect::dbus::Api(app.clone()),
        )
        .await?;
    app.bus
        .request_name(lomiri_kdeconnect::app::BUS_NAME)
        .await?;
    let client = bus.connection().await;
    let p = zbus::Proxy::new(
        &client,
        lomiri_kdeconnect::app::BUS_NAME,
        lomiri_kdeconnect::app::BUS_PATH,
        lomiri_kdeconnect::app::BUS_NAME,
    )
    .await?;
    let devices: String = p.call("Devices", &()).await?;
    let state: serde_json::Value = serde_json::from_str(&devices)?;
    assert_eq!(state["devices"], json!([]));
    let result: zbus::Result<()> = p
        .call("AcceptPair", &("untrusted-device", "stale-fingerprint"))
        .await;
    assert!(result.is_err());
    let result: zbus::Result<()> = p
        .call("SetPlugin", &("untrusted-device", "input", true))
        .await;
    assert!(result.is_err());
    let result: zbus::Result<()> = p.call("ReportClipboard", &("synthetic clipboard",)).await;
    assert!(result.is_err());
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_kdeconnectctl"))
        .arg("devices")
        .env("DBUS_SESSION_BUS_ADDRESS", &bus.1)
        .output()
        .await?;
    assert!(output.status.success());
    let cli: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(cli["id"], state["id"]);
    Ok(())
}

#[tokio::test]
async fn outgoing_tls_role_pair_code_and_remote_unpair() -> Result<()> {
    let bus = Bus::new();
    let left_dir = tempfile::tempdir()?;
    let right_dir = tempfile::tempdir()?;
    let left = app(&bus, left_dir.path()).await;
    let right = app(&bus, right_dir.path()).await;
    let left_id = left.inner.lock().await.store.id.clone();
    let right_id = right.inner.lock().await.store.id.clone();
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let tcp = TcpStream::connect(listener.local_addr()?).await?;
    let (accepted, _) = listener.accept().await?;
    let left_task = tokio::spawn(network::establish(
        left.clone(),
        tcp,
        Some(lomiri_kdeconnect::protocol::Identity {
            id: right_id.clone(),
            name: "Synthetic right".into(),
            version: 8,
        }),
    ));
    let right_task = tokio::spawn(network::establish(right.clone(), accepted, None));
    tokio::time::timeout(Duration::from_secs(10), async {
        while left.inner.lock().await.devices.is_empty()
            || right.inner.lock().await.devices.is_empty()
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    assert!(left
        .request_pair(&right_id, "stale-displayed-fingerprint")
        .await
        .is_err());
    request_pair(&left, &right_id).await?;
    tokio::time::timeout(Duration::from_secs(2), async {
        while right.inner.lock().await.devices[&left_id].pending.is_none() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    let a = left.inner.lock().await.devices[&right_id]
        .pending
        .as_ref()
        .unwrap()
        .code
        .clone();
    let b = right.inner.lock().await.devices[&left_id]
        .pending
        .as_ref()
        .unwrap()
        .code
        .clone();
    assert_eq!(a, b);
    accept_pair(&right, &left_id).await?;
    tokio::time::timeout(Duration::from_secs(2), async {
        while !left.allowed(&right_id, "battery").await {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    right.unpair(&left_id).await?;
    tokio::time::timeout(Duration::from_secs(2), async {
        while left.allowed(&right_id, "battery").await {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    // A remote unpair must not be echoed into an endless pair:false loop.
    tokio::time::sleep(Duration::from_millis(40)).await;
    assert_eq!(left.inner.lock().await.devices[&right_id].tx.capacity(), 64);
    assert_eq!(right.inner.lock().await.devices[&left_id].tx.capacity(), 64);
    left_task.abort();
    right_task.abort();
    Ok(())
}

struct SyntheticNotifications(
    std::sync::Arc<std::sync::Mutex<Vec<(String, String)>>>,
    Arc<std::sync::atomic::AtomicUsize>,
);
#[zbus::interface(name = "org.freedesktop.Notifications")]
impl SyntheticNotifications {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: &str,
        _replaces: u32,
        _icon: &str,
        title: &str,
        body: &str,
        _actions: Vec<String>,
        hints: std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
        _timeout: i32,
    ) -> u32 {
        if app_name == "KDE Connect" {
            assert!(hints.contains_key("x-kdeconnect-origin"));
        }
        self.0.lock().unwrap().push((title.into(), body.into()));
        42
    }
    fn close_notification(&self, _id: u32) {
        self.1.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}
struct SyntheticPlayer(std::sync::Arc<std::sync::atomic::AtomicUsize>);
#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl SyntheticPlayer {
    fn play_pause(&self) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
    #[zbus(property)]
    fn metadata(&self) -> std::collections::HashMap<String, zbus::zvariant::OwnedValue> {
        [(
            "xesam:title".into(),
            zbus::zvariant::Value::from("Synthetic song")
                .try_into()
                .unwrap(),
        )]
        .into_iter()
        .collect()
    }
    #[zbus(property)]
    fn volume(&self) -> f64 {
        0.5
    }
    #[zbus(property)]
    fn position(&self) -> i64 {
        123000
    }
    #[zbus(property)]
    fn playback_status(&self) -> &str {
        "Playing"
    }
    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn can_seek(&self) -> bool {
        false
    }
}
#[tokio::test]
async fn synthetic_notification_and_mpris_services() -> Result<()> {
    let bus = Bus::new();
    let dir = tempfile::tempdir()?;
    let app = app(&bus, dir.path()).await;
    let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let plays = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let closed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mock = bus.connection().await;
    mock.object_server()
        .at(
            "/org/freedesktop/Notifications",
            SyntheticNotifications(calls.clone(), closed.clone()),
        )
        .await?;
    mock.request_name("org.freedesktop.Notifications").await?;
    mock.object_server()
        .at("/org/mpris/MediaPlayer2", SyntheticPlayer(plays.clone()))
        .await?;
    mock.request_name("org.mpris.MediaPlayer2.synthetic")
        .await?;
    let creds = Credentials::generate("synthetic-desktop")?;
    let (mut peer, task) = raw_peer(app.clone(), &creds, "synthetic-desktop", 8).await?;
    peer.get_mut()
        .write_all(
            &Packet::new(
                "kdeconnect.pair",
                json!({"pair":true,"timestamp":lomiri_kdeconnect::protocol::now_millis()/1000}),
            )
            .encode()?,
        )
        .await?;
    tokio::time::sleep(Duration::from_millis(50)).await;
    accept_pair(&app, "synthetic-desktop").await?;
    read_packet(&mut peer, MAX_PACKET).await?;
    app.set_plugin("synthetic-desktop", "notifications", true)
        .await?;
    peer.get_mut().write_all(&Packet::new("kdeconnect.notification",json!({"id":"synthetic-notice","title":"Synthetic notification","text":"<b>untrusted & text</b>"})).encode()?).await?;
    tokio::time::timeout(Duration::from_secs(2), async {
        while calls.lock().unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    assert_eq!(
        calls.lock().unwrap()[0].1,
        "&lt;b&gt;untrusted &amp; text&lt;/b&gt;"
    );
    app.set_plugin("synthetic-desktop", "media", true).await?;
    peer.get_mut()
        .write_all(
            &Packet::new(
                "kdeconnect.mpris.request",
                json!({"player":"org.mpris.MediaPlayer2.synthetic","action":"PlayPause"}),
            )
            .encode()?,
        )
        .await?;
    let response =
        tokio::time::timeout(Duration::from_secs(2), read_packet(&mut peer, MAX_PACKET)).await??;
    assert_eq!(response.kind, "kdeconnect.mpris");
    assert_eq!(response.body["title"], "Synthetic song");
    assert_eq!(response.body["pos"], 123);
    assert_eq!(response.body["volume"], 50.0);
    assert_eq!(plays.load(std::sync::atomic::Ordering::SeqCst), 1);
    let forbidden = Packet::new(
        "kdeconnect.mpris.request",
        json!({"player":"org.mpris.MediaPlayer2.synthetic","action":"OpenUri"}),
    );
    assert!(
        lomiri_kdeconnect::plugins::handle(app.clone(), "synthetic-desktop", &forbidden)
            .await
            .is_err()
    );
    let monitor_connection = bus.connection().await;
    let monitor = tokio::spawn(lomiri_kdeconnect::plugins::notification_monitor_on(
        app.clone(),
        monitor_connection,
    ));
    let notifications = zbus::Proxy::new(
        &app.bus,
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
    )
    .await?;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let _: u32 = notifications
                .call(
                    "Notify",
                    &(
                        "Synthetic phone app",
                        0u32,
                        "",
                        "Synthetic local",
                        "Synthetic body",
                        Vec::<String>::new(),
                        std::collections::HashMap::<String, zbus::zvariant::OwnedValue>::new(),
                        8000i32,
                    ),
                )
                .await?;
            if !app.local_notifications.lock().await.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    let forwarded =
        tokio::time::timeout(Duration::from_secs(2), read_packet(&mut peer, MAX_PACKET)).await??;
    assert_eq!(forwarded.kind, "kdeconnect.notification");
    assert_eq!(forwarded.body["title"], "Synthetic local");
    peer.get_mut()
        .write_all(
            &Packet::new(
                "kdeconnect.notification.request",
                json!({"cancel":forwarded.body["id"]}),
            )
            .encode()?,
        )
        .await?;
    tokio::time::timeout(Duration::from_secs(2), async {
        while closed.load(std::sync::atomic::Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    monitor.abort();
    drop(peer);
    let _ = task.await?;
    Ok(())
}

#[tokio::test]
async fn reconnect_clipboard_ignores_stale_content() -> Result<()> {
    let bus = Bus::new();
    let dir = tempfile::tempdir()?;
    let app = app(&bus, dir.path()).await;
    let now = lomiri_kdeconnect::protocol::now_millis();
    {
        let mut state = app.inner.lock().await;
        state.clipboard = "Synthetic current".into();
        state.clipboard_timestamp = now - 500;
    }
    lomiri_kdeconnect::plugins::handle(
        app.clone(),
        "synthetic",
        &Packet::new(
            "kdeconnect.clipboard.connect",
            json!({"content":"Synthetic stale","timestamp":now-1000}),
        ),
    )
    .await?;
    assert_eq!(app.inner.lock().await.clipboard, "Synthetic current");
    lomiri_kdeconnect::plugins::handle(
        app.clone(),
        "synthetic",
        &Packet::new(
            "kdeconnect.clipboard.connect",
            json!({"content":"Synthetic fresh","timestamp":now}),
        ),
    )
    .await?;
    assert_eq!(app.inner.lock().await.clipboard, "Synthetic fresh");
    Ok(())
}

/// Records the actions/hints of each Notify (inline-reply test).
struct ReplyNotifications(std::sync::Arc<std::sync::Mutex<Vec<(Vec<String>, Vec<String>)>>>);
#[zbus::interface(name = "org.freedesktop.Notifications")]
impl ReplyNotifications {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        _app_name: &str,
        _replaces: u32,
        _icon: &str,
        _title: &str,
        _body: &str,
        actions: Vec<String>,
        hints: std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
        _timeout: i32,
    ) -> u32 {
        let mut keys: Vec<String> = hints.keys().cloned().collect();
        keys.sort();
        self.0.lock().unwrap().push((actions, keys));
        77
    }
    fn close_notification(&self, _id: u32) {}
}

#[tokio::test]
async fn peer_notification_inline_reply() -> Result<()> {
    let bus = Bus::new();
    let dir = tempfile::tempdir()?;
    let app = app(&bus, dir.path()).await;
    let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let server = bus.connection().await;
    server
        .object_server()
        .at(
            "/org/freedesktop/Notifications",
            ReplyNotifications(calls.clone()),
        )
        .await?;
    server.request_name("org.freedesktop.Notifications").await?;
    let creds = Credentials::generate("synthetic-phone")?;
    let (mut peer, _task) = raw_peer(app.clone(), &creds, "synthetic-phone", 8).await?;
    peer.get_mut()
        .write_all(
            &Packet::new(
                "kdeconnect.pair",
                json!({"pair":true,"timestamp":lomiri_kdeconnect::protocol::now_millis()/1000}),
            )
            .encode()?,
        )
        .await?;
    tokio::time::sleep(Duration::from_millis(50)).await;
    accept_pair(&app, "synthetic-phone").await?;
    read_packet(&mut peer, MAX_PACKET).await?;
    app.set_plugin("synthetic-phone", "notifications", true)
        .await?;
    tokio::spawn(lomiri_kdeconnect::plugins::reply_listener(app.clone()));
    // A message notification from the (synthetic) Android phone, with a reply handle.
    peer.get_mut()
        .write_all(&Packet::new("kdeconnect.notification", json!({"id":"sms-1","appName":"Messages","title":"Test Contact","text":"Hello from +999 555 0100","requestReplyId":"reply-uuid-1"})).encode()?)
        .await?;
    tokio::time::timeout(Duration::from_secs(2), async {
        while calls.lock().unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    let (actions, hints) = calls.lock().unwrap()[0].clone();
    assert_eq!(
        actions,
        vec!["inline-reply".to_string(), "Reply".to_string()]
    );
    assert!(hints.contains(&"x-kde-reply-placeholder-text".to_string()));
    tokio::time::sleep(Duration::from_millis(200)).await; // match rule in place
                                                          // A NotificationReplied from a client that is not the notification server is ignored.
    let other = bus.connection().await;
    other
        .emit_signal(
            None::<()>,
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            "NotificationReplied",
            &(77u32, "spoofed"),
        )
        .await?;
    server
        .emit_signal(
            None::<()>,
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            "NotificationReplied",
            &(77u32, "On my way"),
        )
        .await?;
    let reply =
        tokio::time::timeout(Duration::from_secs(2), read_packet(&mut peer, MAX_PACKET)).await??;
    assert_eq!(reply.kind, "kdeconnect.notification.reply");
    assert_eq!(reply.body["requestReplyId"], "reply-uuid-1");
    assert_eq!(reply.body["message"], "On my way");
    // One reply per notification.
    server
        .emit_signal(
            None::<()>,
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            "NotificationReplied",
            &(77u32, "again"),
        )
        .await?;
    assert!(tokio::time::timeout(
        Duration::from_millis(300),
        read_packet(&mut peer, MAX_PACKET)
    )
    .await
    .is_err());
    // Without requestReplyId: no reply action.
    peer.get_mut()
        .write_all(
            &Packet::new(
                "kdeconnect.notification",
                json!({"id":"n-2","title":"Test Contact","text":"no reply"}),
            )
            .encode()?,
        )
        .await?;
    tokio::time::timeout(Duration::from_secs(2), async {
        while calls.lock().unwrap().len() < 2 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    assert!(calls.lock().unwrap()[1].0.is_empty());
    Ok(())
}
