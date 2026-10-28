//! Screenshots, camera, text recognition, sharing.
//!
//!  * Screenshots: Lomiri hands out pixels to nobody (screen-capture
//!    design: no D-Bus method, socket or Wayland global). So the assistant
//!    either uses the newest screenshot in ~/Pictures/Screenshots, or asks
//!    the user to take one (Power + Volume Down) and waits for the file.
//!  * Camera: the Assistant UI requests a photo from the camera app over
//!    Content Hub (the user presses the shutter).
//!  * OCR: com.fairphone6.Ocr1 (offline, English/Romanian).
//!  * Share: the share sheet (com.fairphone6.Share1) and KDE Connect
//!    (com.fairphone6.KdeConnect1).

use super::files::image_mime;
use super::{arg_str, opt_str, opt_u64, schema, spec, with_enum, Confirm, Ctx, Output, Tool, ToolSpec};
use crate::dbusutil::call;
use crate::permissions::{display_path, forbidden_path, resolve_path, Resource};
use crate::settings::Settings;
use async_trait::async_trait;
use base64::Engine;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub fn tools() -> Vec<Arc<dyn Tool>> {
    vec![Arc::new(LatestScreenshot), Arc::new(TakeScreenshot), Arc::new(CameraPhoto), Arc::new(Ocr), Arc::new(Share), Arc::new(KdeDevices), Arc::new(KdeSend)]
}

pub fn screenshot_dir() -> PathBuf {
    if let Ok(d) = std::env::var("FP6_ASSISTANT_SCREENSHOT_DIR") {
        return d.into();
    }
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Pictures/Screenshots")
}

pub fn newest_image(dir: &Path) -> Option<(PathBuf, std::time::SystemTime)> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| image_mime(&e.path()).is_some())
        .filter_map(|e| Some((e.path(), e.metadata().ok()?.modified().ok()?)))
        .max_by_key(|(_, t)| *t)
}

fn image_output(p: &Path, note: &str) -> Result<Output, String> {
    let mime = image_mime(p).ok_or("not an image")?;
    let b = std::fs::read(p).map_err(|e| e.to_string())?;
    Ok(Output {
        text: format!("{note} {} ({} KB). The image is attached for you to look at; use ocr_image to read its text exactly.", display_path(&p.to_string_lossy()), b.len() / 1024),
        image: Some((mime.into(), base64::engine::general_purpose::STANDARD.encode(b))),
        files: vec![p.to_string_lossy().into()],
    })
}

struct LatestScreenshot;
#[async_trait]
impl Tool for LatestScreenshot {
    fn spec(&self) -> ToolSpec {
        spec("screenshot_latest", "Screenshots", "Latest screenshot", "Get the most recent screenshot the user took (image + path).", schema(&[]), false, true)
    }
    fn resources(&self, _a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        Ok(vec![Resource::new("screenshot")])
    }
    async fn run(&self, _a: Value, _ctx: &Ctx) -> Result<Output, String> {
        let (p, _) = newest_image(&screenshot_dir()).ok_or("There are no screenshots yet.")?;
        image_output(&p, "Latest screenshot:")
    }
}

struct TakeScreenshot;
#[async_trait]
impl Tool for TakeScreenshot {
    fn spec(&self) -> ToolSpec {
        spec(
            "screenshot_take",
            "Screenshots",
            "Take a screenshot",
            "Ask the user to take a screenshot of what is on screen (they switch to the app and press Power + Volume Down); waits up to 90 s for it.",
            schema(&[("reason", "string", "What you want to see, shown to the user", false)]),
            false,
            true,
        )
    }
    fn resources(&self, _a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        Ok(vec![Resource::new("screenshot")])
    }
    async fn run(&self, a: Value, ctx: &Ctx) -> Result<Output, String> {
        let dir = screenshot_dir();
        let before = newest_image(&dir).map(|(_, t)| t);
        let ui = ctx.ui.clone();
        let reason = opt_str(&a, "reason").unwrap_or("").to_string();
        let chat = ctx.chat_id.clone();
        // The UI shows a hint banner; cancelling it stops the wait.
        let hint = tokio::spawn(async move {
            ui.ask(json!({"kind": "ui", "op": "screenshot_hint", "args": {"reason": reason}, "chat": chat}), std::time::Duration::from_secs(90)).await
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
        loop {
            if let Some((p, t)) = newest_image(&dir) {
                if Some(t) != before {
                    hint.abort();
                    // let the shell finish writing the PNG
                    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
                    return image_output(&p, "New screenshot:");
                }
            }
            if hint.is_finished() {
                let ans = hint.await.unwrap_or(Value::Null);
                if ans.get("ok").and_then(Value::as_bool) != Some(true) {
                    return Err("The user cancelled the screenshot.".into());
                }
                return Err("No screenshot was taken.".into());
            }
            if std::time::Instant::now() > deadline {
                hint.abort();
                return Err("No screenshot was taken within 90 seconds.".into());
            }
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        }
    }
}

struct CameraPhoto;
#[async_trait]
impl Tool for CameraPhoto {
    fn spec(&self) -> ToolSpec {
        spec("camera_photo", "Camera", "Take a photo", "Open the camera so the user can take a photo; the photo is returned to you.", schema(&[("reason", "string", "What to photograph, shown to the user", false)]), false, true)
    }
    fn resources(&self, _a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        Ok(vec![Resource::new("camera")])
    }
    async fn run(&self, a: Value, ctx: &Ctx) -> Result<Output, String> {
        let r = ctx.ui_op("camera", json!({"reason": opt_str(&a, "reason").unwrap_or("")}), 300).await?;
        let path = r.get("path").and_then(Value::as_str).ok_or("no photo")?;
        let p = resolve_path(path.trim_start_matches("file://"));
        image_output(&p, "Photo:")
    }
}

fn image_arg(a: &Value) -> Result<PathBuf, String> {
    let p = resolve_path(arg_str(a, "path")?);
    if let Some(why) = forbidden_path(&p) {
        return Err(format!("not accessible ({why})"));
    }
    if image_mime(&p).is_none() {
        return Err("only png/jpeg/webp images".into());
    }
    Ok(p)
}

struct Ocr;
#[async_trait]
impl Tool for Ocr {
    fn spec(&self) -> ToolSpec {
        spec(
            "ocr_image",
            "Text recognition",
            "Read text in an image",
            "Recognise the text in an image file on the phone (offline; English and Romanian). Use \"latest_screenshot\" as path for the newest screenshot.",
            schema(&[("path", "string", "Image path or latest_screenshot", true), ("languages", "string", "eng, ron or eng+ron (default)", false)]),
            false,
            true,
        )
    }
    fn resources(&self, a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        if arg_str(a, "path")? == "latest_screenshot" {
            return Ok(vec![Resource::new("ocr"), Resource::new("screenshot")]);
        }
        let p = image_arg(a)?;
        let dir = p.parent().unwrap_or(Path::new("/")).to_string_lossy().to_string();
        Ok(vec![Resource::new("ocr"), Resource::scoped("files.read", dir)])
    }
    async fn run(&self, a: Value, ctx: &Ctx) -> Result<Output, String> {
        let conn = ctx.session()?;
        let path = if arg_str(&a, "path")? == "latest_screenshot" {
            newest_image(&screenshot_dir()).ok_or("There are no screenshots yet.")?.0
        } else {
            image_arg(&a)?
        };
        let langs = match opt_str(&a, "languages") {
            Some(l @ ("eng" | "ron" | "eng+ron" | "ron+eng")) => l,
            _ => "eng+ron",
        };
        let r = call(conn, "com.fairphone6.Ocr1", "/com/fairphone6/Ocr1", "com.fairphone6.Ocr1", "Recognize", &(path.to_string_lossy().as_ref(), langs)).await?;
        let job: String = r.body().deserialize().map_err(|e| e.to_string())?;
        for _ in 0..240 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let r = call(conn, "com.fairphone6.Ocr1", "/com/fairphone6/Ocr1", "com.fairphone6.Ocr1", "GetJob", &(job.as_str(),)).await?;
            let s: String = r.body().deserialize().map_err(|e| e.to_string())?;
            let v: Value = serde_json::from_str(&s).map_err(|e| e.to_string())?;
            match v["status"].as_str() {
                Some("done") => {
                    let text = v["result"]["text"].as_str().unwrap_or("").trim().to_string();
                    return Ok(Output::text(if text.is_empty() { "No text found.".to_string() } else { text }));
                }
                Some("failed") | Some("canceled") => return Err(format!("text recognition failed: {}", v["error"].as_str().unwrap_or(""))),
                _ => {}
            }
        }
        Err("text recognition timed out".into())
    }
}

struct Share;
#[async_trait]
impl Tool for Share {
    fn spec(&self) -> ToolSpec {
        spec(
            "share_open",
            "Share",
            "Open the share sheet",
            "Open the phone's share sheet with a file or a text/link; the user picks where it goes (Quick Share, KDE Connect, apps...).",
            schema(&[("path", "string", "File to share", false), ("text", "string", "Text or link to share", false)]),
            false,
            false,
        )
    }
    fn resources(&self, a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        let mut r = vec![Resource::new("share")];
        match (opt_str(a, "path"), opt_str(a, "text")) {
            (Some(p), None) => {
                let p = resolve_path(p);
                if let Some(why) = forbidden_path(&p) {
                    return Err(format!("not accessible ({why})"));
                }
                r.push(Resource::scoped("files.read", p.parent().unwrap_or(Path::new("/")).to_string_lossy()));
            }
            (None, Some(_)) => {}
            _ => return Err("give either a path or a text".into()),
        }
        Ok(r)
    }
    async fn run(&self, a: Value, ctx: &Ctx) -> Result<Output, String> {
        let conn = ctx.session()?;
        let (items, mime) = if let Some(p) = opt_str(&a, "path") {
            let p = resolve_path(p);
            let mime = image_mime(&p).unwrap_or("application/octet-stream");
            (vec![format!("file://{}", p.display())], mime.to_string())
        } else {
            (vec![arg_str(&a, "text")?.to_string()], "text/plain".to_string())
        };
        call(conn, "com.fairphone6.Share1", "/com/fairphone6/Share1", "com.fairphone6.Share1", "Share", &(items, mime.as_str(), "com.fairphone6.Assistant")).await?;
        Ok(Output::text("The share sheet is open; the user chooses where to send it."))
    }
}

const KDE: (&str, &str, &str) = ("com.fairphone6.KdeConnect1", "/com/fairphone6/KdeConnect1", "com.fairphone6.KdeConnect1");

struct KdeDevices;
#[async_trait]
impl Tool for KdeDevices {
    fn spec(&self) -> ToolSpec {
        spec("kdeconnect_devices", "Share", "KDE Connect devices", "List paired KDE Connect devices (computers, tablets) and whether they are online.", schema(&[]), false, false)
    }
    fn resources(&self, _a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        Ok(vec![Resource::new("share")])
    }
    async fn run(&self, _a: Value, ctx: &Ctx) -> Result<Output, String> {
        let r = call(ctx.session()?, KDE.0, KDE.1, KDE.2, "Devices", &()).await?;
        let s: String = r.body().deserialize().map_err(|e| e.to_string())?;
        let v: Value = serde_json::from_str(&s).unwrap_or_default();
        let list: Vec<Value> = v["devices"]
            .as_array()
            .map(|a| a.iter().filter(|d| d["paired"] == true).map(|d| json!({"id": d["id"], "name": d["name"], "online": d["online"]})).collect())
            .unwrap_or_default();
        Ok(Output::json(&json!(list)))
    }
}

struct KdeSend;
#[async_trait]
impl Tool for KdeSend {
    fn spec(&self) -> ToolSpec {
        spec(
            "kdeconnect_send",
            "Share",
            "Send to a KDE Connect device",
            "Send a file or a link to a paired KDE Connect device, or make it ring (to find it).",
            with_enum(schema(&[("device", "string", "Device id from kdeconnect_devices", true), ("action", "string", "share or ring", true), ("target", "string", "File path or http(s) link (for share)", false)]), "action", &["share", "ring"]),
            true,
            false,
        )
    }
    fn resources(&self, a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        let mut r = vec![Resource::new("share")];
        if arg_str(a, "action")? == "share" {
            let t = arg_str(a, "target")?;
            if !(t.starts_with("https://") || t.starts_with("http://")) {
                let p = resolve_path(t);
                if let Some(why) = forbidden_path(&p) {
                    return Err(format!("not accessible ({why})"));
                }
                r.push(Resource::scoped("files.read", p.parent().unwrap_or(Path::new("/")).to_string_lossy()));
            }
        }
        Ok(r)
    }
    fn confirm(&self, a: &Value) -> Option<Confirm> {
        let action = opt_str(a, "action")?;
        let mut fields = vec![("Device".to_string(), opt_str(a, "device")?.to_string())];
        if action == "share" {
            fields.push(("Send".into(), opt_str(a, "target")?.into()));
        }
        Some(Confirm { title: if action == "ring" { "Ring this device?".into() } else { "Send to this device?".into() }, fields, action: if action == "ring" { "Ring".into() } else { "Send".into() }, destructive: false })
    }
    async fn run(&self, a: Value, ctx: &Ctx) -> Result<Output, String> {
        let dev = arg_str(&a, "device")?;
        let action = arg_str(&a, "action")?;
        let data = if action == "share" {
            let t = arg_str(&a, "target")?;
            if t.starts_with("http") { t.to_string() } else { format!("file://{}", resolve_path(t).display()) }
        } else {
            String::new()
        };
        call(ctx.session()?, KDE.0, KDE.1, KDE.2, "Action", &(dev, action, data.as_str())).await?;
        Ok(Output::text(format!("Done ({action}).")))
    }
}

#[allow(dead_code)]
fn _limits(a: &Value) -> u64 {
    opt_u64(a, "x", 0, 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn newest_image_picks_latest() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("screenshot1.png"), "a").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(d.path().join("screenshot2.png"), "b").unwrap();
        std::fs::write(d.path().join("notes.txt"), "c").unwrap();
        assert!(newest_image(d.path()).unwrap().0.ends_with("screenshot2.png"));
    }
}
