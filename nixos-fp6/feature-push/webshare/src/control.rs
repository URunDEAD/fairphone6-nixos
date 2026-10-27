use crate::{
    network,
    session::{Manager, StartOptions, secret, secure_eq},
};
use anyhow::{Result, bail};
use axum::{
    Router,
    body::to_bytes,
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use tokio::net::TcpListener;

pub const BUS: &str = "com.fairphone6.WebShare1";
pub const OBJECT: &str = "/com/fairphone6/WebShare1";
pub struct Api(pub Arc<Manager>);
fn bus_error(e: impl std::fmt::Display) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(e.to_string())
}
#[zbus::interface(name = "com.fairphone6.WebShare1")]
impl Api {
    async fn start(&self, options_json: &str) -> zbus::fdo::Result<String> {
        let options: StartOptions = serde_json::from_str(options_json).map_err(bus_error)?;
        let status = self.0.start(options).await.map_err(bus_error)?;
        serde_json::to_string(&status).map_err(bus_error)
    }
    async fn stop(&self) {
        self.0.stop().await;
    }
    async fn status(&self) -> String {
        self.0.status().await.to_string()
    }
    async fn interfaces(&self) -> zbus::fdo::Result<String> {
        serde_json::to_string(&network::interfaces().map_err(bus_error)?).map_err(bus_error)
    }
    async fn approve(&self, id: &str) -> zbus::fdo::Result<String> {
        let s = self.0.session().await.map_err(bus_error)?;
        // fsync/copy must not stall the network runtime.
        let id = id.to_owned();
        let path = tokio::task::spawn_blocking(move || s.approve(&id))
            .await
            .map_err(bus_error)?
            .map_err(bus_error)?;
        Ok(path.to_string_lossy().to_string())
    }
    async fn reject(&self, id: &str) -> zbus::fdo::Result<()> {
        self.0
            .session()
            .await
            .map_err(bus_error)?
            .reject(id)
            .map_err(bus_error)
    }
    async fn take_text(&self) -> zbus::fdo::Result<String> {
        self.0
            .session()
            .await
            .map_err(bus_error)?
            .take_text()
            .map_err(bus_error)
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Endpoint {
    pub url: String,
    pub key: String,
}
#[derive(Clone)]
struct Control {
    manager: Arc<Manager>,
    endpoint: Endpoint,
    host: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Command {
    action: String,
    #[serde(default)]
    args: Value,
}
pub async fn listen(manager: Arc<Manager>) -> Result<(Endpoint, tokio::task::JoinHandle<()>)> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let host = listener.local_addr()?.to_string();
    let endpoint = Endpoint {
        url: format!("http://{host}/control"),
        key: secret(),
    };
    let control = Control {
        manager,
        endpoint: endpoint.clone(),
        host,
    };
    let router = Router::new()
        .route("/control", axum::routing::post(handle))
        .with_state(control);
    let task = tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
    });
    Ok((endpoint, task))
}
async fn handle(
    State(c): State<Control>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: Request,
) -> Response {
    let valid_key = req
        .headers()
        .get("x-webshare-key")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| secure_eq(v, &c.endpoint.key));
    let valid_host =
        req.headers().get("host").and_then(|v| v.to_str().ok()) == Some(c.host.as_str());
    // Qt's file:// QML XMLHttpRequest has no Origin (or the opaque "null" origin).
    let valid_origin = req.headers().get("origin").is_none_or(|v| v == "null");
    if !peer.ip().is_loopback() || !valid_key || !valid_host || !valid_origin {
        return StatusCode::FORBIDDEN.into_response();
    }
    let bytes = match tokio::time::timeout(
        Duration::from_secs(10),
        to_bytes(req.into_body(), 128 * 1024),
    )
    .await
    {
        Ok(Ok(b)) => b,
        _ => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
    };
    let command = match serde_json::from_slice::<Command>(&bytes) {
        Ok(cmd) => cmd,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    let result = execute(c.manager, command).await;
    let (code, value) = match result {
        Ok(v) => (StatusCode::OK, json!({"ok":true,"value":v})),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            json!({"ok":false,"error":e.to_string()}),
        ),
    };
    (code, [("cache-control", "no-store")], axum::Json(value)).into_response()
}
async fn execute(m: Arc<Manager>, cmd: Command) -> Result<Value> {
    match cmd.action.as_str() {
        "status" => Ok(m.status().await),
        "interfaces" => Ok(serde_json::to_value(network::interfaces()?)?),
        "start" => Ok(serde_json::to_value(
            m.start(serde_json::from_value(cmd.args)?).await?,
        )?),
        "stop" => {
            m.stop().await;
            Ok(Value::Null)
        }
        "approve" => {
            let s = m.session().await?;
            let id = cmd.args.as_str().unwrap_or("").to_owned();
            let path = tokio::task::spawn_blocking(move || s.approve(&id)).await??;
            Ok(json!(path))
        }
        "reject" => {
            m.session().await?.reject(cmd.args.as_str().unwrap_or(""))?;
            Ok(Value::Null)
        }
        "takeText" => Ok(json!(m.session().await?.take_text()?)),
        _ => bail!("Unknown control action"),
    }
}
