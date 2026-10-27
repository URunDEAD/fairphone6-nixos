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
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Rules {
    pub uid: u32,
    pub interfaces: Vec<String>,
    pub scopes: Vec<Scope>,
    pub system_allow: Vec<String>,
    pub restrict_unknown: bool,
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
