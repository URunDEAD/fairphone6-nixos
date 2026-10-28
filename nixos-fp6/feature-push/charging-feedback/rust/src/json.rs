//! Flat JSON lookups (fp6-batteryd and fp6-usbmoded status files have
//! unique key names) and a small object writer for the published state.

use std::fmt::Write;

fn raw<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let pat = format!("\"{key}\":");
    let i = text.find(&pat)? + pat.len();
    Some(text[i..].trim_start())
}

pub fn jnum(text: &str, key: &str) -> Option<i64> {
    let r = raw(text, key)?;
    let end = r.find(|c: char| !(c.is_ascii_digit() || c == '-')).unwrap_or(r.len());
    r[..end].parse().ok()
}

pub fn jbool(text: &str, key: &str) -> Option<bool> {
    let r = raw(text, key)?;
    if r.starts_with("true") {
        Some(true)
    } else if r.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

pub fn jstr(text: &str, key: &str) -> Option<String> {
    let r = raw(text, key)?.strip_prefix('"')?;
    let mut out = String::new();
    let mut esc = false;
    for c in r.chars() {
        match (esc, c) {
            (true, 'n') => {
                out.push('\n');
                esc = false;
            }
            (true, c) => {
                out.push(c);
                esc = false;
            }
            (false, '\\') => esc = true,
            (false, '"') => return Some(out),
            (false, c) => out.push(c),
        }
    }
    None
}

pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            c if (c as u32) < 0x20 => {
                let _ = write!(o, "\\u{:04x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// `Obj::new().i("a", 1).s("b", "x").finish()`
pub struct Obj(String);

impl Obj {
    pub fn new() -> Self {
        Obj(String::from("{"))
    }
    fn key(&mut self, k: &str) {
        if self.0.len() > 1 {
            self.0.push(',');
        }
        self.0.push_str(&esc(k));
        self.0.push(':');
    }
    pub fn i(mut self, k: &str, v: i64) -> Self {
        self.key(k);
        let _ = write!(self.0, "{v}");
        self
    }
    pub fn b(mut self, k: &str, v: bool) -> Self {
        self.key(k);
        self.0.push_str(if v { "true" } else { "false" });
        self
    }
    pub fn s(mut self, k: &str, v: &str) -> Self {
        self.key(k);
        self.0.push_str(&esc(v));
        self
    }
    pub fn finish(mut self) -> String {
        self.0.push('}');
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookups() {
        let s = r#"{"time":5,"level":77,"charging":{"plugged":true,"charger":"USB \"PD\"","timeToFull": -1},"limit":{"mode":"hold"}}"#;
        assert_eq!(jnum(s, "level"), Some(77));
        assert_eq!(jnum(s, "timeToFull"), Some(-1));
        assert_eq!(jbool(s, "plugged"), Some(true));
        assert_eq!(jstr(s, "charger").as_deref(), Some("USB \"PD\""));
        assert_eq!(jstr(s, "mode").as_deref(), Some("hold"));
        assert_eq!(jbool(s, "missing"), None);
    }

    #[test]
    fn writer() {
        let o = Obj::new().i("a", -3).b("b", true).s("c", "x\"y").finish();
        assert_eq!(o, r#"{"a":-3,"b":true,"c":"x\"y"}"#);
    }
}
