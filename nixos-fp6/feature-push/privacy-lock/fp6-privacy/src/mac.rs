// SPDX-License-Identifier: GPL-3.0-only
//! Wi-Fi MAC address policy.
//!
//! NetworkManager defaults (privacy-lock module): scans use a random MAC
//! (wifi.scan-rand-mac-address=yes), connections a stable per-network
//! random MAC (wifi.cloned-mac-address=stable). Networks saved before the
//! module was enabled keep a **fixed** address: the FP6's ath11k has no
//! factory MAC (no local-mac-address in the DT), its firmware makes up
//! 00:03:7f:12:XX:XX anew on every boot, so "preserve"/"permanent" would
//! show the router a new device after each reboot. A fixed network gets an
//! explicit cloned-mac-address (the address it had when it was pinned), the
//! same on every boot; Settings calls that mode "device" ("Fixed address").

/// Settings value -> NetworkManager 802-11-wireless.cloned-mac-address.
/// "" = no per-connection value (the global stable default applies).
pub fn nm_value(mode: &str) -> Option<&'static str> {
    match mode {
        "default" => Some(""),
        "stable" => Some("stable"),
        "random" => Some("random"),
        // "device" needs the network's own fixed MAC: see fixed_value().
        _ => None,
    }
}

/// "aa:bb:cc:dd:ee:ff" (any case), unicast, not all zero.
pub fn is_mac(s: &str) -> bool {
    let parts: Vec<&str> = s.trim().split(':').collect();
    parts.len() == 6
        && parts.iter().all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()))
        && u8::from_str_radix(parts[0], 16).is_ok_and(|b| b & 1 == 0)
        && parts.iter().any(|p| *p != "00")
}

/// The explicit cloned MAC for a fixed ("device") network: keep the one it
/// already has, else take `current` (this boot's interface address).
pub fn fixed_value(nm: &str, current: &str) -> Option<String> {
    if is_mac(nm) {
        return Some(nm.trim().to_ascii_lowercase());
    }
    is_mac(current).then(|| current.trim().to_ascii_lowercase())
}

/// NetworkManager value -> Settings value.
pub fn mode_of(nm: &str) -> &'static str {
    match nm.trim() {
        "" => "default",
        "stable" => "stable",
        "random" => "random",
        // A fixed address, or a pin from before fixed addresses (converted
        // to an explicit MAC by fp6-privacyd).
        "preserve" | "permanent" => "device",
        v if is_mac(v) => "device",
        _ => "custom",
    }
}

/// One-time migration of a saved keyfile: Wi-Fi client profiles without a
/// cloned-mac-address get `fixed` (this boot's interface MAC, so the address
/// stays the same on later boots), or "preserve" when it is not known yet
/// (fp6-privacyd then fixes it at login). Returns the new text if changed.
pub fn migrate_keyfile(text: &str, fixed: Option<&str>) -> Option<String> {
    let value = fixed.filter(|m| is_mac(m)).map(|m| m.trim().to_ascii_lowercase()).unwrap_or_else(|| "preserve".into());
    let mut section = String::new();
    let mut is_wifi = false;
    let mut is_ap = false;
    let mut has_clone = false;
    let mut has_wifi_group = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            section = t[1..t.len() - 1].to_owned();
            if section == "wifi" || section == "802-11-wireless" {
                has_wifi_group = true;
            }
            continue;
        }
        let Some((k, v)) = t.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim());
        match section.as_str() {
            "connection" if k == "type" => is_wifi = v == "wifi" || v == "802-11-wireless",
            "wifi" | "802-11-wireless" if k == "mode" => is_ap = v == "ap" || v == "adhoc" || v == "mesh",
            "wifi" | "802-11-wireless" if k == "cloned-mac-address" => has_clone = true,
            _ => {}
        }
    }
    if !is_wifi || is_ap || has_clone {
        return None;
    }
    let mut out = String::with_capacity(text.len() + 40);
    let mut inserted = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            let current = &t[1..t.len() - 1];
            out.push_str(line);
            out.push('\n');
            if !inserted && (current == "wifi" || current == "802-11-wireless") {
                out.push_str(&format!("cloned-mac-address={value}\n"));
                inserted = true;
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    if !has_wifi_group {
        if !out.ends_with("\n\n") {
            out.push('\n');
        }
        out.push_str(&format!("[wifi]\ncloned-mac-address={value}\n"));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "[connection]\nid=Home\nuuid=00000000-test\ntype=wifi\n\n[wifi]\nmode=infrastructure\nssid=Home\n\n[wifi-security]\nkey-mgmt=wpa-psk\npsk=TESTPSK\n";

    #[test]
    fn pins_saved_client_networks() {
        let new = migrate_keyfile(HOME, Some("00:03:7F:12:AB:AB")).unwrap();
        assert!(new.contains("[wifi]\ncloned-mac-address=00:03:7f:12:ab:ab\nmode=infrastructure"));
        assert!(new.contains("psk=TESTPSK"), "secrets kept");
        assert_eq!(migrate_keyfile(&new, Some("00:03:7f:12:cd:cd")), None, "idempotent");
        let unknown = migrate_keyfile(HOME, None).unwrap();
        assert!(unknown.contains("cloned-mac-address=preserve"), "MAC not known yet");
        assert!(migrate_keyfile(HOME, Some("bogus")).unwrap().contains("=preserve"));
    }

    #[test]
    fn leaves_others_alone() {
        let ap = HOME.replace("mode=infrastructure", "mode=ap");
        assert_eq!(migrate_keyfile(&ap, None), None, "hotspot");
        let eth = "[connection]\nid=x\ntype=ethernet\n";
        assert_eq!(migrate_keyfile(eth, None), None);
        let set = HOME.replace("mode=infrastructure", "mode=infrastructure\ncloned-mac-address=random");
        assert_eq!(migrate_keyfile(&set, None), None, "explicit choice kept");
        let nogroup = "[connection]\nid=y\ntype=wifi\n";
        assert!(migrate_keyfile(nogroup, None).unwrap().ends_with("[wifi]\ncloned-mac-address=preserve\n"));
    }

    #[test]
    fn modes() {
        for m in ["default", "stable", "random"] {
            assert_eq!(mode_of(nm_value(m).unwrap()), m);
        }
        assert_eq!(nm_value("device"), None, "needs fixed_value");
        assert_eq!(nm_value("bogus"), None);
        assert_eq!(mode_of("permanent"), "device");
        assert_eq!(mode_of("00:03:7f:12:ab:ab"), "device");
    }

    #[test]
    fn fixed_addresses() {
        assert!(is_mac("00:03:7f:12:ab:ab") && is_mac("02:AA:bb:cc:dd:ee"));
        assert!(!is_mac("01:00:5e:00:00:01"), "multicast");
        assert!(!is_mac("00:00:00:00:00:00") && !is_mac("preserve") && !is_mac("00:03:7f:12:ab"));
        // keeps an existing fixed MAC, never replaces it with this boot's
        assert_eq!(fixed_value("00:03:7f:12:ab:ab", "00:03:7f:12:cd:cd").as_deref(), Some("00:03:7f:12:ab:ab"));
        assert_eq!(fixed_value("preserve", "00:03:7F:12:CD:CD").as_deref(), Some("00:03:7f:12:cd:cd"));
        assert_eq!(fixed_value("", "garbage"), None);
    }
}
