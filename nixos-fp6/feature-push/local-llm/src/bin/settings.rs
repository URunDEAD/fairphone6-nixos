use qmetaobject::prelude::*;
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let preview_path = args
        .iter()
        .position(|a| a == "--preview")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_default();
    // The installed QML module supplies the Rust bridge in both settings and this host.
    let mut engine = QmlEngine::new();
    engine.set_property(
        "previewDownloads".into(),
        args.iter().any(|a| a == "--downloads").into(),
    );
    engine.set_property(
        "previewPath".into(),
        QString::from(preview_path.clone()).into(),
    );
    let mut models = serde_json::to_value(fp6_llm::catalogue()).unwrap();
    for (i, m) in models.as_array_mut().unwrap().iter_mut().enumerate() {
        m["installed"] = (i == 0).into();
        m["active"] = (i == 0).into();
        m["partial_bytes"] = 0.into();
        m["ram_mib_ctx"] = (m["ram_mib"].as_u64().unwrap() + 6144 * m["kv_kib_per_token"].as_u64().unwrap() / 1024).into();
        if i == 1 {
            m["progress"] = serde_json::json!({"state":"downloading","received":824102400_u64,"total":2497280256_u64,"message":""});
        }
    }
    let fixture = serde_json::json!({"status":{"active":"qwen3-1.7b-q4-k-m","loaded":false,"idle_seconds":120,"use_gpu":false,"gpu_available":true,"storage_bytes":2106541664_u64,"available_mib":4400,"battery_saver":false,"reason":"idle timeout","n_ctx":8192,"context_tokens":0,"context_requested":8192},"models":models});
    engine.set_property(
        "previewSnapshot".into(),
        QString::from(if preview_path.is_empty() {
            String::new()
        } else {
            fixture.to_string()
        })
        .into(),
    );
    engine.load_file(
        std::env::var("FP6_LLM_QML")
            .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/qml/Main.qml").into())
            .into(),
    );
    engine.exec();
}
