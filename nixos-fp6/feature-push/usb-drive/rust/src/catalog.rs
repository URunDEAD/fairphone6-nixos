//! Curated download list (downloads.json). Every entry points at the
//! project's own HTTPS host; the SHA-256 comes from the project's sums file
//! on its own host, and the image name is picked from that sums file with a
//! pattern, so point releases do not need a catalog update.
//!
//! Entry fields:
//!   id, name, description, homepage, size (text), arch
//!   base      directory URL (https), ends with "/"
//!   sums      sums file name under `base`, or
//!   sumsIndex glob of the sums file name, found in `base`'s directory index
//!   sumsBase  optional other https directory for the sums file (e.g. the
//!             project's master server while `base` is its redirector)
//!   pattern   glob for the image name inside the sums file; `*` any text,
//!             `{v}` a version (digits, '.', '-', starting with a digit)
//!   url       optional fixed download URL (e.g. NixOS "latest-..." link);
//!             the saved file is still named after the sums entry.

use serde_json::Value;
use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub homepage: String,
    pub size: String,
    pub base: String,
    pub sums: String,
    pub sums_index: String,
    pub sums_base: String,
    pub pattern: String,
    pub url: String,
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
}

pub fn is_https(u: &str) -> bool {
    u.starts_with("https://") && u.len() > 8 && !u.contains(char::is_whitespace)
}

pub fn parse(text: &str) -> Result<Vec<Entry>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("downloads.json: {e}"))?;
    let arr = v.get("entries").and_then(|x| x.as_array()).ok_or("downloads.json: no entries")?;
    let mut out = Vec::new();
    for e in arr {
        let en = Entry {
            id: s(e, "id"), name: s(e, "name"), description: s(e, "description"), homepage: s(e, "homepage"),
            size: s(e, "size"), base: s(e, "base"), sums: s(e, "sums"), sums_index: s(e, "sumsIndex"),
            sums_base: s(e, "sumsBase"), pattern: s(e, "pattern"), url: s(e, "url"),
        };
        if en.id.is_empty() || en.name.is_empty() || !is_https(&en.base) || !en.base.ends_with('/') || en.pattern.is_empty()
            || (en.sums.is_empty() == en.sums_index.is_empty())
            || (!en.url.is_empty() && !is_https(&en.url))
            || (!en.sums_base.is_empty() && (!is_https(&en.sums_base) || !en.sums_base.ends_with('/')))
            || en.sums.contains('/') || en.sums_index.contains('/')
        {
            return Err(format!("downloads.json: invalid entry {:?}", en.id));
        }
        out.push(en);
    }
    Ok(out)
}

impl Entry {
    pub fn sums_dir(&self) -> &str {
        if self.sums_base.is_empty() { &self.base } else { &self.sums_base }
    }
    pub fn to_json(&self) -> Value {
        serde_json::json!({"id": self.id, "name": self.name, "description": self.description,
                           "homepage": self.homepage, "size": self.size,
                           "host": host_of(&self.base)})
    }
}

pub fn host_of(u: &str) -> String {
    u.trim_start_matches("https://").split('/').next().unwrap_or("").to_string()
}

/// Glob with `*`, `?` and `{v}`.
pub fn glob(pat: &str, name: &str) -> bool {
    fn m(p: &[u8], n: &[u8]) -> bool {
        if p.is_empty() {
            return n.is_empty();
        }
        if p.starts_with(b"{v}") {
            let rest = &p[3..];
            if n.is_empty() || !n[0].is_ascii_digit() {
                return false;
            }
            let mut i = 1;
            loop {
                if m(rest, &n[i..]) {
                    return true;
                }
                if i < n.len() && (n[i].is_ascii_digit() || n[i] == b'.' || n[i] == b'-') {
                    i += 1;
                } else {
                    return false;
                }
            }
        }
        match p[0] {
            b'*' => (0..=n.len()).any(|i| m(&p[1..], &n[i..])),
            b'?' => !n.is_empty() && m(&p[1..], &n[1..]),
            c => !n.is_empty() && n[0] == c && m(&p[1..], &n[1..]),
        }
    }
    !name.contains('/') && m(pat.as_bytes(), name.as_bytes())
}

/// Natural order: digit runs compare as numbers.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let si = i;
            while i < a.len() && a[i].is_ascii_digit() {
                i += 1;
            }
            let sj = j;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            let na = std::str::from_utf8(&a[si..i]).unwrap().trim_start_matches('0');
            let nb = std::str::from_utf8(&b[sj..j]).unwrap().trim_start_matches('0');
            let o = na.len().cmp(&nb.len()).then(na.cmp(nb));
            if o != Ordering::Equal {
                return o;
            }
        } else {
            // A version separator sorts after the end of a component:
            // "26.04.1-" > "26.04-".
            let rank = |c: u8| if c == b'.' { 1u16 << 9 } else { c as u16 };
            let o = rank(a[i]).cmp(&rank(b[j]));
            if o != Ordering::Equal {
                return o;
            }
            i += 1;
            j += 1;
        }
    }
    (a.len() - i).cmp(&(b.len() - j))
}

/// Picks the newest image matching `pattern` from a sums file:
/// (file name, sha256).
pub fn pick(sums_text: &str, pattern: &str) -> Option<(String, String)> {
    let mut v: Vec<(String, String)> = sums_text.lines().filter_map(crate::checksum::parse_line)
        .map(|(h, n)| (n.rsplit('/').next().unwrap_or("").to_string(), h))
        .filter(|(n, _)| glob(pattern, n))
        .collect();
    v.sort_by(|a, b| natural_cmp(&a.0, &b.0));
    v.pop()
}

/// Names linked from an HTML directory index (href="name").
pub fn index_names(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find("href=\"") {
        rest = &rest[i + 6..];
        let Some(j) = rest.find('"') else { break };
        let n = &rest[..j];
        if !n.is_empty() && !n.contains('/') && !n.contains('?') && !n.contains(':') {
            out.push(n.to_string());
        }
        rest = &rest[j..];
    }
    out
}

pub fn pick_index(html: &str, pattern: &str) -> Option<String> {
    let mut v: Vec<String> = index_names(html).into_iter().filter(|n| glob(pattern, n)).collect();
    v.sort_by(|a, b| natural_cmp(a, b));
    v.pop()
}

/// A file name that is safe to create in the library folder.
pub fn safe_file_name(n: &str) -> bool {
    !n.is_empty() && n.len() < 200 && !n.starts_with('.') && !n.contains('/') && !n.contains('\0')
        && n.chars().all(|c| c.is_ascii_alphanumeric() || "._-+~".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globbing() {
        assert!(glob("ubuntu-{v}-desktop-amd64.iso", "ubuntu-26.04.1-desktop-amd64.iso"));
        assert!(glob("ubuntu-{v}-desktop-amd64.iso", "ubuntu-26.04-desktop-amd64.iso"));
        assert!(!glob("ubuntu-{v}-desktop-amd64.iso", "ubuntu-26.04-live-server-amd64.iso"));
        assert!(glob("debian-{v}-amd64-netinst.iso", "debian-13.7.0-amd64-netinst.iso"));
        assert!(!glob("debian-{v}-amd64-netinst.iso", "debian-edu-13.7.0-amd64-netinst.iso"));
        assert!(glob("Fedora-Workstation-Live-{v}.x86_64.iso", "Fedora-Workstation-Live-44-1.7.x86_64.iso"));
        assert!(glob("Fedora-Workstation-{v}-x86_64-CHECKSUM", "Fedora-Workstation-44-1.7-x86_64-CHECKSUM"));
        assert!(glob("nixos-minimal-*-x86_64-linux.iso", "nixos-minimal-26.05.12658.1f607aa79b63-x86_64-linux.iso"));
        assert!(!glob("*.iso", "a/b.iso"));
    }

    #[test]
    fn natural() {
        assert_eq!(natural_cmp("ubuntu-24.04.10-d", "ubuntu-24.04.9-d"), Ordering::Greater);
        assert_eq!(natural_cmp("ubuntu-26.04.1-desktop", "ubuntu-26.04-desktop"), Ordering::Greater);
        assert_eq!(natural_cmp("a1", "a1"), Ordering::Equal);
    }

    #[test]
    fn picking() {
        let sums = "faab *ubuntu-24.04.3-desktop-amd64.iso\n".replace("faab", &"a".repeat(64))
            + &format!("{} *ubuntu-24.04.4-desktop-amd64.iso\n", "b".repeat(64))
            + &format!("{} *ubuntu-24.04.4-live-server-amd64.iso\n", "c".repeat(64));
        assert_eq!(pick(&sums, "ubuntu-{v}-desktop-amd64.iso"),
                   Some(("ubuntu-24.04.4-desktop-amd64.iso".into(), "b".repeat(64))));
        let html = r#"<a href="?C=N;O=D">x</a><a href="/pub/">up</a><a href="Fedora-Workstation-44-1.7-x86_64-CHECKSUM">c</a>"#;
        assert_eq!(pick_index(html, "Fedora-Workstation-{v}-x86_64-CHECKSUM").as_deref(),
                   Some("Fedora-Workstation-44-1.7-x86_64-CHECKSUM"));
    }

    #[test]
    fn shipped_catalog_is_valid() {
        let text = include_str!("../../downloads.json");
        let e = parse(text).unwrap();
        assert!(e.len() >= 4);
        for x in &e {
            assert!(is_https(&x.base));
        }
        assert!(parse(r#"{"entries":[{"id":"x","name":"x","base":"http://a/","sums":"S","pattern":"*"}]}"#).is_err());
        assert!(parse(r#"{"entries":[{"id":"x","name":"x","base":"https://a/","sums":"../S","pattern":"*"}]}"#).is_err());
    }

    #[test]
    fn names() {
        assert!(safe_file_name("nixos-minimal-26.05.1-x86_64-linux.iso"));
        assert!(!safe_file_name("../x.iso"));
        assert!(!safe_file_name(".x"));
        assert!(!safe_file_name("a b.iso"));
    }
}
