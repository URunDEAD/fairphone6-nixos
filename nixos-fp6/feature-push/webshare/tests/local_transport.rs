use axum::{Router, routing::get};
use fp6_webshare::{control, session::Manager, transport};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tokio_util::sync::CancellationToken;

async fn request(address: std::net::SocketAddr, request: String) -> String {
    let mut socket = TcpStream::connect(address).await.unwrap();
    socket.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    tokio::time::timeout(Duration::from_secs(5), socket.read_to_string(&mut response))
        .await
        .unwrap()
        .unwrap();
    response
}
#[tokio::test]
async fn stop_drains_final_response_and_closes_listener() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let stop = CancellationToken::new();
    let handler_stop = stop.clone();
    let router = Router::new().route(
        "/",
        get(move || {
            let stop = handler_stop.clone();
            async move {
                stop.cancel();
                "synthetic final response"
            }
        }),
    );
    let task = tokio::spawn(transport::serve(listener, router, stop));
    let response = request(
        address,
        "GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n".into(),
    )
    .await;
    assert!(response.ends_with("synthetic final response"), "{response}");
    task.await.unwrap();
    assert!(TcpStream::connect(address).await.is_err());
}
#[tokio::test]
async fn control_requires_private_key_and_rejects_browser_origins() {
    let dir = tempfile::tempdir().unwrap();
    let manager = Manager::new(dir.path().join("stage"), dir.path().join("received")).unwrap();
    let (endpoint, task) = control::listen(Arc::clone(&manager)).await.unwrap();
    let host = endpoint
        .url
        .strip_prefix("http://")
        .unwrap()
        .strip_suffix("/control")
        .unwrap();
    let address = host.parse().unwrap();
    let body = r#"{"action":"status"}"#;
    for (key, origin, expected) in [
        ("wrong", "", 403),
        (
            endpoint.key.as_str(),
            "Origin: http://evil.invalid\r\n",
            403,
        ),
        (endpoint.key.as_str(), "", 200),
    ] {
        let response=request(address,format!("POST /control HTTP/1.1\r\nHost: {host}\r\nX-WebShare-Key: {key}\r\n{origin}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len())).await;
        assert!(
            response.starts_with(&format!("HTTP/1.1 {expected}")),
            "{response}"
        );
        if expected == 200 {
            assert!(response.contains("\"active\":false"));
        }
    }
    task.abort();
}
