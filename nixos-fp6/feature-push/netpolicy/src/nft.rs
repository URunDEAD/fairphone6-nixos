use crate::config::{valid_cgroup, valid_id, valid_iface};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Scope {
    pub app: String,
    pub path: String,
    pub inode: u64,
    pub allowed: bool,
}
/// Per-app network permission (app-permissions: Settings > Privacy > App
/// permissions > <app> > Network). Independent of data saver and of the
/// metered/unmetered classification; applies on every interface but `lo`.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct AppNet {
    /// Wi-Fi and every other link that is neither mobile nor VPN.
    pub wifi: bool,
    pub mobile: bool,
    pub vpn: bool,
    /// Private, link-local and multicast destinations over Wi-Fi/other links.
    pub local: bool,
}
impl Default for AppNet {
    fn default() -> Self {
        Self {
            wifi: true,
            mobile: true,
            vpn: true,
            local: true,
        }
    }
}
impl AppNet {
    pub fn restricted(&self) -> bool {
        !(self.wifi && self.mobile && self.vpn && self.local)
    }
}
/// One running instance (cgroup) of an app with restricted network access.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PermScope {
    pub app: String,
    pub path: String,
    pub inode: u64,
    pub access: AppNet,
}
/// Mobile data links by name, besides NetworkManager modems and the
/// mobile adapter's interface (Qualcomm rmnet/IPA, qmapmux, WWAN).
pub const MOBILE_PATTERNS: &[&str] = &["rmnet*", "qmapmux*", "wwan*"];
/// VPN links by name, besides NetworkManager tun/ip-tunnel/WireGuard
/// devices (OpenVPN/other tun, tap, WireGuard, NetBird's wt0).
pub const VPN_PATTERNS: &[&str] = &["tun*", "tap*", "wg*", "wt*"];
pub const LAN4: &str =
    "10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, 169.254.0.0/16, 224.0.0.0/4, 255.255.255.255";
pub const LAN6: &str = "fc00::/7, fe80::/10, ff00::/8";
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Rules {
    pub uid: u32,
    pub interfaces: Vec<String>,
    pub scopes: Vec<Scope>,
    pub system_allow: Vec<String>,
    pub restrict_unknown: bool,
    /// App network permission: restricted app instances.
    #[serde(default)]
    pub perm: Vec<PermScope>,
    /// Mobile interfaces known by name (NM modems, mobile adapter).
    #[serde(default)]
    pub mobile_interfaces: Vec<String>,
    /// VPN interfaces known by name (NM tunnel devices).
    #[serde(default)]
    pub vpn_interfaces: Vec<String>,
}
/// Chain names of the app network permission: one per restricted app, by
/// position in the sorted app list (app ids can be longer than nft names).
pub fn perm_chain(index: usize, dir: &str) -> String {
    format!("perm_{index}_{dir}")
}
fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\"))
}
fn socket(path: &str) -> String {
    format!(
        "socket cgroupv2 level {} {}",
        path.split('/').count(),
        quote(path)
    )
}
pub fn counter_name(app: &str, dir: &str) -> String {
    // Hex encoding is injective, including dots/dashes/underscores.
    format!(
        "a{}_{}",
        app.bytes().map(|b| format!("{b:02x}")).collect::<String>(),
        dir
    )
}
impl Rules {
    /// Rebuild chains atomically while retaining named counters and their values.
    /// `existing` is the counter name set obtained from the current owned table.
    pub fn render(
        &self,
        existing: &std::collections::BTreeSet<String>,
        table_exists: bool,
    ) -> Result<String> {
        self.render_with(existing, table_exists, 0)
    }
    /// Restricted apps in chain order.
    pub fn perm_apps(&self) -> Vec<(&str, AppNet)> {
        let mut apps: Vec<(&str, AppNet)> = self
            .perm
            .iter()
            .map(|p| (p.app.as_str(), p.access))
            .collect();
        apps.sort_by(|a, b| a.0.cmp(b.0));
        apps.dedup_by(|a, b| a.0 == b.0);
        apps
    }
    /// Like `render`; `previous_perm_chains` = number of app permission
    /// chains the current table has (from the previous render), so that
    /// chains of apps no longer restricted are deleted.
    pub fn render_with(
        &self,
        existing: &std::collections::BTreeSet<String>,
        table_exists: bool,
        previous_perm_chains: usize,
    ) -> Result<String> {
        ensure!(
            self.mobile_interfaces
                .iter()
                .chain(&self.vpn_interfaces)
                .all(|s| valid_iface(s)),
            "invalid interface"
        );
        ensure!(
            self.perm
                .iter()
                .all(|s| valid_id(&s.app) && valid_cgroup(&s.path)),
            "invalid permission scope"
        );
        let perm_apps = self.perm_apps();
        ensure!(
            self.interfaces.iter().all(|s| valid_iface(s)),
            "invalid interface"
        );
        ensure!(
            self.scopes
                .iter()
                .all(|s| valid_id(&s.app) && valid_cgroup(&s.path)),
            "invalid scope"
        );
        ensure!(
            self.system_allow.iter().all(|s| valid_cgroup(s)),
            "invalid system scope"
        );
        let mut out = String::new();
        if !table_exists {
            out.push_str("add table inet fp6_netpolicy\n");
        }
        let mut counters = existing.clone();
        for scope in &self.scopes {
            for dir in ["tx", "rx"] {
                let name = counter_name(&scope.app, dir);
                if counters.insert(name.clone()) {
                    out += &format!("add counter inet fp6_netpolicy {name}\n");
                }
            }
        }
        // App network permission chains (regular chains, jumped to from the
        // base chains below for the app's cgroups).
        for (index, (_, access)) in perm_apps.iter().enumerate() {
            for (dir, iface, addr) in [("out", "oifname", "daddr"), ("in", "iifname", "saddr")] {
                let chain = perm_chain(index, dir);
                out += &format!("add chain inet fp6_netpolicy {chain}\n");
                out += &format!("flush chain inet fp6_netpolicy {chain}\n");
                let prefix = format!("add rule inet fp6_netpolicy {chain}");
                if !access.restricted() {
                    continue;
                }
                if !(access.wifi || access.mobile || access.vpn || access.local) {
                    out += &format!("{prefix} drop\n");
                    continue;
                }
                let verdict = |ok: bool| if ok { "return" } else { "drop" };
                let mobile = self
                    .mobile_interfaces
                    .iter()
                    .map(|s| s.as_str())
                    .chain(MOBILE_PATTERNS.iter().copied());
                for name in mobile {
                    out += &format!(
                        "{prefix} {iface} {} {}\n",
                        quote(name),
                        verdict(access.mobile)
                    );
                }
                let vpn = self
                    .vpn_interfaces
                    .iter()
                    .map(|s| s.as_str())
                    .chain(VPN_PATTERNS.iter().copied());
                for name in vpn {
                    out += &format!("{prefix} {iface} {} {}\n", quote(name), verdict(access.vpn));
                }
                out += &format!(
                    "{prefix} ip {addr} {{ {LAN4} }} {}\n",
                    verdict(access.local)
                );
                out += &format!(
                    "{prefix} ip6 {addr} {{ {LAN6} }} {}\n",
                    verdict(access.local)
                );
                if !access.wifi {
                    out += &format!("{prefix} drop\n");
                }
            }
        }
        for (chain, hook, iface, dir) in [
            ("out", "output", "oifname", "tx"),
            ("in", "input", "iifname", "rx"),
        ] {
            if table_exists {
                out += &format!("flush chain inet fp6_netpolicy {chain}\n");
            } else {
                out += &format!("add chain inet fp6_netpolicy {chain} {{ type filter hook {hook} priority -5; policy accept; }}\n");
            }
            out += &format!("add rule inet fp6_netpolicy {chain} {iface} \"lo\" return\n");
            // App network permission first, on every interface; an allowed
            // packet returns here and meets data saver below.
            let mut perm = self.perm.clone();
            perm.sort_by_key(|s| std::cmp::Reverse(s.path.split('/').count()));
            for scope in &perm {
                if let Some(index) = perm_apps.iter().position(|(a, _)| *a == scope.app) {
                    let target = perm_chain(index, chain);
                    out += &format!(
                        "add rule inet fp6_netpolicy {chain} {} jump {target}\n",
                        socket(&scope.path)
                    );
                }
            }
            for interface in &self.interfaces {
                let prefix = format!(
                    "add rule inet fp6_netpolicy {chain} {iface} {}",
                    quote(interface)
                );
                for path in &self.system_allow {
                    out += &format!("{prefix} {} return\n", socket(path));
                }
                // Deepest scopes first, so nested scopes are charged once.
                let mut scopes = self.scopes.clone();
                scopes.sort_by_key(|s| std::cmp::Reverse(s.path.split('/').count()));
                for scope in &scopes {
                    let matcher = socket(&scope.path);
                    if scope.allowed {
                        out += &format!(
                            "{prefix} {matcher} counter name {} return\n",
                            counter_name(&scope.app, dir)
                        );
                    } else {
                        // RX bytes have already traversed the modem even if dropped.
                        let count = if dir == "rx" {
                            format!("counter name {} ", counter_name(&scope.app, dir))
                        } else {
                            String::new()
                        };
                        out += &format!("{prefix} {matcher} {count}drop\n");
                    }
                }
                if self.restrict_unknown {
                    out += &format!(
                        "{prefix} {} drop\n",
                        socket(&format!("user.slice/user-{}.slice", self.uid))
                    );
                    out += &format!("{prefix} {} drop\n", socket("system.slice"));
                    if dir == "tx" {
                        out += &format!("{prefix} meta skuid {} drop\n", self.uid);
                    }
                }
            }
        }
        for index in perm_apps.len()..previous_perm_chains {
            for dir in ["out", "in"] {
                out += &format!(
                    "delete chain inet fp6_netpolicy {}\n",
                    perm_chain(index, dir)
                );
            }
        }
        Ok(out)
    }
}
pub fn read_counters(
    json: &str,
    apps: &[String],
) -> Result<(std::collections::BTreeSet<String>, BTreeMap<String, u64>)> {
    let value: serde_json::Value = serde_json::from_str(json)?;
    let mut names = std::collections::BTreeSet::new();
    let mut totals: BTreeMap<String, u64> = BTreeMap::new();
    for item in value["nftables"].as_array().into_iter().flatten() {
        if let Some(counter) = item.get("counter") {
            if counter["table"] != "fp6_netpolicy" || counter["family"] != "inet" {
                continue;
            }
            if let Some(name) = counter["name"].as_str() {
                names.insert(name.to_owned());
                for app in apps {
                    if name == counter_name(app, "tx") || name == counter_name(app, "rx") {
                        let total = totals.entry(app.clone()).or_default();
                        *total = total.saturating_add(counter["bytes"].as_u64().unwrap_or(0));
                    }
                }
            }
        }
    }
    Ok((names, totals))
}
