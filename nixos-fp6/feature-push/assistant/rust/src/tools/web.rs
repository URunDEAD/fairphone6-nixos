//! Web fetch and search (permission "Internet"). Only public http(s)
//! addresses: a page (or an injected instruction) cannot make the phone
//! fetch from itself or the local network.

use super::{arg_str, schema, spec, Ctx, Output, Tool, ToolSpec};
use crate::permissions::Resource;
use crate::settings::Settings;
use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::net::IpAddr;
use std::sync::Arc;

const MAX_BYTES: usize = 2 * 1024 * 1024;

pub fn tools() -> Vec<Arc<dyn Tool>> {
    vec![Arc::new(Fetch), Arc::new(Search)]
}

pub fn public_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => !(v.is_private() || v.is_loopback() || v.is_link_local() || v.is_unspecified() || v.is_broadcast() || v.octets()[0] == 100 && (v.octets()[1] & 0xc0) == 64 || v.octets()[0] >= 224),
        IpAddr::V6(v) => {
            let s = v.segments();
            !(v.is_loopback() || v.is_unspecified() || (s[0] & 0xfe00) == 0xfc00 || (s[0] & 0xffc0) == 0xfe80 || v.is_multicast() || v.to_ipv4_mapped().map(|m| !public_ip(&IpAddr::V4(m))).unwrap_or(false))
        }
    }
}

async fn check_public(url: &reqwest::Url) -> Result<(), String> {
    if url.scheme() != "https" && url.scheme() != "http" {
        return Err("only http(s) links".into());
    }
    let host = url.host_str().ok_or("no host")?;
    let port = url.port_or_known_default().unwrap_or(443);
    let addrs: Vec<_> = tokio::net::lookup_host((host, port)).await.map_err(|e| format!("{host}: {e}"))?.collect();
    if addrs.is_empty() || addrs.iter().any(|a| !public_ip(&a.ip())) {
        return Err(format!("{host} is not a public internet address"));
    }
    Ok(())
}

/// Fetch with manual redirects (each hop checked), size cap.
pub async fn get(http: &reqwest::Client, url: &str) -> Result<(String, String), String> {
    let client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(std::time::Duration::from_secs(30)).user_agent("Mozilla/5.0 (Linux; Mobile) fp6-assistant").build().unwrap_or_else(|_| http.clone());
    let mut u = reqwest::Url::parse(url).map_err(|e| e.to_string())?;
    for _ in 0..5 {
        check_public(&u).await?;
        let resp = client.get(u.clone()).send().await.map_err(|e| e.to_string())?;
        if resp.status().is_redirection() {
            let loc = resp.headers().get("location").and_then(|v| v.to_str().ok()).ok_or("redirect without location")?;
            u = u.join(loc).map_err(|e| e.to_string())?;
            continue;
        }
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status().as_u16()));
        }
        let ctype = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
        let mut body = Vec::new();
        let mut s = resp.bytes_stream();
        while let Some(chunk) = s.next().await {
            let c = chunk.map_err(|e| e.to_string())?;
            body.extend_from_slice(&c);
            if body.len() > MAX_BYTES {
                break;
            }
        }
        return Ok((ctype, String::from_utf8_lossy(&body).into_owned()));
    }
    Err("too many redirects".into())
}

fn decode_entities(s: &str) -> String {
    let mut out = s.replace("&nbsp;", " ").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&#x27;", "'");
    // numeric entities
    while let Some(i) = out.find("&#") {
        let Some(j) = out[i..].find(';') else { break };
        let code = &out[i + 2..i + j];
        let ch = if let Some(h) = code.strip_prefix('x') { u32::from_str_radix(h, 16).ok() } else { code.parse().ok() }.and_then(char::from_u32);
        match ch {
            Some(c) => out.replace_range(i..i + j + 1, &c.to_string()),
            None => break,
        }
    }
    out.replace("&amp;", "&")
}

/// Crude HTML -> text: drop script/style/head, tags to spaces/newlines.
pub fn html_to_text(html: &str) -> String {
    let mut s = html.to_string();
    for tag in ["script", "style", "noscript", "svg", "head"] {
        loop {
            let lower = s.to_lowercase();
            let Some(a) = lower.find(&format!("<{tag}")) else { break };
            let end_tag = format!("</{tag}>");
            let b = lower[a..].find(&end_tag).map(|x| a + x + end_tag.len()).unwrap_or(s.len());
            s.replace_range(a..b, " ");
        }
    }
    let mut out = String::with_capacity(s.len() / 2);
    let mut in_tag = false;
    let mut tag = String::new();
    for c in s.chars() {
        if in_tag {
            if c == '>' {
                in_tag = false;
                let t = tag.trim_start_matches('/').split_whitespace().next().unwrap_or("").to_lowercase();
                if ["p", "br", "div", "li", "h1", "h2", "h3", "h4", "tr", "section", "article"].contains(&t.as_str()) {
                    out.push('\n');
                } else {
                    out.push(' ');
                }
                tag.clear();
            } else {
                tag.push(c);
            }
        } else if c == '<' {
            in_tag = true;
        } else {
            out.push(c);
        }
    }
    let text = decode_entities(&out);
    let lines: Vec<String> = text.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|l| !l.is_empty()).collect();
    lines.join("\n")
}

pub fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            b' ' => "+".into(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn urldecode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// DuckDuckGo HTML results: (title, url, snippet).
pub fn parse_ddg(html: &str) -> Vec<Value> {
    let mut out = Vec::new();
    for chunk in html.split("class=\"result__a\"").skip(1).take(10) {
        let href = chunk.split("href=\"").nth(1).and_then(|h| h.split('"').next()).unwrap_or("");
        let url = match href.split("uddg=").nth(1) {
            Some(enc) => urldecode(enc.split('&').next().unwrap_or("")),
            None => href.to_string(),
        };
        let title = chunk.split_once('>').and_then(|(_, t)| t.split("</a>").next()).map(html_to_text).unwrap_or_default();
        let snippet = chunk.split("result__snippet").nth(1).and_then(|s| s.split_once('>')).and_then(|(_, r)| r.split("</a").next()).map(html_to_text).unwrap_or_default();
        if url.starts_with("http") {
            out.push(json!({"title": title, "url": url, "snippet": snippet}));
        }
    }
    out
}

struct Fetch;
#[async_trait]
impl Tool for Fetch {
    fn spec(&self) -> ToolSpec {
        spec("web_fetch", "Internet", "Fetch a web page", "Download a public web page and return its text.", schema(&[("url", "string", "https:// address", true)]), false, true)
    }
    fn resources(&self, a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        let u = arg_str(a, "url")?;
        if !(u.starts_with("https://") || u.starts_with("http://")) {
            return Err("only http(s) addresses".into());
        }
        Ok(vec![Resource::new("web")])
    }
    async fn run(&self, a: Value, ctx: &Ctx) -> Result<Output, String> {
        let url = arg_str(&a, "url")?;
        let (ctype, body) = get(&ctx.http, url).await?;
        let text = if ctype.contains("html") || body.trim_start().starts_with('<') { html_to_text(&body) } else { body };
        Ok(Output::text(crate::llm::truncate(&text, 12_000)))
    }
}

struct Search;
#[async_trait]
impl Tool for Search {
    fn spec(&self) -> ToolSpec {
        spec("web_search", "Internet", "Search the web", "Search the web; returns titles, links and snippets.", schema(&[("query", "string", "Search words", true)]), false, true)
    }
    fn resources(&self, _a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        Ok(vec![Resource::new("web")])
    }
    async fn run(&self, a: Value, ctx: &Ctx) -> Result<Output, String> {
        let q = arg_str(&a, "query")?;
        let url = ctx.settings.web_search_url.replace("{q}", &urlencode(q));
        let (_, body) = get(&ctx.http, &url).await?;
        let results = parse_ddg(&body);
        if results.is_empty() {
            return Ok(Output::text(crate::llm::truncate(&html_to_text(&body), 6000)));
        }
        Ok(Output::json(&json!(results)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_addresses_only() {
        for ip in ["127.0.0.1", "10.0.0.1", "192.168.1.5", "172.16.42.1", "100.64.0.1", "169.254.1.1", "::1", "fe80::1", "fd00::1", "::ffff:192.168.1.1"] {
            assert!(!public_ip(&ip.parse().unwrap()), "{ip}");
        }
        for ip in ["93.184.216.34", "2606:2800:220:1::1"] {
            assert!(public_ip(&ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn html_text() {
        let t = html_to_text("<html><head><title>x</title><style>a{}</style></head><body><h1>Hello&nbsp;World</h1><script>evil()</script><p>a &amp; b &#x41;</p></body></html>");
        assert_eq!(t, "Hello World\na & b A");
    }

    #[test]
    fn ddg_parse() {
        let html = r#"<a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fpage&amp;rut=x">Example <b>Page</b></a><a class="result__snippet" href="x">Snippet text</a>"#;
        let r = parse_ddg(html);
        assert_eq!(r[0]["url"], "https://example.com/page");
        assert_eq!(r[0]["title"], "Example Page");
        assert_eq!(r[0]["snippet"], "Snippet text");
        assert_eq!(urlencode("a b&c"), "a+b%26c");
    }
}
