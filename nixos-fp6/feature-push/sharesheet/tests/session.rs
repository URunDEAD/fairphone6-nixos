use fp6_share::{
    model::Request,
    providers::{Actions, Outcome, Provider},
    service::Snapshot,
    BUS, PATH,
};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use zbus::{
    zvariant::{OwnedObjectPath, OwnedValue, Value},
    Connection,
};

#[derive(Clone, Default)]
struct Quick {
    online: Arc<AtomicBool>,
    sent: Arc<Mutex<Vec<String>>>,
    starts: Arc<AtomicUsize>,
    stops: Arc<AtomicUsize>,
}
#[zbus::interface(name = "com.fairphone6.QuickShare1")]
impl Quick {
    fn list_devices(&self) -> String {
        if self.online.load(Ordering::SeqCst) {
            r#"[{"id":"synthetic-laptop","name":"Synthetic laptop"}]"#
        } else {
            "[]"
        }
        .into()
    }
    #[zbus(property)]
    fn state(&self) -> String {
        "idle".into()
    }
    fn start_discovery(&self) {
        self.starts.fetch_add(1, Ordering::SeqCst);
    }
    fn stop_discovery(&self) {
        self.stops.fetch_add(1, Ordering::SeqCst);
    }
    fn list_transfers(&self) -> String {
        r#"[{"id":"synthetic-transfer","status":"connecting","pin":"1234"}]"#.into()
    }
    fn cancel(&self, id: String) {
        assert_eq!(id, "synthetic-transfer");
    }
    fn send(&self, target: String, items: Vec<String>) -> String {
        assert_eq!(target, "synthetic-laptop");
        *self.sent.lock().unwrap() = items;
        "synthetic-transfer".into()
    }
}
#[derive(Clone, Default)]
struct Kde {
    sent: Arc<Mutex<Vec<String>>>,
}
#[zbus::interface(name = "com.fairphone6.KdeConnect1")]
impl Kde {
    fn devices(&self) -> String {
        r#"{"devices":[{"id":"synthetic-paired","name":"Synthetic paired tablet","online":true,"paired":true,"plugins":{"share":true}},{"id":"unpaired","online":true,"paired":false,"plugins":{"share":true}},{"id":"disabled","online":true,"paired":true,"plugins":{"share":false}}]}"#.into()
    }
    fn action(&self, id: String, action: String, data: String) {
        assert_eq!(id, "synthetic-paired");
        assert_eq!(action, "share");
        self.sent.lock().unwrap().push(data);
    }
}
struct Hub;
#[zbus::interface(name = "com.lomiri.content.dbus.Service")]
impl Hub {
    fn known_shares_for_type(&self, kind: String) -> Vec<OwnedValue> {
        assert!(["text", "links", "pictures", "documents"].contains(&kind.as_str()));
        ["synthetic.notes", "com.fairphone6.Share"]
            .into_iter()
            .map(|id| {
                OwnedValue::try_from(Value::from((
                    id,
                    "Synthetic notes",
                    Vec::<u8>::new(),
                    "notes",
                    false,
                )))
                .unwrap()
            })
            .collect()
    }
}
struct Ocr;
#[zbus::interface(name = "com.fairphone6.Ocr1")]
impl Ocr {
    fn recognize(&self, path: String, languages: String) -> String {
        assert!(path.ends_with("synthetic.png"));
        assert_eq!(languages, "eng+ron");
        "synthetic-ocr-job".into()
    }
    fn cancel_job(&self, id: String) {
        assert_eq!(id, "synthetic-ocr-job");
    }
    fn get_job(&self, id: String) -> String {
        assert_eq!(id, "synthetic-ocr-job");
        r#"{"status":"done","result":{"text":"Synthetic garden"}}"#.into()
    }
}
struct Portal {
    destination: String,
    cancel: Arc<AtomicBool>,
}
#[zbus::interface(name = "org.freedesktop.portal.FileChooser")]
impl Portal {
    async fn save_file(
        &self,
        _parent: String,
        _title: String,
        options: std::collections::HashMap<String, OwnedValue>,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(connection)] bus: &Connection,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        let token = <&str>::try_from(options.get("handle_token").unwrap()).unwrap();
        let sender = header
            .sender()
            .unwrap()
            .as_str()
            .trim_start_matches(':')
            .replace('.', "_");
        let path = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");
        let values = std::collections::HashMap::from([(
            "uris",
            OwnedValue::try_from(Value::from(vec![self.destination.clone()])).unwrap(),
        )]);
        let code = if self.cancel.load(Ordering::SeqCst) {
            1u32
        } else {
            0u32
        };
        // Emit before returning SaveFile, testing the client's subscription race.
        bus.emit_signal(
            None::<&str>,
            path.as_str(),
            "org.freedesktop.portal.Request",
            "Response",
            &(code, values),
        )
        .await
        .unwrap();
        Ok(OwnedObjectPath::try_from(path).unwrap())
    }
}
#[test]
fn synthetic_bus() {
    if std::env::var_os("FP6_SHARE_BUS_TESTS").is_none() {
        let result = std::process::Command::new("dbus-run-session")
            .arg("--config-file")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/session-bus.conf"
            ))
            .arg("--")
            .arg(std::env::current_exe().unwrap())
            .args(["--exact", "synthetic_bus", "--nocapture"])
            .env("FP6_SHARE_BUS_TESTS", "1")
            .status()
            .expect("dbus-run-session is required for isolated integration tests");
        assert!(result.success());
        return;
    }
    // Install only synthetic tools before the async runtime creates any threads.
    // In particular, this suite must never invoke a host CUPS or WebShare UI.
    use std::os::unix::fs::PermissionsExt;
    let tools = tempfile::tempdir().unwrap();
    let shell = fp6_share::providers::executable("bash")
        .expect("bash is needed for synthetic helper executables");
    for (name, body) in [
        (
            "lpstat",
            "printf '%s\\n' 'system default destination: synthetic-printer'",
        ),
        (
            "lp",
            "printf '%s\\n' PRINT \"$@\" >> \"$FP6_SHARE_TEST_LOG\"",
        ),
        (
            "fp6-webshare",
            "printf '%s\\n' WEB \"$@\" >> \"$FP6_SHARE_TEST_LOG\"",
        ),
    ] {
        let path = tools.path().join(name);
        std::fs::write(&path, format!("#!{}\n{}\n", shell.display(), body)).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let mut paths = vec![tools.path().to_path_buf()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
    std::env::set_var("XDG_RUNTIME_DIR", tools.path());
    std::env::set_var("FP6_SHARE_TEST_LOG", tools.path().join("calls.txt"));
    tokio::runtime::Runtime::new().unwrap().block_on(run());
}
async fn run() {
    let temp = tempfile::tempdir().unwrap();
    let mut daemon = tokio::process::Command::new(env!("CARGO_BIN_EXE_fp6-share"))
        .arg("daemon")
        .env("XDG_STATE_HOME", temp.path())
        .env("FP6_SHARE_UI", temp.path().join("intentionally-absent-ui"))
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let client = Connection::session().await.unwrap();
    for _ in 0..100 {
        if fp6_share::providers::available(&client, BUS).await {
            break;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    let p = zbus::Proxy::new(&client, BUS, PATH, BUS).await.unwrap();
    p.call::<_, _, ()>("Attach", &()).await.unwrap(); // Synthetic GUI connection; no real GUI or networking.
    let share = |mime: &str, value: &str| {
        (
            vec![value.to_owned()],
            mime.to_owned(),
            "Synthetic test app".to_owned(),
        )
    };
    p.call::<_, _, ()>("Share", &share("text/plain", "Synthetic text"))
        .await
        .unwrap();
    let snapshot: String = p.call("Snapshot", &()).await.unwrap();
    let state: Snapshot = serde_json::from_str(&snapshot).unwrap();
    assert_eq!(
        state
            .targets
            .iter()
            .map(|t| t.id.as_str())
            .collect::<Vec<_>>(),
        ["action:copy"]
    );
    assert!(p
        .call::<_, _, ()>("Share", &share("text/plain", "Do not replace"))
        .await
        .is_err());
    assert!(p
        .call::<_, _, String>("Select", &(0u64, "action:copy", ""))
        .await
        .is_err());
    let result: String = p
        .call("Select", &(state.revision, "action:copy", ""))
        .await
        .unwrap();
    assert!(
        matches!(serde_json::from_str::<Outcome>(&result).unwrap(), Outcome::Copy(s) if s == "Synthetic text")
    );
    p.call::<_, _, ()>("Cancel", &(state.revision,))
        .await
        .unwrap();

    let quick = Quick::default();
    quick.online.store(true, Ordering::SeqCst);
    let kde = Kde::default();
    let mock = Connection::session().await.unwrap();
    for name in [
        "com.fairphone6.QuickShare1",
        "com.fairphone6.KdeConnect1",
        "com.lomiri.content.dbus.Service",
        "com.fairphone6.Ocr1",
        "com.fairphone6.WebShare1",
        "org.freedesktop.portal.Desktop",
    ] {
        mock.request_name(name).await.unwrap();
    }
    mock.object_server()
        .at("/com/fairphone6/QuickShare1", quick.clone())
        .await
        .unwrap();
    mock.object_server()
        .at("/com/fairphone6/KdeConnect1", kde.clone())
        .await
        .unwrap();
    mock.object_server().at("/", Hub).await.unwrap();
    mock.object_server()
        .at("/com/fairphone6/Ocr1", Ocr)
        .await
        .unwrap();
    let canceled = Arc::new(AtomicBool::new(false));
    let destination = temp.path().join("portal-saved.txt");
    mock.object_server()
        .at(
            "/org/freedesktop/portal/desktop",
            Portal {
                destination: url::Url::from_file_path(&destination).unwrap().to_string(),
                cancel: canceled.clone(),
            },
        )
        .await
        .unwrap();

    p.call::<_, _, ()>(
        "Share",
        &share("text/plain", "https://example.invalid/synthetic"),
    )
    .await
    .unwrap();
    let snapshot: String = p.call("Snapshot", &()).await.unwrap();
    let state: Snapshot = serde_json::from_str(&snapshot).unwrap();
    assert!(state
        .targets
        .iter()
        .any(|t| t.id == "quick:synthetic-laptop"));
    assert!(state.targets.iter().any(|t| t.id == "kde:synthetic-paired"));
    assert!(!state
        .targets
        .iter()
        .any(|t| t.id == "kde:unpaired" || t.id == "kde:disabled"));
    assert!(state
        .targets
        .iter()
        .any(|t| t.id == "content:synthetic.notes"));
    assert!(!state
        .targets
        .iter()
        .any(|t| t.id == "content:com.fairphone6.Share"));
    p.call::<_, _, String>("Select", &(state.revision, "quick:synthetic-laptop", ""))
        .await
        .unwrap();
    let transfer: String = p.call("QuickJob", &("synthetic-transfer",)).await.unwrap();
    assert!(transfer.contains("1234"));
    p.call::<_, _, ()>("CancelJob", &("quick", "synthetic-transfer"))
        .await
        .unwrap();
    assert_eq!(
        *quick.sent.lock().unwrap(),
        ["url:https://example.invalid/synthetic"]
    );
    p.call::<_, _, String>("Select", &(state.revision, "kde:synthetic-paired", ""))
        .await
        .unwrap();
    assert_eq!(
        *kde.sent.lock().unwrap(),
        ["https://example.invalid/synthetic"]
    );
    let snapshot: String = p.call("Snapshot", &()).await.unwrap();
    let state: Snapshot = serde_json::from_str(&snapshot).unwrap();
    assert_eq!(
        state
            .recent
            .iter()
            .map(|t| t.id.as_str())
            .collect::<Vec<_>>(),
        ["kde:synthetic-paired", "quick:synthetic-laptop"]
    );
    quick.online.store(false, Ordering::SeqCst);
    assert!(p
        .call::<_, _, String>("Select", &(state.revision, "quick:synthetic-laptop", ""))
        .await
        .is_err());
    p.call::<_, _, ()>("ClearHistory", &()).await.unwrap();
    p.call::<_, _, ()>("Cancel", &(state.revision,))
        .await
        .unwrap();
    assert!(quick.starts.load(Ordering::SeqCst) > 0);
    assert!(quick.stops.load(Ordering::SeqCst) > 0);

    let r = Request::new(
        vec!["Synthetic save".into()],
        "text/plain".into(),
        "test".into(),
    )
    .unwrap();
    let actions = Actions;
    let targets = actions.targets(&client, &r).await.unwrap();
    let save = targets.iter().find(|t| t.id == "action:save").unwrap();
    actions.activate(&client, &r, save, "").await.unwrap();
    assert_eq!(
        std::fs::read_to_string(&destination).unwrap(),
        "Synthetic save"
    );
    canceled.store(true, Ordering::SeqCst);
    assert!(actions
        .activate(&client, &r, save, "")
        .await
        .unwrap_err()
        .to_string()
        .contains("canceled"));
    let image = temp.path().join("synthetic.png");
    std::fs::write(&image, "Synthetic fixture; no decoder used by mock").unwrap();
    let r = Request::from_cli(&[image.to_string_lossy().into()]).unwrap();
    let targets = actions.targets(&client, &r).await.unwrap();
    let ocr = targets.iter().find(|t| t.id == "action:ocr").unwrap();
    assert!(
        matches!(actions.activate(&client, &r, ocr, "").await.unwrap(), Outcome::Ocr(id) if id == "synthetic-ocr-job")
    );
    let job: String = p.call("OcrJob", &("synthetic-ocr-job",)).await.unwrap();
    assert!(job.contains("Synthetic garden"));
    p.call::<_, _, ()>("CancelJob", &("ocr", "synthetic-ocr-job"))
        .await
        .unwrap();
    // Print uses only our synthetic CUPS tools, and requires confirmation.
    let targets = actions.targets(&client, &r).await.unwrap();
    let print = targets.iter().find(|t| t.id == "action:print").unwrap();
    assert!(actions.activate(&client, &r, print, "").await.is_err());
    actions
        .activate(&client, &r, print, "confirm")
        .await
        .unwrap();
    let web = fp6_share::providers::WebShare;
    let targets = web.targets(&client, &r).await.unwrap();
    web.activate(&client, &r, &targets[0], "").await.unwrap();
    let text = Request::new(
        vec!["Synthetic text download".into()],
        "text/plain".into(),
        "test".into(),
    )
    .unwrap();
    let targets = web.targets(&client, &text).await.unwrap();
    web.activate(&client, &text, &targets[0], "").await.unwrap();
    let log = std::env::var_os("FP6_SHARE_TEST_LOG").unwrap();
    for _ in 0..100 {
        if std::fs::read_to_string(&log)
            .unwrap_or_default()
            .contains("Shared text-")
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let calls = std::fs::read_to_string(log).unwrap();
    assert!(calls.contains("PRINT\n-d\nsynthetic-printer\n--\n"));
    assert!(calls.contains("WEB\ngui\n"));
    assert!(calls.contains("Shared text-"));
    // The real CLI normalizes paths and calls the running service.
    let status = tokio::process::Command::new(env!("CARGO_BIN_EXE_fp6-share"))
        .args(["--text", "Synthetic CLI"])
        .status()
        .await
        .unwrap();
    assert!(status.success());
    let snapshot: String = p.call("Snapshot", &()).await.unwrap();
    let state: Snapshot = serde_json::from_str(&snapshot).unwrap();
    assert_eq!(state.request.unwrap().items, ["Synthetic CLI"]);
    // Release the synthetic GUI connection, then immediately submit again.
    // An absent UI executable must fail, rather than accept a request invisibly.
    drop(p);
    drop(client);
    let next_client = Connection::session().await.unwrap();
    let next = zbus::Proxy::new(&next_client, BUS, PATH, BUS)
        .await
        .unwrap();
    assert!(next
        .call::<_, _, ()>("Share", &share("text/plain", "After close"))
        .await
        .is_err());
    let snapshot: String = next.call("Snapshot", &()).await.unwrap();
    assert!(serde_json::from_str::<Snapshot>(&snapshot)
        .unwrap()
        .request
        .is_none());
    daemon.kill().await.unwrap();
    daemon.wait().await.unwrap();
}
