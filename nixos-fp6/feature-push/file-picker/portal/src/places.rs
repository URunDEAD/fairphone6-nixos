//! Places (XDG user folders, SD card / USB mounts) and the Recent list.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// XDG user directory (`~/.config/user-dirs.dirs`), with the usual default.
pub fn user_dir(key: &str, default: &str) -> PathBuf {
    let home = home();
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    if let Ok(text) = std::fs::read_to_string(config.join("user-dirs.dirs")) {
        if let Some(p) = parse_user_dirs(&text, key, &home) {
            return p;
        }
    }
    home.join(default)
}

pub fn parse_user_dirs(text: &str, key: &str, home: &Path) -> Option<PathBuf> {
    let want = format!("XDG_{key}_DIR=");
    for line in text.lines() {
        let line = line.trim();
        let Some(v) = line.strip_prefix(&want) else { continue };
        let v = v.trim_matches('"');
        let p = if let Some(rest) = v.strip_prefix("$HOME") {
            home.join(rest.trim_start_matches('/'))
        } else {
            PathBuf::from(v)
        };
        // "$HOME/" alone means "disabled" in xdg-user-dirs.
        if p == home {
            return None;
        }
        return Some(p);
    }
    None
}

fn user_name() -> String {
    std::env::var("USER").ok().filter(|u| !u.is_empty()).unwrap_or_else(|| {
        // SAFETY: getpwuid returns a pointer into static storage or NULL.
        unsafe {
            let pw = libc::getpwuid(libc::getuid());
            if pw.is_null() {
                return String::new();
            }
            std::ffi::CStr::from_ptr((*pw).pw_name).to_string_lossy().into_owned()
        }
    })
}

/// Removable media: /media/<user>/* (FP6 SD automount), /run/media/<user>/*.
pub fn media_mounts() -> Vec<PathBuf> {
    let user = user_name();
    let mut out = Vec::new();
    if user.is_empty() {
        return out;
    }
    for base in [PathBuf::from("/media").join(&user), PathBuf::from("/run/media").join(&user)] {
        let Ok(rd) = std::fs::read_dir(&base) else { continue };
        let mut v: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect();
        v.sort();
        out.extend(v);
    }
    out
}

pub fn places() -> Vec<Value> {
    let home = home();
    let mut out = vec![json!({ "id": "home", "name": "Home", "icon": "home", "path": home.to_string_lossy() })];
    for (id, key, default, name, icon) in [
        ("documents", "DOCUMENTS", "Documents", "Documents", "stock_document"),
        ("downloads", "DOWNLOAD", "Downloads", "Downloads", "save-to"),
        ("music", "MUSIC", "Music", "Music", "stock_music"),
        ("pictures", "PICTURES", "Pictures", "Pictures", "stock_image"),
        ("videos", "VIDEOS", "Videos", "Videos", "stock_video"),
    ] {
        let p = user_dir(key, default);
        if p.is_dir() {
            out.push(json!({ "id": id, "name": name, "icon": icon, "path": p.to_string_lossy() }));
        }
    }
    for m in media_mounts() {
        let label = m.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        out.push(json!({ "id": "media", "name": label, "icon": "media-eject", "path": m.to_string_lossy() }));
    }
    out
}

fn mtime(p: &Path) -> Option<(u64, u64)> {
    let md = std::fs::metadata(p).ok()?;
    if !md.is_file() {
        return None;
    }
    let t = md.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs();
    Some((t, md.len()))
}

/// Paths in GTK's recently-used.xbel (newest entries are what we want; the
/// file is small, a plain scan for href="file://..." is enough).
fn xbel_paths(text: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for chunk in text.split("<bookmark ").skip(1) {
        let Some(start) = chunk.find("href=\"") else { continue };
        let rest = &chunk[start + 6..];
        let Some(end) = rest.find('"') else { continue };
        let href = xml_unescape(&rest[..end]);
        if let Some(p) = crate::uri::file_uri_to_path(&href) {
            out.push(p);
        }
    }
    out
}

fn xml_unescape(s: &str) -> String {
    s.replace("&quot;", "\"").replace("&apos;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

fn scan(dir: &Path, depth: u32, out: &mut Vec<PathBuf>, budget: &mut u32) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.filter_map(|e| e.ok()) {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let name = e.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_file() {
            out.push(e.path());
        } else if ft.is_dir() && depth > 0 {
            scan(&e.path(), depth - 1, out, budget);
        }
    }
}

/// Recent files: GTK's recently-used list plus the newest files of the user
/// folders (two levels deep), newest first.
pub fn recent(limit: usize) -> Vec<Value> {
    let home = home();
    let mut paths: Vec<PathBuf> = Vec::new();
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"));
    if let Ok(text) = std::fs::read_to_string(data_home.join("recently-used.xbel")) {
        paths.extend(xbel_paths(&text));
    }
    let mut budget = 20_000u32;
    for (key, default) in [("DOWNLOAD", "Downloads"), ("DOCUMENTS", "Documents"), ("PICTURES", "Pictures"), ("MUSIC", "Music"), ("VIDEOS", "Videos")] {
        scan(&user_dir(key, default), 2, &mut paths, &mut budget);
    }
    paths.sort();
    paths.dedup();
    let mut items: Vec<(u64, u64, PathBuf)> = paths.into_iter().filter_map(|p| mtime(&p).map(|(t, s)| (t, s, p))).collect();
    items.sort_by(|a, b| b.0.cmp(&a.0));
    items.truncate(limit);
    items
        .into_iter()
        .map(|(t, s, p)| {
            json!({
                "path": p.to_string_lossy(),
                "name": p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                "folder": p.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default(),
                "mtime": t,
                "size": s,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_dirs_parse() {
        let home = Path::new("/home/u");
        let text = "# x\nXDG_DOWNLOAD_DIR=\"$HOME/Downloads\"\nXDG_MUSIC_DIR=\"$HOME/\"\nXDG_VIDEOS_DIR=\"/data/Videos\"\n";
        assert_eq!(parse_user_dirs(text, "DOWNLOAD", home), Some(PathBuf::from("/home/u/Downloads")));
        assert_eq!(parse_user_dirs(text, "MUSIC", home), None);
        assert_eq!(parse_user_dirs(text, "VIDEOS", home), Some(PathBuf::from("/data/Videos")));
        assert_eq!(parse_user_dirs(text, "PICTURES", home), None);
    }

    #[test]
    fn xbel() {
        let t = r#"<xbel><bookmark href="file:///home/u/a%20b.txt" added="x"/><bookmark href="https://x"/><bookmark href="file:///home/u/c&amp;d"/></xbel>"#;
        assert_eq!(xbel_paths(t), vec![PathBuf::from("/home/u/a b.txt"), PathBuf::from("/home/u/c&d")]);
    }
}
