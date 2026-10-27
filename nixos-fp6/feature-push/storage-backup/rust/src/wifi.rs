//! NetworkManager Wi-Fi keyfiles: which files are exported and what the
//! root helper accepts back on restore.

use crate::util::safe_name;
use std::collections::HashMap;

pub const MAX_KEYFILE: usize = 64 * 1024;

/// Minimal keyfile reader: section -> key -> value.
pub fn parse(text: &str) -> HashMap<String, HashMap<String, String>> {
    let mut m: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut sec = String::new();
    for l in text.lines() {
        let l = l.trim();
        if l.starts_with('#') || l.is_empty() {
            continue;
        }
        if l.starts_with('[') && l.ends_with(']') {
            sec = l[1..l.len() - 1].to_string();
            m.entry(sec.clone()).or_default();
        } else if let Some((k, v)) = l.split_once('=') {
            m.entry(sec.clone()).or_default().insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    m
}

/// A Wi-Fi connection keyfile: [connection] type=wifi (or 802-11-wireless),
/// a uuid and an id, plain text, bounded size.
pub fn is_wifi(text: &str) -> bool {
    if text.len() > MAX_KEYFILE || text.contains('\0') {
        return false;
    }
    let m = parse(text);
    let Some(c) = m.get("connection") else { return false };
    matches!(c.get("type").map(String::as_str), Some("wifi") | Some("802-11-wireless"))
        && c.get("uuid").is_some_and(|u| !u.is_empty())
        && c.get("id").is_some_and(|u| !u.is_empty())
}

pub fn uuid(text: &str) -> Option<String> {
    parse(text).get("connection")?.get("uuid").cloned()
}

/// File name for a restored keyfile: "<name>.nmconnection" with only
/// [A-Za-z0-9._ -] kept.
pub fn file_name(name: &str) -> Option<String> {
    let base = name.strip_suffix(".nmconnection").unwrap_or(name);
    let clean: String = base.chars().filter(|c| c.is_ascii_alphanumeric() || " ._-".contains(*c)).collect();
    let clean = clean.trim_matches(|c| c == '.' || c == ' ').to_string();
    if clean.is_empty() || !safe_name(&clean) || clean.len() > 200 {
        return None;
    }
    Some(format!("{clean}.nmconnection"))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    pub const SAMPLE: &str = "[connection]\nid=Test Net\nuuid=0b5b6c1e-0000-4000-8000-000000000001\ntype=wifi\n\n[wifi]\nssid=TestNet\n\n[wifi-security]\nkey-mgmt=wpa-psk\npsk=not-a-real-password\n";
    #[test]
    fn keyfiles() {
        assert!(is_wifi(SAMPLE));
        assert!(!is_wifi("[connection]\nid=usb0\nuuid=1\ntype=ethernet\n"));
        assert!(!is_wifi("[connection]\ntype=wifi\n"));
        assert_eq!(uuid(SAMPLE).as_deref(), Some("0b5b6c1e-0000-4000-8000-000000000001"));
        assert_eq!(file_name("Test Net.nmconnection").as_deref(), Some("Test Net.nmconnection"));
        assert_eq!(file_name("../../etc/x").as_deref(), Some("etcx.nmconnection"));
        assert_eq!(file_name("..").as_deref(), None);
        assert_eq!(file_name("a/b").as_deref(), Some("ab.nmconnection"));
    }
}
