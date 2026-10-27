//! Conservative, reusable text redaction. Deliberately favors false positives.
use regex::{Captures, Regex};
use std::{net::Ipv6Addr, sync::LazyLock};

static RULES: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    [
        // Whole lines: secrets can contain spaces, quotes, and multiple key=value pairs.
        (r#"(?im)^.*\b(?:[\w.-]*(?:password|passwd|passphrase|secret|token|credential|setup[_-]?key|api[_-]?key|access[_-]?key|private[_-]?key|public[_-]?key|preshared[_-]?key)[\w.-]*|password|passwd|pwd|passphrase|psk|secret|token|access[_-]?token|refresh[_-]?token|authorization|cookie|credential|api[_-]?key|private[_-]?key|public[_-]?key|preshared[_-]?key|wireguard[_-]?key|netbird[_-]?key|imei\d*|iccid|imsi|subscriberidentity|subscriber[_-]?id|serial(?:number)?|(?:phone|mobile)[_-]?number|msisdn|ownnumbers|ssid|bssid|latitude|longitude|lat|lon|lng|location|gps)\b["']?\s*(?:=|:|\bis\b|\s)\s*[^\r\n]*"#, "[REDACTED sensitive field]"),
        (r#"(?im)^.*\b(?:ssid|bssid)\b[^\r\n]*"#, "[REDACTED network name]"),
        (r"(?i)\b[a-z0-9.!#$%&'*+/=?^_`{|}~-]+@[a-z0-9-]+(?:\.[a-z0-9-]+)+\b", "[EMAIL]"),
        (r"(?i)\b(?:[0-9a-f]{2}[:-]){5}[0-9a-f]{2}\b|\b(?:[0-9a-f]{4}\.){2}[0-9a-f]{4}\b", "[MAC]"),
        (r"\b(?:\d{1,3}\.){3}\d{1,3}(?:/\d{1,2})?\b", "[IPv4]"),
        // Decimal coordinates, including unlabeled pairs, N/S/E/W forms.
        (r"(?i)[+-]?\d{1,3}\.\d{3,}\s*°?\s*[NS]?\s*[,; /]\s*[+-]?\d{1,3}\.\d{3,}\s*°?\s*[EW]?", "[GPS]"),
        (r#"(?i)\d{1,3}\s*°\s*\d{1,2}['′]\s*\d{1,2}(?:\.\d+)?["″]?\s*[NSEW]"#, "[GPS]"),
        // International, domestic, test country codes, extensions; also device IDs.
        (r"(?:\+|\b)(?:\d[ \t()./-]*){6,19}\d(?:\s*(?:ext\.?|x)\s*\d+)?\b", "[NUMBER]"),
        (r"[A-Za-z0-9+/]{43}=", "[KEY]"),
        (r"\beyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\b", "[TOKEN]"),
        (r"(?i)\b[0-9a-f]{32,}\b", "[KEY]"),
        (r"/home/[^/\s]+", "/home/[USER]"),
    ].into_iter().map(|(p, r)| (Regex::new(p).unwrap(), r)).collect()
});
static ANSI: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]").unwrap());
static IP_TOKEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[0-9A-Fa-f:.]+(?:%[a-zA-Z0-9_.-]+)?(?:/\d{1,3})?").unwrap());

pub fn redact(input: &str) -> String {
    let clean = ANSI.replace_all(input, "");
    let clean: String = clean
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect();
    let mut text = IP_TOKEN
        .replace_all(&clean, |c: &Captures<'_>| {
            let token = &c[0];
            let addr = token
                .split(['%', '/'])
                .next()
                .unwrap_or(token)
                .trim_end_matches('.');
            // Trim a log delimiter after an IPv6 address, but preserve valid trailing ::.
            if addr.parse::<Ipv6Addr>().is_ok()
                || addr
                    .strip_prefix(':')
                    .is_some_and(|s| s.parse::<Ipv6Addr>().is_ok())
                || addr
                    .strip_suffix(':')
                    .is_some_and(|s| s.parse::<Ipv6Addr>().is_ok())
            {
                "[IPv6]".to_string()
            } else {
                token.to_string()
            }
        })
        .into_owned();
    for (pattern, replacement) in RULES.iter() {
        text = pattern.replace_all(&text, *replacement).into_owned();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn synthetic_sensitive_values_disappear() {
        for value in [
            "+40 712 345 678",
            "0712-345-678",
            "+999 (123) 456-789",
            "+1 (202) 555-0123",
            "020 7946 0958",
            "123456789012345",
            "89999012345678901234",
            "001019876543210",
            "aa:bb:cc:dd:ee:ff",
            "AA-BB-CC-DD-EE-FF",
            "aabb.ccdd.eeff",
            "192.0.2.42",
            "2001:db8::1234",
            "fe80::1234%wlan0",
            "::ffff:192.0.2.4",
            "::1",
            "2001:db8:0:0:1:2:3:4/64",
            "2001:db8::1.",
            "+40 (0) 712 345 678",
            "0040 712 345 678",
            "person@example.invalid",
            "12.345600, 65.432100",
            "44°25'36.5\"N",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        ] {
            let text = format!("before {value} after");
            assert!(
                !redact(&text).contains(value),
                "not redacted: {value} -> {}",
                redact(&text)
            );
        }
    }
    #[test]
    fn sensitive_fields_hide_entire_values() {
        for key in [
            "SSID",
            "BSSID",
            "Password",
            "access_token",
            "Authorization",
            "PrivateKey",
            "PublicKey",
            "PresharedKey",
            "IMSI",
            "IMEI",
            "ICCID",
            "SubscriberIdentity",
            "OwnNumbers",
            "latitude",
            "lat",
            "lon",
            "lng",
            "longitude",
            "api-key",
            "NetBird_key",
            "client_secret",
            "auth_token",
            "wifi_password",
            "NB_SETUP_KEY",
            "AWS_ACCESS_KEY_ID",
        ] {
            for separator in ["=", ": ", " is "] {
                let input = format!("{key}{separator}Synthetic Secret With Spaces\nhealthy=ok");
                let out = redact(&input);
                assert!(!out.contains("Synthetic"), "{key}{separator}: {out}");
                assert!(out.contains("healthy=ok"));
            }
        }
        assert!(!redact("joining SSID \"Synthetic Cafe\"").contains("Cafe"));
        assert!(!redact("\"Password\": \"synthetic\"").contains("synthetic"));
    }
    #[test]
    fn useful_diagnostics_survive_and_redaction_is_idempotent() {
        let text = "uptime=123.45\nGPU fault -5\nvoltage_now=3900000\nfailed units: 2\nSSID=private\npeer=2001:db8::1\n";
        let out = redact(text);
        assert!(out.contains("GPU fault -5"));
        assert!(out.contains("uptime=123.45"));
        assert_eq!(out, redact(&out));
    }
}
