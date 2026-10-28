// Synthetic subprocess used by the integration tests. No GGUF or external network.
use axum::{
    body::Body,
    extract::Json,
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use serde_json::{json, Value};
use std::{convert::Infallible, time::Duration};
#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let value = |name: &str| {
        args.get(args.iter().position(|s| s == name).unwrap() + 1)
            .unwrap()
            .clone()
    };
    assert_eq!(value("--host"), "127.0.0.1");
    assert!(args.iter().any(|a| a == "--jinja"));
    assert!(args.iter().any(|a| a == "--offline"));
    assert!(args.iter().any(|a| a == "--no-webui"));
    let model = std::path::PathBuf::from(value("--model"));
    std::fs::write(
        model.with_extension("args.json"),
        serde_json::to_vec(&args).unwrap(),
    )
    .unwrap();
    if std::fs::read_to_string(model).unwrap() == "exit" {
        std::process::exit(7);
    }
    let app = Router::new().route("/health", get(|| async { Json(json!({"status":"ok"})) }))
        .route("/v1/chat/completions", post(|Json(body): Json<Value>| async move {
            if body["stream"] == true {
                let stream = async_stream::stream! {
                    yield Ok::<_, Infallible>("data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_synthetic\",\"type\":\"function\",\"function\":{\"name\":\"get_time\",\"arguments\":\"{}\"}}]}}]}\n\n");
                    tokio::time::sleep(Duration::from_millis(body["delay_ms"].as_u64().unwrap_or(50))).await;
                    yield Ok("data: [DONE]\n\n");
                };
                Response::builder().header("content-type", "text/event-stream").body(Body::from_stream(stream)).unwrap()
            } else {
                Json(json!({"object":"chat.completion","model":body["model"],"choices":[{"finish_reason":"tool_calls","message":{"role":"assistant","content":null,"tool_calls":[{"id":"call_synthetic","type":"function","function":{"name":"get_time","arguments":"{}"}}]}}],"forwarded_tools":body["tools"]})).into_response()
            }
        }));
    let listener = tokio::net::TcpListener::bind((
        std::net::Ipv4Addr::LOCALHOST,
        value("--port").parse::<u16>().unwrap(),
    ))
    .await
    .unwrap();
    axum::serve(listener, app).await.unwrap();
}
