use crate::{manager::Manager, BUS, OBJECT};
use anyhow::Result;
use axum::{
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use zbus::{interface, object_server::SignalEmitter};

pub struct Service(pub Arc<Manager>);
fn dbus_error(e: anyhow::Error) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(format!("{e:#}"))
}
#[interface(name = "com.fairphone6.LocalLlm1")]
impl Service {
    async fn status(&self) -> String {
        serde_json::to_string(&self.0.status().await).unwrap()
    }
    async fn list_models(&self) -> String {
        self.0.list_models().await.to_string()
    }
    async fn download(&self, id: &str) -> zbus::fdo::Result<()> {
        self.0.download(id).await.map_err(dbus_error)
    }
    async fn delete(&self, id: &str) -> zbus::fdo::Result<()> {
        self.0.delete(id).await.map_err(dbus_error)
    }
    async fn set_active(&self, id: &str) -> zbus::fdo::Result<()> {
        self.0.set_active(id).await.map_err(dbus_error)
    }
    async fn endpoint(&self) -> zbus::fdo::Result<(String, String)> {
        let active =
            self.0.settings.read().await.active.clone().ok_or_else(|| {
                zbus::fdo::Error::Failed("choose a downloaded model first".into())
            })?;
        Ok((self.0.endpoint.read().await.clone(), active))
    }
    async fn unload(&self) {
        self.0.unload("unloaded by user").await;
    }
    async fn configure(
        &self,
        idle_seconds: u64,
        use_gpu: bool,
        battery_saver: bool,
    ) -> zbus::fdo::Result<()> {
        self.0
            .configure(idle_seconds, use_gpu, battery_saver)
            .await
            .map_err(dbus_error)
    }
    /// Requested context window in tokens (0 = automatic: 8192, lowered
    /// when RAM is short). The effective value is Status.n_ctx.
    async fn set_context_size(&self, tokens: u32) -> zbus::fdo::Result<()> {
        self.0.set_context(tokens).await.map_err(dbus_error)
    }
    #[zbus(signal)]
    pub async fn download_progress(
        emitter: &SignalEmitter<'_>,
        id: &str,
        received: u64,
        total: u64,
        state: &str,
        message: &str,
    ) -> zbus::Result<()>;
}
pub async fn bus(manager: Arc<Manager>) -> Result<zbus::Connection> {
    let mut events = manager.events.subscribe();
    let connection = zbus::connection::Builder::session()?
        .name(BUS)?
        .serve_at(OBJECT, Service(manager))?
        .build()
        .await?;
    let emitter = SignalEmitter::new(&connection, OBJECT)?.into_owned();
    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(p) => {
                    let _ = Service::download_progress(
                        &emitter, &p.id, p.received, p.total, &p.state, &p.message,
                    )
                    .await;
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });
    Ok(connection)
}
fn error(code: StatusCode, message: impl ToString) -> Response {
    (code, Json(json!({"error":{"message":message.to_string(),"type":"local_runtime_error","code":code.as_u16()}}))).into_response()
}
async fn models(State(manager): State<Arc<Manager>>) -> Json<Value> {
    let active = manager.settings.read().await.active.clone();
    Json(
        json!({"object":"list","data":active.into_iter().map(|id| json!({"id":id,"object":"model","owned_by":"local"})).collect::<Vec<_>>()}),
    )
}
async fn chat(State(manager): State<Arc<Manager>>, bytes: Bytes) -> Response {
    let body: Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => return error(StatusCode::BAD_REQUEST, e),
    };
    if let Some(messages) = body.get("messages").and_then(Value::as_array) {
        for message in messages {
            if let Some(parts) = message.get("content").and_then(Value::as_array) {
                if parts
                    .iter()
                    .any(|part| part.get("type").and_then(Value::as_str) != Some("text"))
                {
                    return error(
                        StatusCode::BAD_REQUEST,
                        "this local backend accepts text content only",
                    );
                }
            }
        }
    }
    let active = manager.settings.read().await.active.clone();
    if active.is_none() || body.get("model").and_then(Value::as_str) != active.as_deref() {
        return error(
            StatusCode::NOT_FOUND,
            "model must match the active model returned by Endpoint",
        );
    }
    let (url, guard) = match manager.acquire().await {
        Ok(v) => v,
        Err(e) => return error(StatusCode::SERVICE_UNAVAILABLE, e),
    };
    let response = match manager
        .http
        .post(format!("{url}/v1/chat/completions"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(bytes)
        .timeout(Duration::from_secs(900))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return error(StatusCode::BAD_GATEWAY, e),
    };
    let status = response.status();
    let content_type = response.headers().get(header::CONTENT_TYPE).cloned();
    let stream = async_stream::stream! {
        let _guard = guard;
        let mut chunks = response.bytes_stream();
        while let Some(chunk) = chunks.next().await {
            yield chunk.map_err(std::io::Error::other);
        }
    };
    let mut result = Response::new(Body::from_stream(stream));
    *result.status_mut() = status;
    if let Some(ct) = content_type {
        result.headers_mut().insert(header::CONTENT_TYPE, ct);
    }
    result
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-cache".parse().unwrap());
    result
}
pub fn router(manager: Arc<Manager>) -> Router {
    Router::new()
        .route("/v1/chat/completions", post(chat))
        .route("/v1/models", get(models))
        .route(
            "/health",
            get(|| async { Json(json!({"status":"ok","service":"fp6-llm"})) }),
        )
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
        .with_state(manager)
}
