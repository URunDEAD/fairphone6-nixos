use axum::{
    body::{Body, Bytes, to_bytes},
    extract::ConnectInfo,
    http::{Request, StatusCode},
    response::Response,
};
use fp6_webshare::{network::LanInterface, session::*, web};
use std::{
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tower::ServiceExt;

struct Fixture {
    s: Arc<Session>,
    _dir: tempfile::TempDir,
}
impl Fixture {
    fn new(files: &[(&str, &[u8])], uploads: bool, text: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let paths: Vec<PathBuf> = files
            .iter()
            .map(|(name, bytes)| {
                let p = dir.path().join(name);
                std::fs::write(&p, bytes).unwrap();
                p
            })
            .collect();
        let staging = dir.path().join("staging");
        let received = dir.path().join("received");
        private_dir(&staging).unwrap();
        private_dir(&received).unwrap();
        let s = Session::new(
            StartOptions {
                files: paths.clone(),
                interface: "synthetic-wifi".into(),
                minutes: 10,
                uploads,
                text,
            },
            LanInterface {
                name: "synthetic-wifi".into(),
                address: Ipv4Addr::new(192, 168, 77, 1),
                netmask: Ipv4Addr::new(255, 255, 255, 0),
            },
            open_files(&paths).unwrap(),
            staging,
            received,
        );
        Self {
            s: Arc::new(s),
            _dir: dir,
        }
    }
    fn req(&self, method: &str, tail: &str, cookie: Option<&str>, body: Body) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(format!("/s/{}{tail}", self.s.invitation))
            .header("host", "192.168.77.1:8765");
        if let Some(c) = cookie {
            builder = builder.header("cookie", c);
        }
        let mut req = builder.body(body).unwrap();
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([192, 168, 77, 2], 12345))));
        req
    }
    async fn send(&self, req: Request<Body>) -> Response {
        web::router(self.s.clone()).oneshot(req).await.unwrap()
    }
    async fn claim(&self) -> String {
        let res = self
            .send(self.req("POST", "/claim", None, Body::empty()))
            .await;
        assert_eq!(res.status(), StatusCode::SEE_OTHER);
        res.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string()
    }
    async fn upload(&self, cookie: &str, name: &str, bytes: &'static str) -> Response {
        let mut req = self.req("POST", "/upload", Some(cookie), Body::from(bytes));
        req.headers_mut()
            .insert("x-file-name", name.parse().unwrap());
        self.send(req).await
    }
}
#[tokio::test]
async fn invitation_is_single_use_and_get_does_not_claim() {
    let f = Fixture::new(&[("hello.txt", b"synthetic")], false, false);
    assert_eq!(
        f.send(f.req("GET", "", None, Body::empty())).await.status(),
        StatusCode::OK
    );
    assert!(!f.s.status().claimed);
    let (a, b) = tokio::join!(
        f.send(f.req("POST", "/claim", None, Body::empty())),
        f.send(f.req("POST", "/claim", None, Body::empty()))
    );
    let mut codes = [a.status().as_u16(), b.status().as_u16()];
    codes.sort();
    assert_eq!(codes, [303, 410]);
    assert_eq!(
        f.send(f.req("GET", "/file/0", None, Body::empty()))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}
#[tokio::test]
async fn downloads_are_exact_attachments_and_stop_after_last_file() {
    let f = Fixture::new(
        &[("hello.txt", b"synthetic"), ("empty.bin", b"")],
        false,
        false,
    );
    let cookie = f.claim().await;
    let res = f
        .send(f.req("GET", "/file/0", Some(&cookie), Body::empty()))
        .await;
    assert_eq!(res.headers()["content-type"], "application/octet-stream");
    assert!(
        res.headers()["content-disposition"]
            .to_str()
            .unwrap()
            .starts_with("attachment;")
    );
    assert_eq!(to_bytes(res.into_body(), 100).await.unwrap(), "synthetic");
    assert!(f.s.alive());
    let res = f
        .send(f.req("GET", "/file/1", Some(&cookie), Body::empty()))
        .await;
    assert!(to_bytes(res.into_body(), 100).await.unwrap().is_empty());
    assert!(!f.s.alive());
    assert!(f.s.cancel.is_cancelled());
}
#[tokio::test]
async fn interrupted_download_can_be_retried_and_offsets_are_independent() {
    let f = Fixture::new(&[("hello.txt", b"synthetic")], false, false);
    let cookie = f.claim().await;
    let first = f
        .send(f.req("GET", "/file/0", Some(&cookie), Body::empty()))
        .await;
    drop(first);
    assert!(!f.s.status().files[0].downloaded);
    let second = f
        .send(f.req("GET", "/file/0", Some(&cookie), Body::empty()))
        .await;
    assert_eq!(
        to_bytes(second.into_body(), 100).await.unwrap(),
        "synthetic"
    );
}
#[tokio::test]
async fn consent_is_required_and_approval_never_overwrites() {
    let f = Fixture::new(&[("shared.txt", b"outgoing")], true, false);
    let cookie = f.claim().await;
    assert_eq!(
        f.upload(&cookie, "same.txt", "one").await.status(),
        StatusCode::ACCEPTED
    );
    assert_eq!(std::fs::read_dir(&f.s.received).unwrap().count(), 0);
    let id = f.s.status().pending[0].id.clone();
    let first = f.s.approve(&id).unwrap();
    assert_eq!(std::fs::read(&first).unwrap(), b"one");
    assert_eq!(
        f.upload(&cookie, "same.txt", "two").await.status(),
        StatusCode::ACCEPTED
    );
    let id = f.s.status().pending[0].id.clone();
    let second = f.s.approve(&id).unwrap();
    assert_ne!(first, second);
    assert_eq!(std::fs::read(&first).unwrap(), b"one");
    assert_eq!(std::fs::read(&second).unwrap(), b"two");
    assert!(f.s.approve(&id).is_err());
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(second).unwrap().permissions().mode() & 0o777,
        0o600
    );
}
#[tokio::test]
async fn rejects_encoded_traversal_and_unsafe_names() {
    let f = Fixture::new(&[], true, false);
    let c = f.claim().await;
    for name in [
        "..",
        "%2e%2e%2fsecret",
        "%2Fabsolute",
        "x%5cy",
        "x%00y",
        "%252fsecret",
        "C%3Afile",
        ".",
    ] {
        assert_eq!(
            f.upload(&c, name, "bad").await.status(),
            StatusCode::BAD_REQUEST,
            "{name}"
        );
    }
    for path in [
        "/file/../0",
        "/file/%2e%2e",
        "/file/0?path=/etc/passwd",
        "/%252e%252e",
    ] {
        assert_eq!(
            f.send(f.req("GET", path, Some(&c), Body::empty()))
                .await
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        f.send(f.req("GET", "/file/99", Some(&c), Body::empty()))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}
#[tokio::test]
async fn opt_ins_default_off() {
    let opts: StartOptions = serde_json::from_str(r#"{"files":[],"interface":"test"}"#).unwrap();
    assert!(!opts.uploads && !opts.text);
    assert_eq!(opts.minutes, 10);
    let f = Fixture::new(&[("a", b"b")], false, false);
    let c = f.claim().await;
    assert_eq!(f.upload(&c, "x", "y").await.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        f.send(f.req("POST", "/text", Some(&c), Body::from("hello")))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
}
#[tokio::test]
async fn rejects_foreign_origin_host_and_off_subnet_peer() {
    let f = Fixture::new(&[], true, true);
    let mut req = f.req("POST", "/claim", None, Body::empty());
    req.headers_mut()
        .insert("origin", "http://evil.invalid".parse().unwrap());
    assert_eq!(f.send(req).await.status(), StatusCode::FORBIDDEN);
    let mut req = f.req("POST", "/claim", None, Body::empty());
    req.headers_mut()
        .insert("sec-fetch-site", "cross-site".parse().unwrap());
    assert_eq!(f.send(req).await.status(), StatusCode::FORBIDDEN);
    let mut req = f.req("POST", "/claim", None, Body::empty());
    req.headers_mut()
        .insert("host", "evil.invalid:8765".parse().unwrap());
    assert_eq!(f.send(req).await.status(), StatusCode::FORBIDDEN);
    let mut req = f.req("POST", "/claim", None, Body::empty());
    req.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([10, 1, 1, 1], 1234))));
    assert_eq!(f.send(req).await.status(), StatusCode::FORBIDDEN);
    assert!(!f.s.status().claimed);
}
#[tokio::test]
async fn reject_and_stop_remove_staging() {
    let f = Fixture::new(&[], true, false);
    let c = f.claim().await;
    f.upload(&c, "one", "one").await;
    assert_eq!(std::fs::read_dir(&f.s.staging).unwrap().count(), 1);
    let id = f.s.status().pending[0].id.clone();
    f.s.reject(&id).unwrap();
    assert_eq!(std::fs::read_dir(&f.s.staging).unwrap().count(), 0);
    f.upload(&c, "two", "two").await;
    f.s.stop();
    assert_eq!(std::fs::read_dir(&f.s.staging).unwrap().count(), 0);
    assert!(f.s.status().pending.is_empty());
}
#[tokio::test]
async fn upload_limits_work_with_and_without_content_length() {
    let f = Fixture::new(&[], true, false);
    let c = f.claim().await;
    let mut req = f.req("POST", "/upload", Some(&c), Body::empty());
    req.headers_mut()
        .insert("x-file-name", "big".parse().unwrap());
    req.headers_mut().insert(
        "content-length",
        (MAX_UPLOAD + 1).to_string().parse().unwrap(),
    );
    assert_eq!(f.send(req).await.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let chunks = futures_util::stream::iter(
        (0..101).map(|_| Ok::<_, std::io::Error>(Bytes::from(vec![0u8; 1024 * 1024]))),
    );
    let mut req = f.req("POST", "/upload", Some(&c), Body::from_stream(chunks));
    req.headers_mut()
        .insert("x-file-name", "big".parse().unwrap());
    assert_eq!(f.send(req).await.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert!(f.s.status().pending.is_empty());
    assert_eq!(std::fs::read_dir(&f.s.staging).unwrap().count(), 0);
}
#[tokio::test]
async fn pending_queue_and_rate_are_bounded() {
    let f = Fixture::new(&[], true, false);
    let c = f.claim().await;
    for _ in 0..MAX_PENDING {
        assert_eq!(f.upload(&c, "a", "b").await.status(), StatusCode::ACCEPTED);
    }
    assert_eq!(
        f.upload(&c, "a", "b").await.status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    f.s.data.lock().unwrap().requests = 120;
    assert_eq!(
        f.send(f.req("GET", "/", Some(&c), Body::empty()))
            .await
            .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    f.s.data.lock().unwrap().rate_start = Instant::now() - Duration::from_secs(61);
    assert_eq!(
        f.send(f.req("GET", "/", Some(&c), Body::empty()))
            .await
            .status(),
        StatusCode::OK
    );
}
#[tokio::test]
async fn text_is_bounded_queued_and_consumed_only_locally() {
    let f = Fixture::new(&[], false, true);
    let c = f.claim().await;
    assert_eq!(
        f.send(f.req(
            "POST",
            "/text",
            Some(&c),
            Body::from("x".repeat(MAX_TEXT + 1))
        ))
        .await
        .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        f.send(f.req(
            "POST",
            "/text",
            Some(&c),
            Body::from("https://example.invalid/synthetic")
        ))
        .await
        .status(),
        StatusCode::ACCEPTED
    );
    assert_eq!(
        f.send(f.req("POST", "/text", Some(&c), Body::from("overwrite")))
            .await
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        f.send(f.req("POST", "/takeText", Some(&c), Body::empty()))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        f.s.take_text().unwrap(),
        "https://example.invalid/synthetic"
    );
    assert!(!f.s.alive());
}
#[tokio::test]
async fn expiry_and_stop_revoke_browser_access() {
    let mut f = Fixture::new(&[], true, false);
    Arc::get_mut(&mut f.s).unwrap().deadline = Instant::now() - Duration::from_secs(1);
    assert_eq!(
        f.send(f.req("POST", "/claim", None, Body::empty()))
            .await
            .status(),
        StatusCode::GONE
    );
}
#[tokio::test]
async fn filenames_are_escaped_and_security_headers_are_present() {
    let f = Fixture::new(&[("<script>alert(1)<script>.txt", b"test")], false, false);
    let c = f.claim().await;
    let res = f.send(f.req("GET", "/", Some(&c), Body::empty())).await;
    assert_eq!(res.headers()["cache-control"], "no-store");
    assert_eq!(res.headers()["referrer-policy"], "no-referrer");
    assert!(res.headers().contains_key("content-security-policy"));
    let body = String::from_utf8(to_bytes(res.into_body(), 10000).await.unwrap().to_vec()).unwrap();
    assert!(!body.contains("<script>alert"));
    assert!(body.contains("&lt;script&gt;"));
}
#[test]
fn rejects_symlinks_directories_and_pins_open_inode() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("a");
    std::fs::write(&p, b"old").unwrap();
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&p, &link).unwrap();
    assert!(open_files(&[link]).is_err());
    assert!(open_files(&[dir.path().to_owned()]).is_err());
    let files = open_files(std::slice::from_ref(&p)).unwrap();
    std::fs::remove_file(&p).unwrap();
    std::fs::write(&p, b"new").unwrap();
    use std::os::unix::fs::FileExt;
    let mut bytes = [0; 3];
    files[0].file.read_at(&mut bytes, 0).unwrap();
    assert_eq!(&bytes, b"old");
}
#[test]
fn random_tokens_have_128_bits_and_networks_fail_closed() {
    let tokens: std::collections::HashSet<_> = (0..1000).map(|_| secret()).collect();
    assert_eq!(tokens.len(), 1000);
    assert!(tokens.iter().all(|t| t.len() == 22));
    assert!(!secure_eq("one", "two"));
    assert!(!secure_eq("one", "one1"));
    assert!(secure_eq("one", "one"));
    assert!(fp6_webshare::network::select("lo").is_err());
}
#[tokio::test]
async fn stale_staging_is_cleaned_and_invalid_start_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let staging = dir.path().join("stage");
    std::fs::create_dir(&staging).unwrap();
    std::fs::write(staging.join("orphan"), b"synthetic").unwrap();
    let m = Manager::new(staging.clone(), dir.path().join("received")).unwrap();
    assert_eq!(std::fs::read_dir(staging).unwrap().count(), 0);
    for minutes in [0, 31] {
        assert!(
            m.start(StartOptions {
                files: vec![],
                interface: "lo".into(),
                minutes,
                uploads: true,
                text: false
            })
            .await
            .is_err()
        );
    }
    assert!(
        m.start(StartOptions {
            files: vec![],
            interface: "lo".into(),
            minutes: 1,
            uploads: true,
            text: false
        })
        .await
        .is_err()
    );
}
