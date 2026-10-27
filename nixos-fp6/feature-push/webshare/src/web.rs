use crate::session::{
    MAX_PENDING, MAX_TEXT, MAX_UPLOAD, PendingUpload, PendingView, Session, secret, secure_eq,
    valid_name,
};
use axum::{
    Router,
    body::{Body, Bytes, to_bytes},
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{Html, IntoResponse, Response},
};
use futures_util::StreamExt;
use std::{
    net::SocketAddr,
    os::unix::fs::FileExt,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::io::AsyncWriteExt;

pub fn router(session: Arc<Session>) -> Router {
    Router::new().fallback(handle).with_state(session)
}
pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn page(title: &str, content: &str, script: bool) -> Response {
    Html(format!("<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{title} · WebShare</title><style>{}</style><main><header>LOCAL • PRIVATE BY DEFAULT</header><h1>{title}</h1>{content}<footer>HTTP is not encrypted. Use trusted Wi-Fi or a private phone hotspot.</footer></main>{}</html>", include_str!("../web/style.css"), if script { "<script src=\"app.js\"></script>" } else { "" })).into_response()
}
fn error(code: StatusCode, message: &str) -> Response {
    (code, message.to_owned()).into_response()
}
fn secure_headers(response: &mut Response) {
    for (name, value) in [
        ("cache-control", "no-store"),
        ("referrer-policy", "no-referrer"),
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        (
            "content-security-policy",
            "default-src 'none'; script-src 'self'; style-src 'unsafe-inline'; connect-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'",
        ),
        (
            "permissions-policy",
            "camera=(), microphone=(), geolocation=()",
        ),
    ] {
        response
            .headers_mut()
            .insert(name, HeaderValue::from_static(value));
    }
}
async fn handle(
    State(s): State<Arc<Session>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: Request,
) -> Response {
    let mut response = dispatch(s, peer, req).await;
    secure_headers(&mut response);
    response
}
fn browser_authenticated(s: &Session, headers: &HeaderMap) -> bool {
    let d = s.data.lock().unwrap();
    let Some(expected) = &d.browser else {
        return false;
    };
    headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|cookies| {
            cookies
                .split(';')
                .filter_map(|v| v.trim().strip_prefix("webshare="))
                .any(|v| secure_eq(v, expected))
        })
}
async fn dispatch(s: Arc<Session>, peer: SocketAddr, req: Request) -> Response {
    if !s.alive() {
        return error(StatusCode::GONE, "Share ended");
    }
    let allowed = match peer.ip() {
        std::net::IpAddr::V4(ip) => s.interface.contains(ip),
        _ => false,
    };
    if !allowed {
        return error(StatusCode::FORBIDDEN, "Same Wi-Fi/hotspot required");
    }
    {
        let mut d = s.data.lock().unwrap();
        if d.rate_start.elapsed() >= Duration::from_secs(60) {
            d.rate_start = Instant::now();
            d.requests = 0;
        }
        d.requests += 1;
        if d.requests > 120 {
            return error(StatusCode::TOO_MANY_REQUESTS, "Try again in one minute");
        }
    }
    let host = format!("{}:{}", s.interface.address, crate::session::PORT);
    if req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        != Some(host.as_str())
    {
        return error(StatusCode::FORBIDDEN, "Invalid host");
    }
    if req.method() != Method::GET && req.method() != Method::HEAD {
        let origin = format!("http://{host}");
        if req
            .headers()
            .get(header::ORIGIN)
            .is_some_and(|v| v.to_str().ok() != Some(origin.as_str()))
            || req
                .headers()
                .get("sec-fetch-site")
                .is_some_and(|v| v == "cross-site")
        {
            return error(StatusCode::FORBIDDEN, "Foreign origin");
        }
    }
    let prefix = format!("/s/{}", s.invitation);
    let path = req.uri().path();
    if path.contains('%')
        || path.contains('\\')
        || path.contains("..")
        || req.uri().query().is_some()
    {
        return error(StatusCode::BAD_REQUEST, "Invalid path");
    }
    let Some(tail) = path
        .strip_prefix(&prefix)
        .filter(|tail| tail.is_empty() || tail.starts_with('/'))
    else {
        return error(StatusCode::NOT_FOUND, "Not found");
    };
    let authenticated = browser_authenticated(&s, req.headers());
    if tail.is_empty() || tail == "/" {
        if req.method() == Method::GET && !authenticated {
            if s.data.lock().unwrap().browser.is_some() {
                return error(StatusCode::GONE, "This invitation has already been used");
            }
            return page(
                "Share via link",
                &format!(
                    "<p>Connect to this phone to receive shared files or send items with its owner's permission.</p><form method=\"post\" action=\"{prefix}/claim\"><button>Connect to phone</button></form>"
                ),
                false,
            );
        }
        if req.method() == Method::GET && authenticated {
            // A canonical trailing slash makes relative resources unambiguous in Safari.
            if tail.is_empty() {
                return axum::response::Redirect::to(&format!("{prefix}/")).into_response();
            }
            return landing(&s);
        }
    }
    if tail == "/claim" && req.method() == Method::POST {
        let mut d = s.data.lock().unwrap();
        if d.browser.is_some() {
            return error(StatusCode::GONE, "Invitation already used");
        }
        let cookie = secret();
        d.browser = Some(cookie.clone());
        let mut response = axum::response::Redirect::to(&format!("{prefix}/")).into_response();
        response.headers_mut().insert(
            header::SET_COOKIE,
            HeaderValue::from_str(&format!(
                "webshare={cookie}; Path={prefix}/; HttpOnly; SameSite=Strict; Max-Age={}",
                s.deadline
                    .saturating_duration_since(Instant::now())
                    .as_secs()
            ))
            .unwrap(),
        );
        return response;
    }
    if !authenticated {
        return error(StatusCode::NOT_FOUND, "Not found");
    }
    match (req.method(), tail) {
        (&Method::GET, "/app.js") => (
            [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
            include_str!("../web/app.js"),
        )
            .into_response(),
        (&Method::GET, path) if path.starts_with("/file/") => {
            let Ok(id) = path[6..].parse::<usize>() else {
                return error(StatusCode::NOT_FOUND, "Not found");
            };
            download(s, id)
        }
        (&Method::POST, "/upload") => upload(s, req).await,
        (&Method::POST, "/text") => {
            if !s.text_enabled {
                return error(StatusCode::FORBIDDEN, "Text receiving is disabled");
            }
            let body = match tokio::time::timeout(
                Duration::from_secs(15),
                to_bytes(req.into_body(), MAX_TEXT),
            )
            .await
            {
                Ok(Ok(b)) => b,
                _ => return error(StatusCode::PAYLOAD_TOO_LARGE, "Text too large or timed out"),
            };
            let text = match String::from_utf8(body.to_vec()) {
                Ok(t) if !t.trim().is_empty() && !t.contains('\0') => t,
                _ => return error(StatusCode::BAD_REQUEST, "Send UTF-8 text"),
            };
            let mut d = s.data.lock().unwrap();
            if !s.alive() {
                return error(StatusCode::GONE, "Share ended");
            }
            if d.text.is_some() {
                return error(StatusCode::CONFLICT, "Previous text awaits the phone owner");
            }
            d.text = Some(text);
            (
                StatusCode::ACCEPTED,
                "Text sent. The phone owner can copy it.",
            )
                .into_response()
        }
        (&Method::POST, "/finish") => {
            {
                let d = s.data.lock().unwrap();
                if !d.pending.is_empty() || d.text.is_some() {
                    return error(
                        StatusCode::CONFLICT,
                        "Items still await confirmation on the phone",
                    );
                }
            }
            s.stop();
            (StatusCode::OK, "Sharing ended. You can close this page.").into_response()
        }
        _ => error(StatusCode::NOT_FOUND, "Not found"),
    }
}
fn landing(s: &Session) -> Response {
    let mut content = String::from(
        "<p>Connected to the phone. This link is now reserved for your browser.</p><section><h2>Shared files</h2>",
    );
    if s.files.is_empty() {
        content.push_str("<p>No files shared by the phone.</p>");
    }
    for (id, f) in s.files.iter().enumerate() {
        content.push_str(&format!("<a class=\"file\" href=\"file/{id}\" download><span>{}</span><small>{} bytes · Download</small></a>", escape(&f.name), f.size));
    }
    content.push_str("</section>");
    if s.uploads {
        content.push_str("<section><h2>Send a file</h2><p>Up to 100 MiB. Each file needs approval on the phone.</p><input type=\"file\" id=\"file\"><button id=\"upload\">Send for approval</button></section>");
    }
    if s.text_enabled {
        content.push_str("<section><h2>Send text or a link</h2><textarea id=\"text\" maxlength=\"4096\" rows=\"4\" placeholder=\"Paste text here\" aria-label=\"Text to send\"></textarea><button id=\"sendText\">Send text</button></section>");
    }
    content.push_str("<p id=\"status\" role=\"status\" aria-live=\"polite\"></p><button id=\"finish\" class=\"secondary\">Finish sharing</button><p>The share closes automatically after all downloads (download-only mode) or when its timer expires.</p>");
    page("Your local share", &content, true)
}
fn download(s: Arc<Session>, id: usize) -> Response {
    let Some(file) = s.files.get(id) else {
        return error(StatusCode::NOT_FOUND, "Not found");
    };
    let Ok(permit) = s.transfers.clone().try_acquire_owned() else {
        return error(StatusCode::TOO_MANY_REQUESTS, "Too many transfers");
    };
    let handle = file.file.clone();
    let size = file.size;
    let filename =
        percent_encoding::utf8_percent_encode(&file.name, percent_encoding::NON_ALPHANUMERIC)
            .to_string();
    let stream = async_stream::try_stream! {
        let _permit = permit;
        let mut offset = 0;
        while offset < size {
            if !s.alive() { Err(std::io::Error::other("Share ended"))?; }
            let f = handle.clone();
            let remaining = (size - offset).min(64 * 1024) as usize;
            let chunk = tokio::task::spawn_blocking(move || {
                let mut buf = vec![0; remaining]; let n = f.read_at(&mut buf, offset)?; buf.truncate(n);
                Ok::<_, std::io::Error>(buf)
            }).await.map_err(std::io::Error::other)??;
            if chunk.is_empty() { Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "Shared file changed"))?; }
            offset += chunk.len() as u64;
            yield Bytes::from(chunk);
        }
        s.downloaded(id);
    };
    let body = Body::from_stream(stream.map(|item: Result<Bytes, std::io::Error>| item));
    let mut response = Response::new(body);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    // Chunked body ensures completion callback is polled after the final chunk.
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "attachment; filename=\"download\"; filename*=UTF-8''{filename}"
        ))
        .unwrap(),
    );
    response
}
async fn upload(s: Arc<Session>, req: Request) -> Response {
    if !s.uploads {
        return error(StatusCode::FORBIDDEN, "Uploads are disabled");
    }
    let Some(encoded) = req
        .headers()
        .get("x-file-name")
        .and_then(|v| v.to_str().ok())
    else {
        return error(StatusCode::BAD_REQUEST, "Missing filename");
    };
    let name = match percent_encoding::percent_decode_str(encoded).decode_utf8() {
        Ok(n) if valid_name(&n) => n.into_owned(),
        _ => return error(StatusCode::BAD_REQUEST, "Invalid filename"),
    };
    if req
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .is_some_and(|n| n > MAX_UPLOAD)
    {
        return error(StatusCode::PAYLOAD_TOO_LARGE, "100 MiB maximum");
    }
    let Ok(_permit) = s.transfers.clone().try_acquire_owned() else {
        return error(StatusCode::TOO_MANY_REQUESTS, "Too many transfers");
    };
    if s.data.lock().unwrap().pending.len() >= MAX_PENDING {
        return error(StatusCode::TOO_MANY_REQUESTS, "Pending queue full");
    }
    let file = match tempfile::NamedTempFile::new_in(&s.staging) {
        Ok(f) => f,
        Err(_) => return error(StatusCode::INSUFFICIENT_STORAGE, "No staging space"),
    };
    let writer = match file.reopen() {
        Ok(f) => f,
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "Staging failed"),
    };
    let mut writer = tokio::fs::File::from_std(writer);
    let result = tokio::time::timeout(Duration::from_secs(60), async {
        let mut total = 0u64;
        let mut stream = req.into_body().into_data_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| StatusCode::BAD_REQUEST)?;
            total = total
                .checked_add(chunk.len() as u64)
                .ok_or(StatusCode::PAYLOAD_TOO_LARGE)?;
            if total > MAX_UPLOAD {
                return Err(StatusCode::PAYLOAD_TOO_LARGE);
            }
            if !s.alive() {
                return Err(StatusCode::GONE);
            }
            writer
                .write_all(&chunk)
                .await
                .map_err(|_| StatusCode::INSUFFICIENT_STORAGE)?;
        }
        writer
            .flush()
            .await
            .map_err(|_| StatusCode::INSUFFICIENT_STORAGE)?;
        Ok(total)
    })
    .await;
    let size = match result {
        Ok(Ok(size)) => size,
        Ok(Err(code)) => return error(code, "Upload rejected"),
        Err(_) => return error(StatusCode::REQUEST_TIMEOUT, "Upload timed out"),
    };
    let mut d = s.data.lock().unwrap();
    if !s.alive() {
        return error(StatusCode::GONE, "Share ended");
    }
    if d.pending.len() >= MAX_PENDING {
        return error(StatusCode::TOO_MANY_REQUESTS, "Pending queue full");
    }
    d.pending.push(PendingUpload {
        view: PendingView {
            id: secret(),
            name,
            size,
        },
        file,
    });
    (
        StatusCode::ACCEPTED,
        "Received temporarily. Approve this file on the phone to keep it.",
    )
        .into_response()
}
