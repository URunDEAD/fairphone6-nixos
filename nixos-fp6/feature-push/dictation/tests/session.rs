//! Synthetic capture on a private D-Bus: never opens the host microphone.
use fp6_dictation::{BUS, PATH};
use std::{os::unix::fs::PermissionsExt, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

async fn proxy(conn: &zbus::Connection) -> zbus::Proxy<'_> {
    zbus::proxy::Builder::new(conn)
        .destination(BUS)
        .unwrap()
        .path(PATH)
        .unwrap()
        .interface(BUS)
        .unwrap()
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
        .unwrap()
}
async fn wait_state(p: &zbus::Proxy<'_>, expected: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let current = p.get_property::<String>("State").await;
            if current.ok().as_deref() == Some(expected) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn private_bus_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let bus_config = dir.path().join("bus.conf");
    std::fs::write(&bus_config, r#"<busconfig>
      <type>session</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth>
      <policy context="default"><allow own="*"/><allow send_destination="*"/><allow receive_sender="*"/></policy>
    </busconfig>"#).unwrap();
    let mut bus = Command::new("dbus-daemon")
        .arg("--config-file")
        .arg(&bus_config)
        .args(["--nofork", "--print-address=1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .expect("Install dbus or use nix-shell");
    let mut reader = BufReader::new(bus.stdout.take().unwrap());
    let mut address = String::new();
    reader.read_line(&mut address).await.unwrap();
    let address = address.trim();
    let conn = zbus::connection::Builder::address(address)
        .unwrap()
        .build()
        .await
        .unwrap();
    let other = zbus::connection::Builder::address(address)
        .unwrap()
        .build()
        .await
        .unwrap();
    let shell = std::process::Command::new("sh")
        .args(["-c", "command -v sh"])
        .output()
        .unwrap();
    let shell = String::from_utf8(shell.stdout).unwrap();
    let shell = shell.trim();
    let capture = dir.path().join("capture");
    std::fs::write(
        &capture,
        format!(
            "#!{}\nwhile :; do head -c 3200 /dev/zero; sleep 0.1; done\n",
            shell
        ),
    )
    .unwrap();
    std::fs::set_permissions(&capture, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut daemon = Command::new(env!("CARGO_BIN_EXE_fp6-dictationd"))
        .env("DBUS_SESSION_BUS_ADDRESS", address)
        .env("XDG_DATA_HOME", dir.path())
        .env("XDG_CONFIG_HOME", dir.path())
        .env("FP6_CAPTURE_PROGRAM", &capture)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let p = proxy(&conn).await;
    let q = proxy(&other).await;
    wait_state(&p, "idle").await;
    assert!(
        p.call::<_, _, u64>("Start", &()).await.is_err(),
        "Missing model must fail before capture"
    );
    let model = fp6_dictation::model(&fp6_dictation::Settings::default().model).unwrap();
    let model_dir = dir.path().join("fp6-dictation/models");
    std::fs::create_dir_all(&model_dir).unwrap();
    // Sparse placeholder is never passed to inference: synthetic silence is gated first.
    std::fs::File::create(model_dir.join(model.filename))
        .unwrap()
        .set_len(model.bytes)
        .unwrap();
    let id: u64 = p.call("Start", &()).await.unwrap();
    wait_state(&p, "recording").await;
    assert!(p.get_property::<bool>("MicActive").await.unwrap());
    assert!(q.call::<_, _, u64>("Start", &()).await.is_err());
    assert!(q.call::<_, _, ()>("Cancel", &(id,)).await.is_err());
    assert!(q.call::<_, _, String>("Stop", &(id,)).await.is_err());
    assert!(p.call::<_, _, String>("Stop", &(id + 1,)).await.is_err());
    tokio::time::sleep(Duration::from_millis(200)).await;
    let text: String = p.call("Stop", &(id,)).await.unwrap();
    assert!(text.is_empty());
    wait_state(&p, "idle").await;
    assert!(!p.get_property::<bool>("MicActive").await.unwrap());
    let id: u64 = p.call("Start", &()).await.unwrap();
    let _: () = p.call("Cancel", &(id,)).await.unwrap();
    wait_state(&p, "idle").await;
    let owner = zbus::connection::Builder::address(address)
        .unwrap()
        .build()
        .await
        .unwrap();
    let _: u64 = proxy(&owner).await.call("Start", &()).await.unwrap();
    wait_state(&p, "recording").await;
    owner.close().await.unwrap();
    wait_state(&p, "idle").await;
    // Capture failure also returns idle and permits a new session.
    std::fs::write(&capture, format!("#!{shell}\nexit 1\n")).unwrap();
    let _: u64 = p.call("Start", &()).await.unwrap();
    wait_state(&p, "idle").await;
    // An overproducing recorder must hit the byte cap, without a 30-second wait.
    std::fs::write(&capture, format!("#!{shell}\nhead -c 1000000 /dev/zero\n")).unwrap();
    let _: u64 = p.call("Start", &()).await.unwrap();
    wait_state(&p, "idle").await;
    // Graceful SIGTERM is also handled outside systemd's cgroup cleanup.
    std::fs::write(
        &capture,
        format!("#!{shell}\nwhile :; do head -c 3200 /dev/zero; sleep 0.1; done\n"),
    )
    .unwrap();
    let _: u64 = p.call("Start", &()).await.unwrap();
    wait_state(&p, "recording").await;
    assert!(Command::new("sh")
        .args([
            "-c",
            "kill -TERM \"$1\"",
            "sh",
            &daemon.id().unwrap().to_string()
        ])
        .status()
        .await
        .unwrap()
        .success());
    assert!(tokio::time::timeout(Duration::from_secs(5), daemon.wait())
        .await
        .unwrap()
        .unwrap()
        .success());
    bus.kill().await.unwrap();
}
