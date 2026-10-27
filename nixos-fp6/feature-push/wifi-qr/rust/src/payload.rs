// SPDX-License-Identifier: GPL-3.0-only
//! Wi-Fi network QR payloads in the ZXing "WIFI:" format, as Android and
//! iOS write and read them:
//!
//! ```text
//! WIFI:T:WPA;S:<ssid>;P:<password>;H:<true|false>;;
//! ```
//!
//! Field values escape `\ ; , : "` with a backslash. `T` is `WPA`
//! (WPA/WPA2 personal, also WPA2/WPA3 transition), `SAE` (WPA3 personal),
//! `WEP` or `nopass`. Enterprise (802.1X) codes (`T:WPA2-EAP` or the
//! `E:`/`I:`/`A:`/`PH2:` fields) are recognised but not supported.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Security {
    Open,
    Wep,
    /// WPA/WPA2 personal (NetworkManager key-mgmt wpa-psk).
    Wpa,
    /// WPA3 personal (key-mgmt sae).
    Sae,
}

impl Security {
    /// The `T:` value written for this security type.
    pub fn qr_type(self) -> &'static str {
        match self {
            Security::Open => "nopass",
            Security::Wep => "WEP",
            Security::Wpa => "WPA",
            Security::Sae => "SAE",
        }
    }

    pub fn code(self) -> i32 {
        match self {
            Security::Open => 0,
            Security::Wep => 1,
            Security::Wpa => 2,
            Security::Sae => 3,
        }
    }

    pub fn from_code(c: i32) -> Option<Security> {
        Some(match c {
            0 => Security::Open,
            1 => Security::Wep,
            2 => Security::Wpa,
            3 => Security::Sae,
            _ => return None,
        })
    }
}

/// A network as encoded in / decoded from a QR code.
#[derive(Clone, PartialEq, Eq)]
pub struct WifiNetwork {
    pub ssid: String,
    pub security: Security,
    /// Empty for open networks.
    pub password: String,
    pub hidden: bool,
}

// Never print the password (logs, panics, test failures).
impl fmt::Debug for WifiNetwork {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WifiNetwork")
            .field("ssid", &self.ssid)
            .field("security", &self.security)
            .field("password", &format_args!("<{} bytes>", self.password.len()))
            .field("hidden", &self.hidden)
            .finish()
    }
}

/// WEP key as NetworkManager wants it: a raw key (5/13/16 ASCII or
/// 10/26/32 hex characters) or a passphrase hashed by NM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WepKeyType {
    Key = 1,
    Passphrase = 2,
}

pub fn wep_key_type(key: &str) -> WepKeyType {
    let hex = key.bytes().all(|b| b.is_ascii_hexdigit());
    match key.len() {
        10 | 26 | 32 if hex => WepKeyType::Key,
        5 | 13 | 16 if key.is_ascii() => WepKeyType::Key,
        _ => WepKeyType::Passphrase,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// Not a "WIFI:" code (a URL, text, a contact, ...).
    NotWifi,
    /// A WIFI: code without a network name.
    MissingSsid,
    /// More than 32 bytes: not a valid 802.11 SSID.
    SsidTooLong,
    /// 802.1X (enterprise) network; the string is the EAP method if given.
    Enterprise(String),
    /// A `T:` value this phone does not know.
    UnknownSecurity(String),
    /// A password that the security type cannot use (WPA: 8-63 characters
    /// or 64 hex digits).
    InvalidPassword,
    /// Secured network without a password.
    MissingPassword,
}

impl ParseError {
    pub fn code(&self) -> i32 {
        match self {
            ParseError::NotWifi => 1,
            ParseError::MissingSsid => 2,
            ParseError::SsidTooLong => 3,
            ParseError::Enterprise(_) => 4,
            ParseError::UnknownSecurity(_) => 5,
            ParseError::InvalidPassword => 6,
            ParseError::MissingPassword => 7,
        }
    }
}

const SPECIAL: &[char] = &['\\', ';', ',', ':', '"'];

/// Escapes `\ ; , : "` with a backslash (ZXing / Android / iOS).
pub fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 4);
    for c in value.chars() {
        if SPECIAL.contains(&c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The QR payload for a network.
pub fn encode(net: &WifiNetwork) -> String {
    let mut out = String::from("WIFI:T:");
    out.push_str(net.security.qr_type());
    out.push_str(";S:");
    out.push_str(&escape(&net.ssid));
    if net.security != Security::Open {
        out.push_str(";P:");
        out.push_str(&escape(&net.password));
    }
    out.push_str(";H:");
    out.push_str(if net.hidden { "true" } else { "false" });
    out.push_str(";;");
    out
}

/// Splits the field list at unescaped `;` and each field at its first
/// unescaped `:`; values are unescaped. A value enclosed in unescaped double
/// quotes loses them (the ZXing spec lets writers quote values that look
/// like hex).
fn fields(body: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut key = String::new();
    let mut value = String::new();
    let mut in_value = false;
    // positions (in `value`) of unescaped quotes, to strip a quoted value
    let mut quote_first = false;
    let mut quote_last = false;
    let mut chars = body.chars();
    let finish = |key: &mut String, value: &mut String, in_value: &mut bool, qf: &mut bool, ql: &mut bool, out: &mut Vec<(String, String)>| {
        if *in_value || !key.is_empty() {
            let mut v = std::mem::take(value);
            if *qf && *ql && v.chars().count() >= 2 {
                v.remove(0);
                v.pop();
            }
            out.push((std::mem::take(key).trim().to_ascii_uppercase(), v));
        }
        *in_value = false;
        *qf = false;
        *ql = false;
    };
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                // escaped character (a trailing lone backslash is kept)
                let lit = chars.next().unwrap_or('\\');
                if in_value {
                    value.push(lit);
                    quote_last = false;
                } else {
                    key.push(lit);
                }
            }
            ';' => finish(&mut key, &mut value, &mut in_value, &mut quote_first, &mut quote_last, &mut out),
            ':' if !in_value => in_value = true,
            '"' if in_value => {
                if value.is_empty() {
                    quote_first = true;
                }
                quote_last = true;
                value.push('"');
            }
            _ => {
                if in_value {
                    value.push(c);
                    quote_last = false;
                } else {
                    key.push(c);
                }
            }
        }
    }
    finish(&mut key, &mut value, &mut in_value, &mut quote_first, &mut quote_last, &mut out);
    out
}

fn valid_wpa_psk(p: &str) -> bool {
    let n = p.len();
    (n == 64 && p.bytes().all(|b| b.is_ascii_hexdigit()))
        || ((8..=63).contains(&n) && p.bytes().all(|b| (0x20..=0x7e).contains(&b)))
}

/// The network name of a WIFI: payload, also when it cannot be joined
/// (for messages such as "<ssid> uses enterprise sign-in").
pub fn ssid_of(text: &str) -> Option<String> {
    let text = text.trim_start_matches('\u{feff}').trim();
    if !text.get(..5)?.eq_ignore_ascii_case("WIFI:") {
        return None;
    }
    fields(&text[5..]).into_iter().find(|(k, _)| k == "S").map(|(_, v)| v).filter(|s| !s.is_empty())
}

/// Parses a scanned payload.
pub fn parse(text: &str) -> Result<WifiNetwork, ParseError> {
    let text = text.trim_start_matches('\u{feff}').trim();
    let Some(prefix) = text.get(..5) else { return Err(ParseError::NotWifi) };
    if !prefix.eq_ignore_ascii_case("WIFI:") {
        return Err(ParseError::NotWifi);
    }
    let mut ssid = None;
    let mut t = String::new();
    let mut password = String::new();
    let mut hidden = false;
    let mut eap = None;
    for (k, v) in fields(&text[5..]) {
        match k.as_str() {
            "S" => ssid = Some(v),
            "T" => t = v,
            "P" => password = v,
            "H" => hidden = v.eq_ignore_ascii_case("true"),
            "E" => eap = Some(v),
            "I" | "A" | "PH2" if eap.is_none() => eap = Some(String::new()),
            _ => {}
        }
    }
    let ssid = ssid.filter(|s| !s.is_empty()).ok_or(ParseError::MissingSsid)?;
    if ssid.len() > 32 {
        return Err(ParseError::SsidTooLong);
    }
    let tu = t.trim().to_ascii_uppercase();
    if tu.contains("EAP") || tu == "802.1X" || eap.is_some() {
        return Err(ParseError::Enterprise(eap.unwrap_or_default()));
    }
    let security = match tu.as_str() {
        // Some writers omit T for secured networks: a password means WPA.
        "" if !password.is_empty() => Security::Wpa,
        "" | "NOPASS" | "NONE" | "OPEN" => Security::Open,
        "WEP" => Security::Wep,
        "WPA" | "WPA2" | "WPA/WPA2" | "WPA2/WPA3" | "WPA-PSK" | "WPA2-PSK" => Security::Wpa,
        "SAE" | "WPA3" | "WPA3-SAE" => Security::Sae,
        _ => return Err(ParseError::UnknownSecurity(t)),
    };
    match security {
        Security::Open => password.clear(),
        Security::Wpa if password.is_empty() => return Err(ParseError::MissingPassword),
        Security::Wpa if !valid_wpa_psk(&password) => return Err(ParseError::InvalidPassword),
        Security::Sae | Security::Wep if password.is_empty() => return Err(ParseError::MissingPassword),
        Security::Sae if password.len() > 256 => return Err(ParseError::InvalidPassword),
        _ => {}
    }
    Ok(WifiNetwork { ssid, security, password, hidden })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn net(ssid: &str, security: Security, password: &str, hidden: bool) -> WifiNetwork {
        WifiNetwork { ssid: ssid.into(), security, password: password.into(), hidden }
    }

    #[test]
    fn escaping() {
        assert_eq!(escape("Test;Net"), "Test\\;Net");
        assert_eq!(escape("p@ss:word,1"), "p@ss\\:word\\,1");
        assert_eq!(escape(r#"a\b"c"#), r#"a\\b\"c"#);
        assert_eq!(escape("plain"), "plain");
    }

    #[test]
    fn encode_wpa() {
        let n = net("Test;Net", Security::Wpa, "p@ss:word,1", false);
        assert_eq!(encode(&n), "WIFI:T:WPA;S:Test\\;Net;P:p@ss\\:word\\,1;H:false;;");
    }

    #[test]
    fn encode_sae_hidden_open_wep() {
        assert_eq!(encode(&net("Lab Net", Security::Sae, "longpassword", true)),
                   "WIFI:T:SAE;S:Lab Net;P:longpassword;H:true;;");
        assert_eq!(encode(&net("Cafe \"Guest\"", Security::Open, "", false)),
                   "WIFI:T:nopass;S:Cafe \\\"Guest\\\";H:false;;");
        assert_eq!(encode(&net("Old", Security::Wep, "abcde", false)), "WIFI:T:WEP;S:Old;P:abcde;H:false;;");
    }

    #[test]
    fn round_trips() {
        for n in [
            net("Test;Net", Security::Wpa, "p@ss:word,1", false),
            net("Back\\slash", Security::Wpa, "quote\"d;pass", true),
            net("Ünïcødé 📶", Security::Sae, "s", false),
            net("::;;,,", Security::Wpa, "\\\\\\\\;;;;", false),
            net("\"ABCD\"", Security::Wpa, "\"12345678\"", false),
            net("Open", Security::Open, "", true),
            net("Wep", Security::Wep, "0123456789", false),
        ] {
            let p = encode(&n);
            assert_eq!(parse(&p).as_ref(), Ok(&n), "payload {p}");
        }
    }

    #[test]
    fn parses_other_writers() {
        // field order, lower case, no trailing ";;", quoted hex-looking values
        let n = parse("wifi:S:\"ABCD\";t:wpa;p:\"0123456789\"").unwrap();
        assert_eq!(n.ssid, "ABCD");
        assert_eq!(n.security, Security::Wpa);
        assert_eq!(n.password, "0123456789");
        assert!(!n.hidden);
        // H:true, WPA2 alias
        let n = parse("WIFI:S:Test\\;Net;T:WPA2;P:p@ss\\:word\\,1;H:true;;").unwrap();
        assert_eq!((n.ssid.as_str(), n.password.as_str(), n.hidden), ("Test;Net", "p@ss:word,1", true));
        // WPA3 aliases
        assert_eq!(parse("WIFI:T:WPA3;S:x;P:y;;").unwrap().security, Security::Sae);
        assert_eq!(parse("WIFI:T:SAE;S:x;P:y;;").unwrap().security, Security::Sae);
        // open without T; a password without T means WPA
        assert_eq!(parse("WIFI:S:Free;;").unwrap().security, Security::Open);
        assert_eq!(parse("WIFI:S:Free;P:abcdefgh;;").unwrap().security, Security::Wpa);
        // nopass ignores a stray password
        let n = parse("WIFI:T:nopass;S:Free;P:junk;;").unwrap();
        assert_eq!((n.security, n.password.as_str()), (Security::Open, ""));
        // unknown fields are ignored
        assert_eq!(parse("WIFI:T:WPA;S:x;P:abcdefgh;R:1;K:abc;;").unwrap().ssid, "x");
        // a quote inside a value stays
        assert_eq!(parse("WIFI:T:nopass;S:a\"b;;").unwrap().ssid, "a\"b");
    }

    #[test]
    fn rejects() {
        assert_eq!(parse("https://example.org/"), Err(ParseError::NotWifi));
        assert_eq!(parse("WIFI"), Err(ParseError::NotWifi));
        assert_eq!(parse(""), Err(ParseError::NotWifi));
        assert_eq!(parse("WIFI:T:WPA;P:abcdefgh;;"), Err(ParseError::MissingSsid));
        assert_eq!(parse("WIFI:T:WPA;S:;P:abcdefgh;;"), Err(ParseError::MissingSsid));
        assert_eq!(parse(&format!("WIFI:T:nopass;S:{};;", "x".repeat(33))), Err(ParseError::SsidTooLong));
        assert_eq!(parse("WIFI:T:WPA;S:x;P:short;;"), Err(ParseError::InvalidPassword));
        assert_eq!(parse(&format!("WIFI:T:WPA;S:x;P:{};;", "z".repeat(64))), Err(ParseError::InvalidPassword));
        assert!(parse(&format!("WIFI:T:WPA;S:x;P:{};;", "a1".repeat(32))).is_ok(), "64 hex digits = raw PSK");
        assert_eq!(parse("WIFI:T:WPA;S:x;;"), Err(ParseError::MissingPassword));
        assert_eq!(parse("WIFI:T:WEP;S:x;;"), Err(ParseError::MissingPassword));
        assert_eq!(parse("WIFI:T:WAPI;S:x;P:abcdefgh;;"), Err(ParseError::UnknownSecurity("WAPI".into())));
    }

    #[test]
    fn enterprise_is_unsupported() {
        assert_eq!(parse("WIFI:T:WPA2-EAP;S:Corp;E:PEAP;PH2:MSCHAPV2;I:someone;P:abcdefgh;;"),
                   Err(ParseError::Enterprise("PEAP".into())));
        assert_eq!(parse("WIFI:T:WPA;S:Corp;I:someone;P:abcdefgh;;"), Err(ParseError::Enterprise(String::new())));
        assert_eq!(parse("WIFI:T:WPA2-EAP;S:Corp;;"), Err(ParseError::Enterprise(String::new())));
    }

    #[test]
    fn ssid_of_unsupported() {
        assert_eq!(ssid_of("WIFI:T:WPA2-EAP;S:Corp\\;Net;E:PEAP;;").as_deref(), Some("Corp;Net"));
        assert_eq!(ssid_of("http://x"), None);
        assert_eq!(ssid_of("WIFI:T:WPA;;"), None);
    }

    #[test]
    fn wep_keys() {
        assert_eq!(wep_key_type("abcde"), WepKeyType::Key);
        assert_eq!(wep_key_type("0123456789"), WepKeyType::Key);
        assert_eq!(wep_key_type("0123456789abcdef0123456789"), WepKeyType::Key);
        assert_eq!(wep_key_type("a longer passphrase"), WepKeyType::Passphrase);
    }

    #[test]
    fn debug_hides_password() {
        let s = format!("{:?}", net("Test;Net", Security::Wpa, "p@ss:word,1", false));
        assert!(!s.contains("p@ss"), "{s}");
        assert!(s.contains("11 bytes"));
    }
}
