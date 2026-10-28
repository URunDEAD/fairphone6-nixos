use fp6_llm::{
    download,
    manager::{available_mib, memory_pressure, Manager, Options},
    service, Model, Settings,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};
use tempfile::TempDir;

fn synthetic_model(data: &[u8]) -> Model {
    Model {
        id: "synthetic".into(),
        name: "Synthetic model".into(),
        quantization: "Q4_0".into(),
        license: "MIT".into(),
        license_url: "https://example.invalid/license".into(),
        source: "https://example.invalid".into(),
        revision: "test".into(),
        url: String::new(),
        sha256: format!("{:x}", Sha256::digest(data)),
        bytes: data.len() as u64,
        ram_mib: 10,
        kv_kib_per_token: 128,
        notes: "test only".into(),
    }
}
async fn fixture(data: &[u8]) -> (TempDir, Arc<Manager>) {
    let tmp = tempfile::tempdir().unwrap();
    let mut opts = Options::new(
        tmp.path().into(),
        env!("CARGO_BIN_EXE_fake-llama-server").into(),
        None,
    );
    opts.meminfo = tmp.path().join("meminfo");
    opts.pressure = tmp.path().join("pressure");
    opts.saver_file = tmp.path().join("saver");
    opts.check_power_profiles = false;
    opts.start_timeout = Duration::from_secs(3);
    std::fs::write(&opts.meminfo, "MemAvailable: 8388608 kB\n").unwrap();
    std::fs::write(&opts.pressure, "full avg10=0.00 avg60=0.00 total=0\n").unwrap();
    let m = synthetic_model(data);
    let manager = Manager::new(opts, vec![m.clone()]).await.unwrap();
    std::fs::write(m.path(&manager.model_root()), data).unwrap();
    manager.set_active(&m.id).await.unwrap();
    (tmp, manager)
}
async fn proxy(manager: Arc<Manager>) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    *manager.endpoint.write().await = format!("{url}/v1");
    let task = tokio::spawn(async move {
        axum::serve(listener, service::router(manager))
            .await
            .unwrap();
    });
    (url, task)
}
#[test]
fn resource_parsers_and_settings_validation() {
    assert_eq!(
        available_mib("MemFree: 10 kB\nMemAvailable: 1048576 kB\n").unwrap(),
        1024
    );
    assert!(available_mib("MemFree: 20 kB").is_err());
    assert!(memory_pressure(
        "some avg10=60.0\nfull avg10=5.01 avg60=0.0"
    ));
    assert!(!memory_pressure("some avg10=90.0\nfull avg10=0.00"));
    let settings = Settings {
        idle_seconds: 0,
        ..Settings::default()
    };
    assert!(settings.validate().is_err());
    let catalogue = fp6_llm::catalogue();
    for m in catalogue {
        assert_eq!(m.sha256.len(), 64);
        assert!(m.url.contains(&m.revision));
        assert!(m.bytes > 1_000_000_000);
    }
}
#[tokio::test]
async fn lazy_start_tools_streaming_unload_and_delete() {
    let (_tmp, manager) = fixture(b"synthetic gguf").await;
    let (url, task) = proxy(manager.clone()).await;
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    assert!(!manager.status().await.loaded);
    let models: Value = http
        .get(format!("{url}/v1/models"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(models["data"][0]["id"], "synthetic");
    assert!(!manager.status().await.loaded);
    let invalid = http
        .post(format!("{url}/v1/chat/completions"))
        .json(&json!({"model":"stale", "messages":[]}))
        .send()
        .await
        .unwrap();
    assert_eq!(invalid.status(), reqwest::StatusCode::NOT_FOUND);
    let media = http.post(format!("{url}/v1/chat/completions")).json(&json!({"model":"synthetic", "messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"https://example.invalid/synthetic.png"}}]}]})).send().await.unwrap();
    assert_eq!(media.status(), reqwest::StatusCode::BAD_REQUEST);
    assert!(!manager.status().await.loaded);
    let tools = json!([{"type":"function","function":{"name":"get_time","parameters":{"type":"object","properties":{}}}}]);
    let result: Value = http.post(format!("{url}/v1/chat/completions")).json(&json!({"model":"synthetic","messages":[{"role":"user","content":"what time?"}],"tools":tools})).send().await.unwrap().json().await.unwrap();
    assert_eq!(result["forwarded_tools"], tools);
    assert_eq!(
        result["choices"][0]["message"]["tool_calls"][0]["function"]["name"],
        "get_time"
    );
    assert!(manager.status().await.loaded);
    let stream = http
        .post(format!("{url}/v1/chat/completions"))
        .json(&json!({"model":"synthetic","messages":[],"stream":true,"tools":tools}))
        .send()
        .await
        .unwrap();
    assert_eq!(stream.headers()["content-type"], "text/event-stream");
    let text = stream.text().await.unwrap();
    assert!(text.contains("tool_calls"));
    assert!(text.ends_with("data: [DONE]\n\n"));
    manager.unload("test").await;
    assert!(!manager.status().await.loaded);
    let (backend, guard) = manager.acquire().await.unwrap();
    assert!(backend.starts_with("http://127.0.0.1:"));
    drop(guard);
    manager.delete("synthetic").await.unwrap();
    assert!(!manager.status().await.loaded);
    assert!(manager.status().await.active.is_none());
    assert!(!manager.models[0].path(&manager.model_root()).exists());
    task.abort();
}
#[tokio::test]
async fn low_memory_saver_pressure_and_restart() {
    let (_tmp, manager) = fixture(b"synthetic gguf").await;
    std::fs::write(&manager.options.meminfo, "MemAvailable: 1024 kB\n").unwrap();
    assert!(manager.acquire().await.is_err());
    assert!(!manager.status().await.loaded);
    std::fs::write(&manager.options.meminfo, "MemAvailable: 8388608 kB\n").unwrap();
    let (_, guard) = manager.acquire().await.unwrap();
    drop(guard);
    std::fs::write(&manager.options.saver_file, "1\n").unwrap();
    manager.maintain().await;
    assert!(!manager.status().await.loaded);
    assert!(manager.acquire().await.is_err());
    std::fs::write(&manager.options.saver_file, "0\n").unwrap();
    let (_, guard) = manager.acquire().await.unwrap();
    drop(guard);
    std::fs::write(&manager.options.pressure, "full avg10=6.0\n").unwrap();
    manager.maintain().await;
    assert!(!manager.status().await.loaded);
    std::fs::write(&manager.options.pressure, "full avg10=0.0\n").unwrap();
    manager.configure(10, false, true).await.unwrap();
    assert!(manager.acquire().await.is_err());
    manager.configure(10, false, false).await.unwrap();
    assert!(manager.configure(0, false, false).await.is_err());
    assert!(manager.configure(10, true, false).await.is_err());
    assert_eq!(
        Settings::load(&manager.options.root).unwrap().idle_seconds,
        10
    );
    let (_, guard) = manager.acquire().await.unwrap();
    drop(guard);
    std::fs::write(&manager.options.meminfo, "MemAvailable: 1024 kB\n").unwrap();
    manager.maintain().await;
    assert!(!manager.status().await.loaded);
}
#[tokio::test]
async fn startup_failure_and_corruption_are_reported() {
    let (_tmp, manager) = fixture(b"exit").await;
    assert!(manager
        .acquire()
        .await
        .err()
        .unwrap()
        .to_string()
        .contains("exited"));
    assert!(!manager.status().await.loaded);
    std::fs::write(manager.models[0].path(&manager.model_root()), b"oops").unwrap();
    assert!(manager.set_active("synthetic").await.is_err());
    assert!(manager.set_active("../../escape").await.is_err());
    assert!(manager.delete("unknown").await.is_err());
}
#[tokio::test]
async fn idle_timeout_retains_inflight_stream_then_releases_model() {
    let (_tmp, manager) = fixture(b"synthetic gguf").await;
    manager.configure(10, false, false).await.unwrap();
    let (url, task) = proxy(manager.clone()).await;
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let response = http
        .post(format!("{url}/v1/chat/completions"))
        .json(&json!({"model":"synthetic","messages":[],"stream":true,"delay_ms":11000}))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(10300)).await;
    manager.maintain().await;
    assert!(manager.status().await.loaded);
    assert_eq!(manager.status().await.requests, 1);
    assert!(response.text().await.unwrap().contains("[DONE]"));
    manager.maintain().await;
    assert!(manager.status().await.loaded);
    tokio::time::sleep(Duration::from_millis(10100)).await;
    manager.maintain().await;
    assert!(!manager.status().await.loaded);
    task.abort();
}
#[tokio::test]
async fn downloads_resume_hash_and_handle_ignored_ranges() {
    use axum::{
        body::Body,
        extract::State,
        http::{HeaderMap, StatusCode},
        routing::get,
        Router,
    };
    let data = b"synthetic gguf";
    let tmp = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    async fn serve(State(ignore): State<bool>, headers: HeaderMap) -> axum::response::Response {
        let data = b"synthetic gguf";
        if !ignore && headers.contains_key("range") {
            assert_eq!(headers["range"], "bytes=4-");
            axum::response::Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header("Content-Range", "bytes 4-13/14")
                .body(Body::from(data[4..].to_vec()))
                .unwrap()
        } else {
            axum::response::Response::new(Body::from(data.to_vec()))
        }
    }
    let router = Router::new()
        .route("/resume", get(serve).with_state(false))
        .route("/ignore", get(serve).with_state(true));
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    for path in ["resume", "ignore"] {
        let mut model = synthetic_model(data);
        model.url = format!("{url}/{path}");
        std::fs::write(model.part(tmp.path()), &data[..4]).unwrap();
        let mut events = Vec::new();
        download::fetch(&model, tmp.path(), &client, |p| events.push(p))
            .await
            .unwrap();
        assert_eq!(std::fs::read(model.path(tmp.path())).unwrap(), data);
        assert_eq!(events.last().unwrap().state, "complete");
        assert!(!model.part(tmp.path()).exists());
        std::fs::remove_file(model.path(tmp.path())).unwrap();
    }
    let mut model = synthetic_model(data);
    model.sha256 = "0".repeat(64);
    model.url = format!("{url}/ignore");
    assert!(download::fetch(&model, tmp.path(), &client, |_| {})
        .await
        .unwrap_err()
        .to_string()
        .contains("SHA-256"));
    assert!(model.part(tmp.path()).exists());
    assert!(!model.path(tmp.path()).exists());
    task.abort();
}

#[tokio::test]
async fn dbus_contract_and_progress_signals() {
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
        eprintln!("D-Bus test needs dbus-run-session; skipped");
        return;
    }
    use futures_util::StreamExt;
    let (_tmp, manager) = fixture(b"synthetic gguf").await;
    let (url, task) = proxy(manager.clone()).await;
    let _service = service::bus(manager.clone()).await.unwrap();
    let client = zbus::Connection::session().await.unwrap();
    let p = zbus::Proxy::new(&client, fp6_llm::BUS, fp6_llm::OBJECT, fp6_llm::BUS)
        .await
        .unwrap();
    let (endpoint, model): (String, String) = p.call("Endpoint", &()).await.unwrap();
    assert_eq!(endpoint, format!("{url}/v1"));
    assert_eq!(model, "synthetic");
    let status: String = p.call("Status", &()).await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&status).unwrap()["loaded"],
        false
    );
    let models: String = p.call("ListModels", &()).await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&models).unwrap()[0]["installed"],
        true
    );
    p.call::<_, _, ()>("Configure", &(30_u64, false, false))
        .await
        .unwrap();
    p.call::<_, _, ()>("SetContextSize", &(4096_u32,))
        .await
        .unwrap();
    let status: Value =
        serde_json::from_str(&p.call::<_, _, String>("Status", &()).await.unwrap()).unwrap();
    assert_eq!((status["n_ctx"].as_u64(), status["context_tokens"].as_u64()), (Some(4096), Some(4096)));
    assert!(p.call::<_, _, ()>("SetContextSize", &(100_u32,)).await.is_err());
    p.call::<_, _, ()>("SetActive", &("synthetic",))
        .await
        .unwrap();
    assert!(p
        .call::<_, _, ()>("Download", &("synthetic",))
        .await
        .is_err());
    let mut signals = p.receive_signal("DownloadProgress").await.unwrap();
    manager
        .events
        .send(download::Progress::new(
            &manager.models[0],
            7,
            "downloading",
            "",
        ))
        .unwrap();
    let signal = tokio::time::timeout(Duration::from_secs(2), signals.next())
        .await
        .unwrap()
        .unwrap();
    let event: (String, u64, u64, String, String) = signal.body().deserialize().unwrap();
    assert_eq!(event.0, "synthetic");
    assert_eq!(event.1, 7);
    p.call::<_, _, ()>("Unload", &()).await.unwrap();
    p.call::<_, _, ()>("Delete", &("synthetic",)).await.unwrap();
    assert!(p
        .call::<_, _, (String, String)>("Endpoint", &())
        .await
        .is_err());
    task.abort();
}

#[tokio::test]
async fn preferences_persist_and_missing_memory_unloads() {
    let (_tmp, manager) = fixture(b"synthetic gguf").await;
    manager.configure(30, false, false).await.unwrap();
    let restarted = Manager::new(manager.options.clone(), manager.models.clone())
        .await
        .unwrap();
    assert_eq!(
        restarted.status().await.active.as_deref(),
        Some("synthetic")
    );
    assert_eq!(restarted.status().await.idle_seconds, 30);
    assert!(!restarted.status().await.loaded);
    let (_, guard) = restarted.acquire().await.unwrap();
    drop(guard);
    std::fs::remove_file(&restarted.options.meminfo).unwrap();
    assert!(restarted.acquire().await.is_err());
    assert!(!restarted.status().await.loaded);
    assert_eq!(
        restarted.status().await.reason,
        "memory information unavailable"
    );
}

fn server_ctx(manager: &Manager) -> String {
    let args: Vec<String> = serde_json::from_slice(
        &std::fs::read(manager.models[0].path(&manager.model_root()).with_extension("args.json"))
            .unwrap(),
    )
    .unwrap();
    args[args.iter().position(|a| a == "--ctx-size").unwrap() + 1].clone()
}
#[tokio::test]
async fn context_defaults_to_8192_and_shrinks_when_ram_is_short() {
    use fp6_llm::{context_candidates, plan_context, DEFAULT_CTX};
    assert_eq!(context_candidates(0), vec![8192, 4096, 2048]);
    assert_eq!(context_candidates(16384), vec![16384, 8192, 4096, 2048]);
    assert_eq!(context_candidates(1000), vec![2048]);
    let m = synthetic_model(b"x");
    // 10 MiB + 128 KiB/token over 2048, + 512 MiB reserve.
    assert_eq!(m.ram_mib_at(8192), 10 + 768);
    assert_eq!(plan_context(&m, 0, 8192, 512), Some(DEFAULT_CTX));
    assert_eq!(plan_context(&m, 0, 1100, 512), Some(4096));
    assert_eq!(plan_context(&m, 0, 600, 512), Some(2048));
    assert_eq!(plan_context(&m, 0, 500, 512), None);
    // The packaged catalogue: every model fits 8192 tokens with ~6 GiB free.
    for m in fp6_llm::catalogue() {
        assert!(m.kv_kib_per_token >= 100, "{}", m.id);
        assert_eq!(plan_context(&m, 0, 6 * 1024, 512), Some(8192), "{}", m.id);
    }

    let (_tmp, manager) = fixture(b"synthetic gguf").await;
    let status = manager.status().await;
    assert_eq!((status.n_ctx, status.context_tokens, status.context_requested), (8192, 0, 8192));
    let (_, guard) = manager.acquire().await.unwrap();
    drop(guard);
    assert_eq!(server_ctx(&manager), "8192");
    assert_eq!(manager.status().await.n_ctx, 8192);
    // Less free RAM: the next load halves the context; Status says so before and after.
    manager.unload("test").await;
    std::fs::write(&manager.options.meminfo, "MemAvailable: 1126400 kB\n").unwrap();
    assert_eq!(manager.status().await.n_ctx, 4096);
    let (_, guard) = manager.acquire().await.unwrap();
    drop(guard);
    assert_eq!(server_ctx(&manager), "4096");
    // A loaded server keeps its context even when RAM recovers.
    std::fs::write(&manager.options.meminfo, "MemAvailable: 8388608 kB\n").unwrap();
    assert_eq!(manager.status().await.n_ctx, 4096);
    // An explicit size persists, unloads, and is used on the next load.
    manager.set_context(16384).await.unwrap();
    assert!(!manager.status().await.loaded);
    assert_eq!(manager.status().await.n_ctx, 16384);
    let (_, guard) = manager.acquire().await.unwrap();
    drop(guard);
    assert_eq!(server_ctx(&manager), "16384");
    assert!(manager.set_context(1000).await.is_err());
    assert!(manager.set_context(65536).await.is_err());
    assert_eq!(Settings::load(&manager.options.root).unwrap().context_tokens, 16384);
    manager.set_context(0).await.unwrap();
    // Not even 2048 tokens fit: refused, as before.
    manager.unload("test").await;
    std::fs::write(&manager.options.meminfo, "MemAvailable: 524288 kB\n").unwrap();
    assert!(manager.acquire().await.is_err());
    assert!(!manager.status().await.loaded);
}
