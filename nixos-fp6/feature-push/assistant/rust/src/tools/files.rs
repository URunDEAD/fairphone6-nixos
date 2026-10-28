//! Files, scoped to the folders the user grants (files.read:<dir>,
//! files.write:<dir>). Paths are resolved (~, .., symlinks) before the
//! scope check; protected locations are never granted.

use super::{arg_str, opt_u64, schema, spec, Confirm, Ctx, Output, Tool, ToolSpec};
use crate::llm::Part;
use crate::permissions::{display_path, forbidden_path, resolve_path, Resource};
use crate::settings::Settings;
use async_trait::async_trait;
use base64::Engine;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MAX_TEXT: u64 = 256 * 1024;
const MAX_IMAGE: u64 = 8 * 1024 * 1024;

pub fn tools() -> Vec<Arc<dyn Tool>> {
    vec![Arc::new(List), Arc::new(ReadFile), Arc::new(Find), Arc::new(Write), Arc::new(Delete)]
}

pub fn image_mime(p: &Path) -> Option<&'static str> {
    match p.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        _ => None,
    }
}

fn is_texty(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes).is_ok() && !bytes.contains(&0)
}

/// The part a user attachment becomes in the model request.
pub fn attachment_part(p: &Path) -> Result<Part, String> {
    let meta = std::fs::metadata(p).map_err(|e| format!("{}: {e}", display_path(&p.to_string_lossy())))?;
    if let Some(mime) = image_mime(p) {
        if meta.len() > MAX_IMAGE {
            return Err("the image is too large to attach (8 MB max)".into());
        }
        let b = std::fs::read(p).map_err(|e| e.to_string())?;
        return Ok(Part::Image { mime: mime.into(), data: base64::engine::general_purpose::STANDARD.encode(b) });
    }
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    if meta.len() <= MAX_TEXT {
        let b = std::fs::read(p).map_err(|e| e.to_string())?;
        if is_texty(&b) {
            return Ok(Part::Text { text: format!("\n[Attached file {name}]\n{}\n[End of {name}]", String::from_utf8_lossy(&b)) });
        }
    }
    Ok(Part::Text { text: format!("\n[Attached file {name}: {} bytes, not text; path {}]", meta.len(), p.display()) })
}

fn dir_scope(p: &Path) -> String {
    if p.is_dir() {
        p.to_string_lossy().into()
    } else {
        p.parent().unwrap_or(Path::new("/")).to_string_lossy().into()
    }
}

fn path_arg(a: &Value) -> Result<PathBuf, String> {
    let p = resolve_path(arg_str(a, "path")?);
    if let Some(why) = forbidden_path(&p) {
        return Err(format!("{} is not accessible ({why})", display_path(&p.to_string_lossy())));
    }
    Ok(p)
}

struct List;
#[async_trait]
impl Tool for List {
    fn spec(&self) -> ToolSpec {
        spec("files_list", "Files", "List a folder", "List files in a folder (\"~\" is the home folder, e.g. ~/Documents, ~/Pictures/Screenshots, ~/Downloads).", schema(&[("path", "string", "Folder", true)]), false, true)
    }
    fn resources(&self, a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        let p = path_arg(a)?;
        Ok(vec![Resource::scoped("files.read", p.to_string_lossy())])
    }
    async fn run(&self, a: Value, _ctx: &Ctx) -> Result<Output, String> {
        let p = path_arg(&a)?;
        let mut rows = Vec::new();
        let rd = std::fs::read_dir(&p).map_err(|e| format!("{}: {e}", display_path(&p.to_string_lossy())))?;
        for e in rd.flatten().take(500) {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let m = e.metadata().ok();
            rows.push(json!({
                "name": name,
                "dir": m.as_ref().map(|m| m.is_dir()).unwrap_or(false),
                "size": m.as_ref().map(|m| m.len()).unwrap_or(0),
                "modified": m.and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()),
            }));
        }
        rows.sort_by(|a, b| b["modified"].as_u64().cmp(&a["modified"].as_u64()));
        Ok(Output::json(&json!({"folder": display_path(&p.to_string_lossy()), "entries": rows})))
    }
}

struct ReadFile;
#[async_trait]
impl Tool for ReadFile {
    fn spec(&self) -> ToolSpec {
        spec("files_read", "Files", "Read a file", "Read a text file, or look at an image file (png/jpeg/webp).", schema(&[("path", "string", "File path", true)]), false, true)
    }
    fn resources(&self, a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        let p = path_arg(a)?;
        Ok(vec![Resource::scoped("files.read", dir_scope(&p))])
    }
    async fn run(&self, a: Value, _ctx: &Ctx) -> Result<Output, String> {
        let p = path_arg(&a)?;
        match attachment_part(&p)? {
            Part::Image { mime, data } => Ok(Output { text: format!("Image {} attached.", display_path(&p.to_string_lossy())), image: Some((mime, data)), files: vec![] }),
            Part::Text { text } => Ok(Output::text(text)),
            _ => Err("unreadable".into()),
        }
    }
}

struct Find;
#[async_trait]
impl Tool for Find {
    fn spec(&self) -> ToolSpec {
        spec(
            "files_find",
            "Files",
            "Find files",
            "Find files by name below a folder (case-insensitive substring, up to 4 levels deep).",
            schema(&[("path", "string", "Folder to search", true), ("name", "string", "Part of the file name", true), ("limit", "integer", "Max results (default 30)", false)]),
            false,
            true,
        )
    }
    fn resources(&self, a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        let p = path_arg(a)?;
        Ok(vec![Resource::scoped("files.read", p.to_string_lossy())])
    }
    async fn run(&self, a: Value, _ctx: &Ctx) -> Result<Output, String> {
        let root = path_arg(&a)?;
        let needle = arg_str(&a, "name")?.to_lowercase();
        let limit = opt_u64(&a, "limit", 30, 200) as usize;
        let mut out = Vec::new();
        let mut stack = vec![(root.clone(), 0)];
        while let Some((d, depth)) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with('.') {
                    continue;
                }
                let ft = e.file_type().ok();
                // Never follow symlinks out of the granted folder.
                if ft.map(|t| t.is_symlink()).unwrap_or(true) {
                    continue;
                }
                if name.to_lowercase().contains(&needle) {
                    out.push(display_path(&e.path().to_string_lossy()));
                    if out.len() >= limit {
                        return Ok(Output::json(&json!(out)));
                    }
                }
                if ft.map(|t| t.is_dir()).unwrap_or(false) && depth < 4 {
                    stack.push((e.path(), depth + 1));
                }
            }
        }
        Ok(Output::json(&json!(out)))
    }
}

struct Write;
#[async_trait]
impl Tool for Write {
    fn spec(&self) -> ToolSpec {
        spec(
            "files_write",
            "Files",
            "Write a file",
            "Create or overwrite a text file (or append to it). The user confirms first.",
            schema(&[("path", "string", "File path", true), ("content", "string", "Text to write", true), ("append", "boolean", "Append instead of overwrite", false)]),
            true,
            false,
        )
    }
    fn resources(&self, a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        let p = path_arg(a)?;
        if p.is_dir() {
            return Err("that is a folder".into());
        }
        Ok(vec![Resource::scoped("files.write", dir_scope(&p))])
    }
    fn confirm(&self, a: &Value) -> Option<Confirm> {
        let p = resolve_path(a.get("path")?.as_str()?);
        let append = a.get("append").and_then(Value::as_bool).unwrap_or(false);
        let verb = if append { "Append to" } else if p.exists() { "Overwrite" } else { "Create" };
        Some(Confirm {
            title: format!("{verb} this file?"),
            fields: vec![("File".into(), display_path(&p.to_string_lossy())), ("Content".into(), crate::llm::truncate(a.get("content")?.as_str()?, 4000))],
            action: if append { "Append".into() } else { "Save".into() },
            destructive: verb == "Overwrite",
        })
    }
    async fn run(&self, a: Value, _ctx: &Ctx) -> Result<Output, String> {
        use std::io::Write as _;
        let p = path_arg(&a)?;
        let content = arg_str(&a, "content")?;
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let append = a.get("append").and_then(Value::as_bool).unwrap_or(false);
        let mut f = std::fs::OpenOptions::new().create(true).write(true).append(append).truncate(!append).open(&p).map_err(|e| e.to_string())?;
        f.write_all(content.as_bytes()).map_err(|e| e.to_string())?;
        Ok(Output { text: format!("Saved {}.", display_path(&p.to_string_lossy())), image: None, files: vec![p.to_string_lossy().into()] })
    }
}

struct Delete;
#[async_trait]
impl Tool for Delete {
    fn spec(&self) -> ToolSpec {
        spec("files_delete", "Files", "Delete a file", "Move a file to the trash (~/.local/share/Trash). The user confirms first.", schema(&[("path", "string", "File path", true)]), true, false)
    }
    fn resources(&self, a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        let p = path_arg(a)?;
        if !p.is_file() {
            return Err("only single files can be deleted".into());
        }
        Ok(vec![Resource::scoped("files.write", dir_scope(&p))])
    }
    fn confirm(&self, a: &Value) -> Option<Confirm> {
        let p = resolve_path(a.get("path")?.as_str()?);
        Some(Confirm { title: "Move this file to the trash?".into(), fields: vec![("File".into(), display_path(&p.to_string_lossy()))], action: "Delete".into(), destructive: true })
    }
    async fn run(&self, a: Value, _ctx: &Ctx) -> Result<Output, String> {
        let p = path_arg(&a)?;
        let to = trash(&p)?;
        Ok(Output::text(format!("Moved {} to the trash ({}).", display_path(&p.to_string_lossy()), display_path(&to.to_string_lossy()))))
    }
}

/// freedesktop trash: files/<name> + info/<name>.trashinfo.
pub fn trash(p: &Path) -> Result<PathBuf, String> {
    let home = PathBuf::from(std::env::var("HOME").map_err(|e| e.to_string())?);
    let base = home.join(".local/share/Trash");
    std::fs::create_dir_all(base.join("files")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(base.join("info")).map_err(|e| e.to_string())?;
    let name = p.file_name().ok_or("no file name")?.to_string_lossy().to_string();
    let mut target = base.join("files").join(&name);
    let mut n = 1;
    while target.exists() {
        target = base.join("files").join(format!("{name}.{n}"));
        n += 1;
    }
    let tname = target.file_name().unwrap().to_string_lossy().to_string();
    let info = format!("[Trash Info]\nPath={}\nDeletionDate={}\n", p.display(), crate::agent::chrono_like_now().get(..16).unwrap_or("").replace(' ', "T"));
    std::fs::write(base.join("info").join(format!("{tname}.trashinfo")), info).map_err(|e| e.to_string())?;
    std::fs::rename(p, &target).or_else(|_| std::fs::copy(p, &target).and_then(|_| std::fs::remove_file(p))).map_err(|e| e.to_string())?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_from_paths() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("a.txt");
        std::fs::write(&f, "hello").unwrap();
        let r = ReadFile.resources(&json!({"path": f.to_str().unwrap()}), &Settings::default());
        // tempdir is outside $HOME in the sandbox: refused, or scoped to the folder
        match r {
            Ok(v) => assert_eq!(v[0].scope.as_deref(), Some(resolve_path(d.path().to_str().unwrap()).to_str().unwrap())),
            Err(e) => assert!(e.contains("not accessible")),
        }
    }

    #[test]
    fn attachments() {
        let d = tempfile::tempdir().unwrap();
        let t = d.path().join("note.txt");
        std::fs::write(&t, "buy milk").unwrap();
        assert!(matches!(attachment_part(&t).unwrap(), Part::Text { text } if text.contains("buy milk")));
        let i = d.path().join("x.png");
        std::fs::write(&i, [0x89, b'P', b'N', b'G']).unwrap();
        assert!(matches!(attachment_part(&i).unwrap(), Part::Image { mime, .. } if mime == "image/png"));
    }
}
