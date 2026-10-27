//! MIME type -> glob patterns from the shared-mime-info database
//! (`mime/globs2`, `mime/aliases`, `mime/subclasses` in the XDG data dirs),
//! and glob -> regular expression for the picker's Recent list.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

#[derive(Default)]
pub struct MimeDb {
    /// mime type -> globs
    globs: HashMap<String, Vec<String>>,
    /// alias -> canonical type
    aliases: HashMap<String, String>,
    /// parent -> direct children (subclasses)
    children: HashMap<String, Vec<String>>,
}

pub fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    match std::env::var_os("XDG_DATA_HOME") {
        Some(d) if !d.is_empty() => dirs.push(PathBuf::from(d)),
        _ => {
            if let Some(h) = std::env::var_os("HOME") {
                dirs.push(PathBuf::from(h).join(".local/share"));
            }
        }
    }
    let sys = std::env::var("XDG_DATA_DIRS").unwrap_or_default();
    let sys = if sys.is_empty() { "/usr/local/share:/usr/share".to_string() } else { sys };
    for d in sys.split(':').filter(|d| !d.is_empty()) {
        dirs.push(PathBuf::from(d));
    }
    // The user manager's XDG_DATA_DIRS may be the bare default on NixOS.
    for d in ["/run/current-system/sw/share", "/etc/profiles/per-user"] {
        let p = PathBuf::from(d);
        if !dirs.contains(&p) {
            dirs.push(p);
        }
    }
    dirs
}

impl MimeDb {
    pub fn load() -> MimeDb {
        let mut db = MimeDb::default();
        for dir in data_dirs() {
            let mime = dir.join("mime");
            if let Ok(text) = std::fs::read_to_string(mime.join("globs2")) {
                db.add_globs2(&text);
            }
            if let Ok(text) = std::fs::read_to_string(mime.join("aliases")) {
                for line in text.lines() {
                    if let Some((a, c)) = line.split_once(' ') {
                        db.aliases.entry(a.to_string()).or_insert_with(|| c.to_string());
                    }
                }
            }
            if let Ok(text) = std::fs::read_to_string(mime.join("subclasses")) {
                for line in text.lines() {
                    if let Some((child, parent)) = line.split_once(' ') {
                        let v = db.children.entry(parent.to_string()).or_default();
                        if !v.iter().any(|c| c == child) {
                            v.push(child.to_string());
                        }
                    }
                }
            }
        }
        db
    }

    pub fn add_globs2(&mut self, text: &str) {
        for line in text.lines() {
            if line.starts_with('#') {
                continue;
            }
            // weight:mimetype:glob[:flags]
            let mut parts = line.splitn(4, ':');
            let (_w, Some(mime), Some(glob)) = (parts.next(), parts.next(), parts.next()) else {
                continue;
            };
            let v = self.globs.entry(mime.to_string()).or_default();
            if !v.iter().any(|g| g == glob) {
                v.push(glob.to_string());
            }
        }
    }

    fn canonical<'a>(&'a self, mime: &'a str) -> &'a str {
        self.aliases.get(mime).map(String::as_str).unwrap_or(mime)
    }

    /// Glob patterns for a MIME type, including its subclasses and, for
    /// "major/*", every type of that major type. "*" for "everything".
    pub fn globs_for(&self, mime: &str) -> Vec<String> {
        let mime = mime.trim().to_ascii_lowercase();
        if mime == "*" || mime == "*/*" || mime == "application/octet-stream" {
            return vec!["*".to_string()];
        }
        let mut out: Vec<String> = Vec::new();
        let push = |g: &str, out: &mut Vec<String>| {
            if !out.iter().any(|x| x == g) {
                out.push(g.to_string());
            }
        };
        if let Some(major) = mime.strip_suffix("/*") {
            let prefix = format!("{major}/");
            let mut keys: Vec<&String> = self.globs.keys().filter(|k| k.starts_with(&prefix)).collect();
            keys.sort();
            for k in keys {
                for g in &self.globs[k] {
                    push(g, &mut out);
                }
            }
            return out;
        }
        let mut seen = HashSet::new();
        let mut stack = vec![self.canonical(&mime).to_string()];
        while let Some(m) = stack.pop() {
            if !seen.insert(m.clone()) || seen.len() > 512 {
                continue;
            }
            if let Some(gs) = self.globs.get(&m) {
                for g in gs {
                    push(g, &mut out);
                }
            }
            if let Some(cs) = self.children.get(&m) {
                stack.extend(cs.iter().cloned());
            }
        }
        out
    }
}

/// Anchored, case-insensitive-ready regular expression (JavaScript syntax)
/// for a shell glob as used in file filters.
pub fn glob_to_regex(glob: &str) -> String {
    let mut re = String::from("^");
    let mut chars = glob.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' => re.push_str(".*"),
            '?' => re.push('.'),
            '[' => {
                // Copy a bracket expression if it is closed, else a literal '['.
                let rest: String = chars.clone().collect();
                if let Some(end) = rest.find(']').filter(|&e| e > 0 || rest.len() > 1) {
                    let body: String = rest[..end].to_string();
                    let body = body.strip_prefix('!').map(|b| format!("^{b}")).unwrap_or(body);
                    re.push('[');
                    re.push_str(&body.replace('\\', "\\\\"));
                    re.push(']');
                    for _ in 0..=end {
                        chars.next();
                    }
                } else {
                    re.push_str("\\[");
                }
            }
            c if "\\^$.|+(){}]/".contains(c) => {
                re.push('\\');
                re.push(c);
            }
            c => re.push(c),
        }
    }
    re.push('$');
    re
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_and_subclasses() {
        let mut db = MimeDb::default();
        db.add_globs2("# comment\n50:application/x-bittorrent:*.torrent\n50:text/plain:*.txt\n50:text/x-csrc:*.c\n50:image/png:*.png\n50:image/jpeg:*.jpg\n50:image/jpeg:*.jpeg\n");
        db.children.insert("text/plain".into(), vec!["text/x-csrc".into()]);
        db.aliases.insert("application/x-torrent".into(), "application/x-bittorrent".into());
        assert_eq!(db.globs_for("application/x-bittorrent"), vec!["*.torrent"]);
        assert_eq!(db.globs_for("application/x-torrent"), vec!["*.torrent"]);
        let t = db.globs_for("text/plain");
        assert!(t.contains(&"*.txt".to_string()) && t.contains(&"*.c".to_string()));
        assert_eq!(db.globs_for("image/*").len(), 3);
        assert_eq!(db.globs_for("application/octet-stream"), vec!["*"]);
        assert!(db.globs_for("application/x-unknown").is_empty());
    }

    #[test]
    fn regex() {
        assert_eq!(glob_to_regex("*.torrent"), "^.*\\.torrent$");
        assert_eq!(glob_to_regex("README?"), "^README.$");
        assert_eq!(glob_to_regex("*.[ch]"), "^.*\\.[ch]$");
        assert_eq!(glob_to_regex("*.[!o]"), "^.*\\.[^o]$");
        assert_eq!(glob_to_regex("a+b(1)"), "^a\\+b\\(1\\)$");
    }
}
