// SPDX-License-Identifier: GPL-3.0-only
//! Wi-Fi MAC address policy.
//!
//! NetworkManager defaults (privacy-lock module): scans use a random MAC
//! (wifi.scan-rand-mac-address=yes), connections a stable per-network
//! random MAC (wifi.cloned-mac-address=stable). Networks saved before the
//! module was enabled are pinned to "preserve" once, before NetworkManager
//! starts, so they keep the address the router knows (DHCP lease, MAC
//! filter); Settings can switch each of them.

/// Settings value -> NetworkManager 802-11-wireless.cloned-mac-address.
/// "" = no per-connection value (the global stable default applies).
pub fn nm_value(mode: &str) -> Option<&'static str> {
    match mode {
        "default" => Some(""),
        "stable" => Some("stable"),
        "random" => Some("random"),
        "device" => Some("preserve"),
        _ => None,
    }
}

/// NetworkManager value -> Settings value.
pub fn mode_of(nm: &str) -> &'static str {
    match nm.trim() {
        "" => "default",
        "stable" => "stable",
        "random" => "random",
        "preserve" | "permanent" => "device",
        _ => "custom",
    }
}

/// One-time migration of a saved keyfile: Wi-Fi client profiles without a
/// cloned-mac-address get "preserve". Returns the new text if it changed.
pub fn migrate_keyfile(text: &str) -> Option<String> {
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
                out.push_str("cloned-mac-address=preserve\n");
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
        out.push_str("[wifi]\ncloned-mac-address=preserve\n");
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "[connection]\nid=Home\nuuid=00000000-test\ntype=wifi\n\n[wifi]\nmode=infrastructure\nssid=Home\n\n[wifi-security]\nkey-mgmt=wpa-psk\npsk=TESTPSK\n";

    #[test]
    fn pins_saved_client_networks() {
        let new = migrate_keyfile(HOME).unwrap();
        assert!(new.contains("[wifi]\ncloned-mac-address=preserve\nmode=infrastructure"));
        assert!(new.contains("psk=TESTPSK"), "secrets kept");
        assert_eq!(migrate_keyfile(&new), None, "idempotent");
    }

    #[test]
    fn leaves_others_alone() {
        let ap = HOME.replace("mode=infrastructure", "mode=ap");
        assert_eq!(migrate_keyfile(&ap), None, "hotspot");
        let eth = "[connection]\nid=x\ntype=ethernet\n";
        assert_eq!(migrate_keyfile(eth), None);
        let set = HOME.replace("mode=infrastructure", "mode=infrastructure\ncloned-mac-address=random");
        assert_eq!(migrate_keyfile(&set), None, "explicit choice kept");
        let nogroup = "[connection]\nid=y\ntype=wifi\n";
        assert!(migrate_keyfile(nogroup).unwrap().ends_with("[wifi]\ncloned-mac-address=preserve\n"));
    }

    #[test]
    fn modes() {
        for m in ["default", "stable", "random", "device"] {
            assert_eq!(mode_of(nm_value(m).unwrap()), m);
        }
        assert_eq!(nm_value("bogus"), None);
        assert_eq!(mode_of("permanent"), "device");
    }
}
