//! Springboard: type first, act at once (after Volla's Springboard). For
//! the text typed so far, instant local suggestions with no model call:
//! matching apps, contacts (call / message: opens the dialer or messaging
//! app with the number filled in, nothing is sent), a calculation, a link,
//! a web search, and always "Ask the assistant". Choosing "Ask" (or
//! pressing Enter) hands the text to the agent, which turns it into actions
//! through the permission-gated tools.

use crate::tools::apps::{find_apps, App};
use crate::tools::web::urlencode;
use serde_json::{json, Value};

/// A tiny calculator: + - * / % ^ and parentheses, decimals.
pub fn calc(expr: &str) -> Option<f64> {
    let e: String = expr.chars().filter(|c| !c.is_whitespace()).collect::<String>().replace('×', "*").replace('÷', "/").replace(',', ".");
    if e.is_empty() || !e.chars().all(|c| c.is_ascii_digit() || "+-*/%^().".contains(c)) || !e.chars().any(|c| "+-*/%^".contains(c)) {
        return None;
    }
    let toks: Vec<char> = e.chars().collect();
    let mut pos = 0;
    let v = expr_sum(&toks, &mut pos)?;
    (pos == toks.len() && v.is_finite()).then_some(v)
}

fn expr_sum(t: &[char], p: &mut usize) -> Option<f64> {
    let mut v = expr_prod(t, p)?;
    while *p < t.len() && (t[*p] == '+' || t[*p] == '-') {
        let op = t[*p];
        *p += 1;
        let r = expr_prod(t, p)?;
        v = if op == '+' { v + r } else { v - r };
    }
    Some(v)
}

fn expr_prod(t: &[char], p: &mut usize) -> Option<f64> {
    let mut v = expr_pow(t, p)?;
    while *p < t.len() && "*/%".contains(t[*p]) {
        let op = t[*p];
        *p += 1;
        let r = expr_pow(t, p)?;
        v = match op {
            '*' => v * r,
            '/' => v / r,
            _ => v * r / 100.0, // "15%200" reads as 15 % of 200
        };
    }
    Some(v)
}

fn expr_pow(t: &[char], p: &mut usize) -> Option<f64> {
    let b = expr_atom(t, p)?;
    if *p < t.len() && t[*p] == '^' {
        *p += 1;
        let e = expr_pow(t, p)?;
        return Some(b.powf(e));
    }
    Some(b)
}

fn expr_atom(t: &[char], p: &mut usize) -> Option<f64> {
    if *p < t.len() && t[*p] == '-' {
        *p += 1;
        return expr_atom(t, p).map(|v| -v);
    }
    if *p < t.len() && t[*p] == '(' {
        *p += 1;
        let v = expr_sum(t, p)?;
        if *p < t.len() && t[*p] == ')' {
            *p += 1;
            return Some(v);
        }
        return None;
    }
    let start = *p;
    while *p < t.len() && (t[*p].is_ascii_digit() || t[*p] == '.') {
        *p += 1;
    }
    t[start..*p].iter().collect::<String>().parse().ok()
}

fn fmt_num(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 && v.abs() < 1e15 {
        format!("{}", v.round() as i64)
    } else {
        let s = format!("{v:.6}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn looks_like_url(s: &str) -> Option<String> {
    let t = s.trim();
    if t.contains(' ') || t.len() < 4 {
        return None;
    }
    if t.starts_with("https://") || t.starts_with("http://") {
        return Some(t.into());
    }
    let host = t.split('/').next()?;
    let tld = host.rsplit('.').next()?;
    (host.contains('.') && tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic())).then(|| format!("https://{t}"))
}

/// Contacts are looked up by the caller (they need the contacts DB and
/// the user's own Springboard use; no model is involved).
pub fn suggest(text: &str, apps: &[App], contacts: &[Value], web_search_url: &str) -> Vec<Value> {
    let t = text.trim();
    let mut out = Vec::new();
    if t.is_empty() {
        return out;
    }
    if let Some(v) = calc(t) {
        out.push(json!({"kind": "calc", "title": format!("= {}", fmt_num(v)), "value": fmt_num(v)}));
    }
    let lower = t.to_lowercase();
    // "call Test" / "text Test" / "message Test"
    let (verb, rest) = match lower.split_once(' ') {
        Some((v, r)) if ["call", "ring", "phone"].contains(&v) => ("call", r.to_string()),
        Some((v, r)) if ["text", "sms", "message", "msg"].contains(&v) => ("message", r.to_string()),
        _ => ("", lower.clone()),
    };
    for c in contacts.iter().take(3) {
        let name = c["name"].as_str().unwrap_or("");
        let Some(number) = c["phones"].get(0).and_then(Value::as_str) else { continue };
        let clean: String = number.chars().filter(|ch| ch.is_ascii_digit() || *ch == '+').collect();
        if verb != "message" {
            out.push(json!({"kind": "call", "title": format!("Call {name}"), "subtitle": number, "url": format!("tel:///{clean}")}));
        }
        if verb != "call" {
            out.push(json!({"kind": "message", "title": format!("Message {name}"), "subtitle": number, "url": format!("message:///{clean}")}));
        }
    }
    let _ = rest;
    for a in find_apps(t, apps).into_iter().take(4) {
        out.push(json!({"kind": "app", "title": a.name, "subtitle": "Open app", "app": a.id, "icon": a.icon}));
    }
    if let Some(u) = looks_like_url(t) {
        out.push(json!({"kind": "url", "title": format!("Open {u}"), "url": u}));
    }
    // The assistant first (the point of the Springboard), then search.
    out.insert(0, json!({"kind": "ask", "title": "Ask the assistant", "subtitle": t}));
    out.push(json!({"kind": "search", "title": format!("Search the web for \u{201c}{t}\u{201d}"), "url": web_search_url.replace("html.duckduckgo.com/html/", "duckduckgo.com/").replace("{q}", &urlencode(t))}));
    out
}

/// The contact name to look up for the typed text, if any.
pub fn contact_query(text: &str) -> Option<String> {
    let lower = text.trim().to_lowercase();
    let rest = match lower.split_once(' ') {
        Some((v, r)) if ["call", "ring", "phone", "text", "sms", "message", "msg"].contains(&v) => r.to_string(),
        _ => lower,
    };
    (rest.chars().count() >= 2 && !rest.chars().all(|c| c.is_ascii_digit() || "+-*/%^(). ".contains(c))).then_some(rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculator() {
        assert_eq!(calc("2+3*4"), Some(14.0));
        assert_eq!(calc("(2+3)*4"), Some(20.0));
        assert_eq!(calc("2^10"), Some(1024.0));
        assert_eq!(calc("15%200"), Some(30.0));
        assert_eq!(calc("-3+5"), Some(2.0));
        assert_eq!(calc("42"), None);
        assert_eq!(calc("2+"), None);
        assert_eq!(calc("rm -rf"), None);
        assert_eq!(fmt_num(0.1 + 0.2), "0.3");
    }

    #[test]
    fn suggestions() {
        let apps = vec![App { id: "lomiri-camera-app".into(), name: "Camera".into(), icon: String::new(), keywords: vec![] }];
        let contacts = vec![json!({"name": "Test Contact", "phones": ["+999 555 0100"]})];
        let s = suggest("call test", &apps, &contacts, "https://html.duckduckgo.com/html/?q={q}");
        assert_eq!(s[0]["kind"], "ask");
        assert!(s.iter().any(|x| x["kind"] == "call" && x["url"] == "tel:///+9995550100"));
        assert!(!s.iter().any(|x| x["kind"] == "message"));
        let s = suggest("cam", &apps, &[], "https://html.duckduckgo.com/html/?q={q}");
        assert!(s.iter().any(|x| x["kind"] == "app"));
        assert_eq!(s.last().unwrap()["url"], "https://duckduckgo.com/?q=cam");
        let s = suggest("example.com", &[], &[], "https://html.duckduckgo.com/html/?q={q}");
        assert!(s.iter().any(|x| x["url"] == "https://example.com"));
        assert!(suggest("  ", &[], &[], "").is_empty());
        assert_eq!(contact_query("text Test").as_deref(), Some("test"));
        assert_eq!(contact_query("2+2"), None);
    }
}
