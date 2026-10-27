//! Update server URLs: parsing, normalisation and the transport rule.
//!
//! * `https://` to any host.
//! * `http://` only to an IP literal in a private range: 10/8, 172.16/12,
//!   192.168/16, NetBird/CGNAT 100.64/10, loopback 127/8, link-local
//!   169.254/16; IPv6 ::1, fc00::/7, fe80::/10; or `localhost`. Host names
//!   over http are refused (DNS could point them anywhere; use https).
//! * `file://` only on the laptop dry run's fake phone.
//! Integrity never depends on this: manifests and store paths are signed
//! with keys fixed in the system configuration. The rule only keeps update
//! traffic off the open internet in clear text.

use std::net::{Ipv4Addr, Ipv6Addr};

#[derive(Debug, PartialEq, Clone)]
pub struct Url {
    pub scheme: String,
    pub host: String,
    pub port: Option<u16>,
    pub path: String,
}

pub fn parse(s: &str) -> Result<Url, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty address".into());
    }
    if s.len() > 512 || s.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("address too long or contains spaces".into());
    }
    let (scheme, rest) = s.split_once("://").ok_or("the address must start with https:// or http://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme == "file" {
        return Ok(Url { scheme, host: String::new(), port: None, path: rest.to_string() });
    }
    if scheme != "http" && scheme != "https" {
        return Err(format!("unsupported scheme {scheme}://"));
    }
    if rest.contains('?') || rest.contains('#') {
        return Err("no query or fragment allowed".into());
    }
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    if authority.contains('@') {
        return Err("no user name or password in the address".into());
    }
    let (host, port) = if let Some(r) = authority.strip_prefix('[') {
        let (h, after) = r.split_once(']').ok_or("unterminated IPv6 address")?;
        let port = match after.strip_prefix(':') {
            Some(p) => Some(p),
            None if after.is_empty() => None,
            None => return Err("bad address after the IPv6 host".into()),
        };
        (format!("[{h}]"), port)
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), Some(p)),
            None => (authority.to_string(), None),
        }
    };
    let port = match port {
        Some(p) => Some(p.parse::<u16>().ok().filter(|p| *p > 0).ok_or("bad port")?),
        None => None,
    };
    if host.is_empty() || !host.chars().all(|c| c.is_ascii_alphanumeric() || "-.[]:".contains(c)) {
        return Err("bad host name".into());
    }
    if path.split('/').any(|c| c == "..") {
        return Err("no .. in the path".into());
    }
    Ok(Url { scheme, host: host.to_ascii_lowercase(), port, path: path.trim_end_matches('/').to_string() })
}

impl Url {
    pub fn to_string(&self) -> String {
        if self.scheme == "file" {
            return format!("file://{}", self.path);
        }
        format!("{}://{}{}{}", self.scheme, self.host, self.port.map(|p| format!(":{p}")).unwrap_or_default(), self.path)
    }
}

pub fn private_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    ip.is_private() || ip.is_loopback() || ip.is_link_local() || (o[0] == 100 && (64..128).contains(&o[1]))
}

pub fn private_v6(ip: Ipv6Addr) -> bool {
    let s = ip.segments()[0];
    ip.is_loopback() || (s & 0xfe00) == 0xfc00 || (s & 0xffc0) == 0xfe80
}

/// Whether plain http is acceptable to this host.
pub fn private_host(host: &str) -> bool {
    if host == "localhost" {
        return true;
    }
    if let Some(v6) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        return v6.parse::<Ipv6Addr>().is_ok_and(private_v6);
    }
    host.parse::<Ipv4Addr>().is_ok_and(private_v4)
}

/// Validates and normalises a server or cache URL. `allow_file` only for
/// the laptop dry run.
pub fn check(s: &str, allow_file: bool) -> Result<String, String> {
    let u = parse(s)?;
    match u.scheme.as_str() {
        "https" => Ok(u.to_string()),
        "http" if private_host(&u.host) => Ok(u.to_string()),
        "http" => Err("plain http:// is only allowed on your local or private network; use https:// for anything else".into()),
        "file" if allow_file => Ok(u.to_string()),
        _ => Err("file:// addresses are not allowed on the phone".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted() {
        let nb = format!("http://{}.71.0.9:8742", 100);
        assert_eq!(check(&format!("{nb}/"), false).unwrap(), nb);
        for u in ["http://192.168.1.10:8742", "http://10.0.0.2", "http://172.20.1.1:80/ota", "http://127.0.0.1:18742",
                  "http://localhost:8742", "http://[::1]:8742", "http://[fd00::5]:1", "https://updates.example.org/fp6",
                  "HTTPS://Updates.Example.org/fp6/"] {
            assert!(check(u, false).is_ok(), "{u}");
        }
        assert_eq!(check("HTTPS://Updates.Example.org/fp6/", false).unwrap(), "https://updates.example.org/fp6");
        assert!(check("file:///srv/ota", true).is_ok());
    }

    #[test]
    fn refused() {
        for u in ["http://example.org", "http://8.8.8.8", "http://172.32.0.1", "http://100.128.0.1", "http://[2001:db8::1]",
                  "ftp://10.0.0.1", "10.0.0.1:8742", "http://10.0.0.1:0", "http://10.0.0.1:99999", "http://user:pw@10.0.0.1",
                  "http://10.0.0.1/?x=1", "http://10.0.0.1/a/../b", "http://10.0.0.1 /x", "", "http://", "http://laptop.local",
                  "file:///etc"] {
            assert!(check(u, false).is_err(), "{u}");
        }
    }
}
