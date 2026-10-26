//! Minimal JSON writer for the status/history files the Settings page reads.

use std::fmt::Write;

pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(o, "\\u{:04x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Builds one JSON object: `Obj::new().i("a", 1).s("b", "x").finish()`.
#[derive(Default)]
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
    pub fn f(mut self, k: &str, v: f64) -> Self {
        self.key(k);
        if v.is_finite() {
            let _ = write!(self.0, "{v:.3}");
        } else {
            self.0.push_str("null");
        }
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
    /// Raw JSON value (object/array built elsewhere).
    pub fn raw(mut self, k: &str, v: &str) -> Self {
        self.key(k);
        self.0.push_str(v);
        self
    }
    pub fn opt_i(self, k: &str, v: Option<i64>) -> Self {
        match v {
            Some(v) => self.i(k, v),
            None => self.raw(k, "null"),
        }
    }
    pub fn finish(mut self) -> String {
        self.0.push('}');
        self.0
    }
}

pub fn array<I: IntoIterator<Item = String>>(items: I) -> String {
    let mut o = String::from("[");
    for (n, it) in items.into_iter().enumerate() {
        if n > 0 {
            o.push(',');
        }
        o.push_str(&it);
    }
    o.push(']');
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn writes() {
        let o = Obj::new().i("a", -1).s("b", "x\"y\n").b("c", true).raw("d", &array(vec!["1".into(), "2".into()])).opt_i("e", None).finish();
        assert_eq!(o, r#"{"a":-1,"b":"x\"y\n","c":true,"d":[1,2],"e":null}"#);
    }
}
