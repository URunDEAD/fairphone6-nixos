use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use url::Url;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Request {
    pub items: Vec<String>,
    pub mime: String,
    pub source_app: String,
}
impl Request {
    pub fn new(items: Vec<String>, mime: String, source_app: String) -> Result<Self> {
        ensure!(!items.is_empty() && items.len() <= 32, "Share 1–32 items");
        ensure!(
            items.iter().map(String::len).sum::<usize>() <= 128 * 1024,
            "Payload exceeds 128 KiB"
        );
        ensure!(
            source_app.len() <= 256 && mime.len() <= 128,
            "Metadata too long"
        );
        ensure!(
            mime.contains('/') && !mime.chars().any(char::is_control),
            "Invalid MIME type"
        );
        let mut normalized = Vec::new();
        for item in items {
            if mime.starts_with("text/") && !item.starts_with("file:") {
                ensure!(!item.contains('\0'), "Text contains NUL");
                normalized.push(item);
                continue;
            }
            let uri = Url::parse(&item).context("Expected a file URI or text MIME")?;
            ensure!(
                uri.scheme() == "file" && uri.host_str().is_none_or(|h| h == "localhost"),
                "Only local file URIs are accepted"
            );
            let path = uri
                .to_file_path()
                .map_err(|_| anyhow::anyhow!("Invalid file URI"))?;
            let path = path.canonicalize().context("File is unavailable")?;
            ensure!(path.is_file(), "Only regular files can be shared");
            normalized.push(Url::from_file_path(path).unwrap().into());
        }
        let files = normalized.iter().filter(|s| s.starts_with("file:")).count();
        ensure!(
            files == 0 || files == normalized.len(),
            "Mixed files and text are unsupported"
        );
        Ok(Self {
            items: normalized,
            mime,
            source_app,
        })
    }
    pub fn files(&self) -> Result<Vec<PathBuf>> {
        self.items
            .iter()
            .map(|s| {
                Url::parse(s)?
                    .to_file_path()
                    .map_err(|_| anyhow::anyhow!("This action requires local files"))
            })
            .collect()
    }
    pub fn is_files(&self) -> bool {
        self.items.iter().all(|s| s.starts_with("file:"))
    }
    pub fn is_link(&self) -> bool {
        !self.is_files()
            && self.items.len() == 1
            && Url::parse(&self.items[0]).is_ok_and(|u| matches!(u.scheme(), "http" | "https"))
    }
    pub fn content_type(&self) -> &'static str {
        if self.mime == "text/vcard" {
            "contacts"
        } else if self.mime == "text/calendar" {
            "events"
        } else if self.mime == "application/epub+zip" {
            "ebooks"
        } else if self.mime.starts_with("image/") {
            "pictures"
        } else if self.mime.starts_with("audio/") {
            "music"
        } else if self.mime.starts_with("video/") {
            "videos"
        } else if self.is_link() {
            "links"
        } else if self.mime.starts_with("text/") {
            "text"
        } else {
            "documents"
        }
    }
    pub fn from_cli(args: &[String]) -> Result<Self> {
        ensure!(
            !args.is_empty(),
            "Usage: fp6-share FILE... | --text TEXT | --url URL | share:?text=..."
        );
        if args[0] == "--text" || args[0] == "--url" {
            ensure!(args.len() == 2, "Expected exactly one value");
            ensure!(
                !args[1].starts_with("file:"),
                "Use file arguments to share files; file: text is ambiguous"
            );
            if args[0] == "--url" {
                let u = Url::parse(&args[1])?;
                ensure!(
                    matches!(u.scheme(), "http" | "https"),
                    "Expected HTTP(S) URL"
                );
            }
            return Self::new(vec![args[1].clone()], "text/plain".into(), "CLI".into());
        }
        if args.len() == 1 && args[0].starts_with("share:") {
            return Self::from_share_uri(&args[0]);
        }
        let mut items = Vec::new();
        for arg in args {
            let path = if arg.starts_with("file:") {
                Url::parse(arg)?
                    .to_file_path()
                    .map_err(|_| anyhow::anyhow!("Invalid file URI"))?
            } else {
                Path::new(arg).canonicalize()?
            };
            items.push(
                Url::from_file_path(path)
                    .map_err(|_| anyhow::anyhow!("Expected absolute path"))?
                    .to_string(),
            );
        }
        let mime = if items.len() == 1 {
            mime_for(&items[0])
        } else {
            "application/octet-stream"
        };
        Self::new(items, mime.into(), "CLI".into())
    }
    pub fn from_share_uri(value: &str) -> Result<Self> {
        ensure!(value.len() <= 192 * 1024, "Share URI too long");
        let uri = Url::parse(value)?;
        ensure!(
            uri.scheme() == "share"
                && uri.host_str().is_none()
                && uri.path().is_empty()
                && uri.fragment().is_none(),
            "Use share:?text=... or share:?url=..."
        );
        let query: Vec<_> = uri.query_pairs().collect();
        ensure!(query.len() == 1, "Expected one text or url parameter");
        let (key, value) = &query[0];
        ensure!(
            key == "text" || key == "url",
            "Only text and url parameters are supported; use OpenFile for local files"
        );
        ensure!(
            !value.starts_with("file:"),
            "Share URI cannot request local files"
        );
        if key == "url" {
            let target = Url::parse(value)?;
            ensure!(
                matches!(target.scheme(), "http" | "https"),
                "Expected HTTP(S) link"
            );
        }
        Self::new(
            vec![value.to_string()],
            "text/plain".into(),
            "share URI".into(),
        )
    }
}
pub fn mime_for(path: &str) -> &'static str {
    let path = path.to_ascii_lowercase();
    match path.rsplit('.').next().unwrap_or("") {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "vcf" => "text/vcard",
        "ics" => "text/calendar",
        "epub" => "application/epub+zip",
        "txt" | "md" => "text/plain",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        _ => "application/octet-stream",
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Target {
    pub id: String,
    pub provider: String,
    pub label: String,
    pub detail: String,
    pub direct: bool,
}
impl Target {
    pub fn new(provider: &str, id: &str, label: &str, detail: &str, direct: bool) -> Self {
        Self {
            id: format!("{provider}:{id}"),
            provider: provider.into(),
            label: label.into(),
            detail: detail.into(),
            direct,
        }
    }
    pub fn local_id(&self) -> Result<&str> {
        self.id
            .strip_prefix(&format!("{}:", self.provider))
            .ok_or_else(|| anyhow::anyhow!("Invalid target"))
    }
}
pub fn require_single(request: &Request) -> Result<()> {
    if request.items.len() != 1 {
        bail!("Select one item for this action");
    }
    Ok(())
}
