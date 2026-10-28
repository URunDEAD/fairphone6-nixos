//! The D-Bus interface: untrusted callers can read and stop, but not drive
//! the agent, change settings or keys, register as the UI or answer dialogs.

mod common;
use common::*;
use fp6_assistant::service::{Service, BUS, PATH};
use serde_json::{json, Value};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn untrusted_callers_are_limited() {
    let Some(bus) = Bus::new() else { return };
    let e = env(None, yes_user());
    let conn = bus.connection().await;
    let bin = tempfile::tempdir().unwrap();
    conn.object_server().at(PATH, Service { agent: e.agent.clone(), bin_dir: Some(bin.path().to_path_buf()), trust_any: false }).await.unwrap();
    conn.request_name(BUS).await.unwrap();
    // This test binary is not in the package's bin/ directory.
    let client = bus.connection().await;
    let call = |m: &'static str, body: Value| {
        let client = client.clone();
        async move {
            match body {
                Value::Null => client.call_method(Some(BUS), PATH, Some(BUS), m, &()).await,
                Value::String(s) => client.call_method(Some(BUS), PATH, Some(BUS), m, &(s,)).await,
                Value::Array(a) => {
                    let v: Vec<String> = a.iter().map(|x| x.as_str().unwrap().to_string()).collect();
                    client.call_method(Some(BUS), PATH, Some(BUS), m, &(v[0].clone(), v[1].clone())).await
                }
                _ => unreachable!(),
            }
        }
    };
    for (m, b) in [
        ("RegisterUi", Value::Null),
        ("NewChat", json!("")),
        ("Quick", json!("text +999 555 0100 hello")),
        ("SetSettings", json!("{}")),
        ("SetKey", json!(["backend:anthropic", "test-key-not-real"])),
        ("ClearHistory", Value::Null),
    ] {
        let r = call(m, b).await;
        assert!(r.is_err() && r.unwrap_err().to_string().contains("AccessDenied"), "{m} must be refused");
    }
    // Read-only and reducing calls work.
    let r = call("ListChats", Value::Null).await.unwrap();
    assert_eq!(r.body().deserialize::<String>().unwrap(), "[]");
    call("StopAll", Value::Null).await.unwrap();
    let tools: String = call("Tools", Value::Null).await.unwrap().body().deserialize().unwrap();
    let tools: Value = serde_json::from_str(&tools).unwrap();
    assert!(tools.as_array().unwrap().iter().any(|t| t["name"] == "messages_send" && t["policy"] == "always"));
    // Answering a dialog without being the registered UI does nothing.
    let ok: bool = call("Respond", json!(["p1", "\"always\""])).await.unwrap().body().deserialize().unwrap();
    assert!(!ok);
    assert!(!e.agent.secrets.has("backend:anthropic").await);
}
