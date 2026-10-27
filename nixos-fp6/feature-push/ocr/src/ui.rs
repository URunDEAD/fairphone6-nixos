use crate::service::{BUS, PATH};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::PathBuf,
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tiny_http::{Header, Method, Response, Server};

fn string<'a>(request: &'a Value, key: &str) -> Result<&'a str> {
    request[key]
        .as_str()
        .with_context(|| format!("Missing {key}"))
}
fn dispatch(
    proxy: &zbus::blocking::Proxy<'_>,
    request: &Value,
    exports: &std::path::Path,
) -> Result<Value> {
    Ok(match string(request, "action")? {
        "recognize" => {
            let id: String = proxy.call(
                "Recognize",
                &(string(request, "path")?, string(request, "languages")?),
            )?;
            json!({"id":id})
        }
        "job" => {
            let text: String = proxy.call("GetJob", &(string(request, "id")?,))?;
            serde_json::from_str(&text)?
        }
        "cancel" => {
            proxy.call::<_, _, ()>("CancelJob", &(string(request, "id")?,))?;
            json!({})
        }
        "latest" => {
            let path: String = proxy.call("LatestScreenshot", &())?;
            json!({"path":path})
        }
        "settings" => {
            let text: String = proxy.call("GetSettings", &())?;
            serde_json::from_str(&text)?
        }
        "index" => {
            proxy.call::<_, _, ()>(
                "SetIndexEnabled",
                &(request["enabled"].as_bool().context("Missing enabled")?,),
            )?;
            json!({})
        }
        "search" => {
            let text: String = proxy.call("Search", &(string(request, "query")?,))?;
            serde_json::from_str(&text)?
        }
        "export" => {
            let text = string(request, "text")?;
            let path = exports.join(format!("ocr-{}.txt", uuid::Uuid::new_v4()));
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)?;
            file.write_all(text.as_bytes())?;
            json!({"url":crate::ocr::file_url(&path)?})
        }
        _ => bail!("Unknown action"),
    })
}

pub fn run(initial: Option<String>) -> Result<()> {
    let initial = initial
        .map(|p| crate::ocr::local_path(&p).map(|p| p.to_string_lossy().into_owned()))
        .transpose()?
        .unwrap_or_default();
    let connection =
        zbus::blocking::Connection::session().context("The UI requires a D-Bus session")?;
    let proxy = zbus::blocking::Proxy::new(&connection, BUS, PATH, BUS)?;
    let _: String = proxy
        .call("GetSettings", &())
        .context("OCR service unavailable; enable the NixOS module or start fairphone-ocr serve")?;
    let server = Server::http("127.0.0.1:0").map_err(|e| anyhow::anyhow!("{e}"))?;
    let token = uuid::Uuid::new_v4().simple().to_string();
    let endpoint = format!("http://{}/{token}", server.server_addr());
    let temp = tempfile::tempdir()?;
    let export_dir = temp.path().join("exports");
    fs::create_dir(&export_dir)?;
    let running = Arc::new(AtomicBool::new(true));
    let thread_running = running.clone();
    let worker = std::thread::spawn(move || -> Result<()> {
        let proxy = zbus::blocking::Proxy::new(&connection, BUS, PATH, BUS)?;
        while thread_running.load(Ordering::Relaxed) {
            let Some(mut request) = server.recv_timeout(Duration::from_millis(200))? else {
                continue;
            };
            // No CORS, cookie auth, or GET side effects. A custom header prevents browser simple requests.
            let authorized = request.method() == &Method::Post
                && request.url() == format!("/{token}")
                && request
                    .headers()
                    .iter()
                    .any(|h| h.field.equiv("X-Ocr-Token") && h.value.as_str() == token);
            if !authorized {
                let _ = request.respond(Response::from_string("Forbidden").with_status_code(403));
                continue;
            }
            let result: Result<Value> = (|| {
                use std::io::Read;
                if request.body_length().is_some_and(|n| n > 128 * 1024) {
                    bail!("Request too large");
                }
                let mut body = String::new();
                request
                    .as_reader()
                    .take(128 * 1024 + 1)
                    .read_to_string(&mut body)?;
                if body.len() > 128 * 1024 {
                    bail!("Request too large");
                }
                dispatch(&proxy, &serde_json::from_str(&body)?, &export_dir)
            })();
            let value = result.unwrap_or_else(|e| json!({"error":format!("{e:#}")}));
            let response = Response::from_string(value.to_string())
                .with_header(Header::from_bytes("Content-Type", "application/json").unwrap())
                .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap());
            let _ = request.respond(response);
        }
        Ok(())
    });
    let qml_dir = std::env::var_os("OCR_QML_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("qml"));
    let qml_url = crate::ocr::file_url(&qml_dir.canonicalize()?)?;
    let entry = temp.path().join("Window.qml");
    fs::write(
        &entry,
        format!(
            "import QtQuick 2.12\nimport {} as App\nApp.Main {{ apiUrl: {}; initialPath: {} }}\n",
            serde_json::to_string(&qml_url)?,
            serde_json::to_string(&endpoint)?,
            serde_json::to_string(&initial)?
        ),
    )?;
    let status =
        Command::new(std::env::var_os("OCR_QMLSCENE").unwrap_or_else(|| "qmlscene".into()))
            .env("APP_ID", "fairphone-ocr")
            .arg(&entry)
            .status();
    running.store(false, Ordering::Relaxed);
    worker
        .join()
        .map_err(|_| anyhow::anyhow!("UI bridge failed"))??;
    if !status.context("Cannot start Qt 5 qmlscene")?.success() {
        bail!("QML window failed");
    }
    Ok(())
}
