//! Post-boot health check: the decision part (pure, unit-tested).
//!
//! Always critical: display-manager active, a Lomiri process (greeter or
//! session) running and still running after the stability wait, no failed
//! critical unit, sshd active and listening.
//! SIM-dependent: with a SIM present the modem must be powered (ofono).
//! Baseline-relative: "modem online", "NetBird connected" and "NetBird
//! service active" only count when they passed on the last good boot, so a
//! phone in flight mode, without a SIM or logged out of NetBird is not rolled
//! back for it.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Facts {
    pub display_manager_active: bool,
    /// Lomiri processes found (names).
    pub shell_processes: Vec<String>,
    /// Still running after the stability wait.
    pub shell_stable: bool,
    pub failed_critical_units: Vec<String>,
    /// All failed units (for the report only).
    #[serde(default)]
    pub failed_units: Vec<String>,
    /// None: ofono not reachable / no modem.
    pub sim_present: Option<bool>,
    pub modem_powered: Option<bool>,
    pub modem_online: Option<bool>,
    pub sshd_active: bool,
    pub ssh_listening: bool,
    /// None: NetBird not configured on this system.
    pub netbird_active: Option<bool>,
    pub netbird_connected: Option<bool>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pass,
    Fail,
    Skip,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CheckResult {
    pub name: String,
    pub status: Status,
    pub critical: bool,
    pub detail: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub time: i64,
    pub passed: bool,
    pub checks: Vec<CheckResult>,
    #[serde(default)]
    pub slot: String,
    #[serde(default)]
    pub toplevel: String,
}

/// Which checks passed on the last good boot.
pub type Baseline = BTreeMap<String, bool>;

impl Report {
    pub fn baseline(&self) -> Baseline {
        self.checks.iter().filter(|c| c.status != Status::Skip).map(|c| (c.name.clone(), c.status == Status::Pass)).collect()
    }
    pub fn failures(&self) -> Vec<String> {
        self.checks.iter().filter(|c| c.critical && c.status == Status::Fail).map(|c| format!("{}: {}", c.name, c.detail)).collect()
    }
}

pub fn evaluate(f: &Facts, baseline: Option<&Baseline>, time: i64) -> Report {
    let mut checks = Vec::new();
    let mut add = |name: &str, ok: Option<bool>, critical: bool, detail: String| {
        checks.push(CheckResult {
            name: name.into(),
            status: match ok { Some(true) => Status::Pass, Some(false) => Status::Fail, None => Status::Skip },
            critical,
            detail,
        });
    };
    let was_ok = |name: &str| baseline.is_some_and(|b| b.get(name) == Some(&true));

    add("display-manager", Some(f.display_manager_active), true,
        if f.display_manager_active { "active".into() } else { "display-manager.service is not active".into() });
    let shell = !f.shell_processes.is_empty() && f.shell_stable;
    add("lomiri", Some(shell), true, if f.shell_processes.is_empty() {
        "no greeter or Lomiri process".into()
    } else if !f.shell_stable {
        format!("{} exited during the stability wait", f.shell_processes.join(", "))
    } else {
        f.shell_processes.join(", ")
    });
    add("critical-units", Some(f.failed_critical_units.is_empty()), true, if f.failed_critical_units.is_empty() {
        format!("none failed ({} failed units in total)", f.failed_units.len())
    } else {
        format!("failed: {}", f.failed_critical_units.join(", "))
    });
    let ssh = f.sshd_active && f.ssh_listening;
    add("ssh", Some(ssh), true, match (f.sshd_active, f.ssh_listening) {
        (true, true) => "sshd active, port 22 listening".into(),
        (false, _) => "sshd.service is not active".into(),
        (true, false) => "sshd is not listening on port 22".into(),
    });
    match f.sim_present {
        Some(true) => {
            add("modem", f.modem_powered.or(Some(false)), true,
                if f.modem_powered == Some(true) { "SIM present, modem powered".into() } else { "SIM present but the modem is not powered".into() });
            let crit = was_ok("modem-online");
            add("modem-online", f.modem_online.or(Some(false)), crit,
                format!("{}{}", if f.modem_online == Some(true) { "online" } else { "offline" },
                        if crit { "" } else { " (not required: not online on the last good boot)" }));
        }
        Some(false) => add("modem", None, false, "no SIM".into()),
        None => add("modem", None, false, "ofono not reachable or no modem".into()),
    }
    match f.netbird_active {
        None => add("netbird", None, false, "not configured".into()),
        Some(a) => {
            let crit = was_ok("netbird");
            add("netbird", Some(a), crit, if a { "netbird.service active".into() } else { "netbird.service is not active".into() });
            let crit = was_ok("netbird-connected");
            add("netbird-connected", f.netbird_connected, crit,
                match f.netbird_connected { Some(true) => "connected".into(), Some(false) => "not connected".into(), None => "unknown".into() });
        }
    }
    let passed = !checks.iter().any(|c| c.critical && c.status == Status::Fail);
    Report { time, passed, checks, slot: String::new(), toplevel: String::new() }
}

/// What the boot-time unit does with a report.
#[derive(Debug, PartialEq, Clone)]
pub enum Action {
    /// No update awaits confirmation on this boot: record only.
    Record,
    /// The pending update booted and is healthy: mark the slot successful.
    MarkSuccessful,
    /// Unhealthy, attempts left: re-arm the new slot and reboot.
    Retry { attempt: u32 },
    /// Unhealthy, no attempts left: make the previous slot active and reboot.
    RollBack,
    /// The bootloader (or the user) already returned to the previous slot.
    FellBack,
    /// Single-slot install: unhealthy, but there is no other slot to go to.
    ReportOnly,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    /// "ab" (slot switched) or "single" (boot partition of the running slot rewritten).
    pub mode: String,
    pub slot: String,
    pub toplevel: String,
    pub previous_slot: String,
    pub previous_toplevel: String,
    pub attempts: u32,
    pub max_attempts: u32,
    pub release: u64,
    pub version: String,
    pub time: i64,
    /// "install" or "rollback".
    #[serde(default)]
    pub kind: String,
}

pub fn decide(pending: Option<&Pending>, booted_slot: &str, booted_toplevel: &str, report: &Report) -> Action {
    let Some(p) = pending else { return Action::Record };
    if p.mode != "ab" {
        if booted_toplevel != p.toplevel {
            return Action::FellBack;
        }
        return if report.passed { Action::MarkSuccessful } else { Action::ReportOnly };
    }
    if booted_slot != p.slot || booted_toplevel != p.toplevel {
        return Action::FellBack;
    }
    if report.passed {
        Action::MarkSuccessful
    } else if p.attempts + 1 < p.max_attempts.max(1) {
        Action::Retry { attempt: p.attempts + 1 }
    } else {
        Action::RollBack
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn healthy() -> Facts {
        Facts {
            display_manager_active: true,
            shell_processes: vec!["lomiri".into()],
            shell_stable: true,
            failed_critical_units: vec![],
            failed_units: vec!["fp6-something.service".into()],
            sim_present: Some(true),
            modem_powered: Some(true),
            modem_online: Some(true),
            sshd_active: true,
            ssh_listening: true,
            netbird_active: Some(true),
            netbird_connected: Some(false),
        }
    }

    #[test]
    fn healthy_passes_and_noncritical_failures_are_ignored() {
        let r = evaluate(&healthy(), None, 1);
        assert!(r.passed, "{:?}", r.failures());
        assert_eq!(r.checks.iter().find(|c| c.name == "netbird-connected").unwrap().status, Status::Fail);
        assert!(!r.checks.iter().find(|c| c.name == "netbird-connected").unwrap().critical);
    }

    #[test]
    fn critical_failures() {
        for (f, name) in [
            (Facts { display_manager_active: false, ..healthy() }, "display-manager"),
            (Facts { shell_processes: vec![], ..healthy() }, "lomiri"),
            (Facts { shell_stable: false, ..healthy() }, "lomiri"),
            (Facts { failed_critical_units: vec!["NetworkManager.service".into()], ..healthy() }, "critical-units"),
            (Facts { sshd_active: false, ..healthy() }, "ssh"),
            (Facts { ssh_listening: false, ..healthy() }, "ssh"),
            (Facts { modem_powered: Some(false), ..healthy() }, "modem"),
            (Facts { modem_powered: None, ..healthy() }, "modem"),
        ] {
            let r = evaluate(&f, None, 1);
            assert!(!r.passed, "{name} should fail");
            assert!(r.failures()[0].starts_with(name), "{:?}", r.failures());
        }
    }

    #[test]
    fn modem_and_netbird_relative_to_baseline() {
        // no SIM: modem not considered
        assert!(evaluate(&Facts { sim_present: Some(false), modem_powered: None, modem_online: None, ..healthy() }, None, 1).passed);
        assert!(evaluate(&Facts { sim_present: None, ..healthy() }, None, 1).passed);
        // offline without a baseline: flight mode is fine
        let offline = Facts { modem_online: Some(false), ..healthy() };
        assert!(evaluate(&offline, None, 1).passed);
        // online on the last good boot: now required
        let good = evaluate(&Facts { netbird_connected: Some(true), ..healthy() }, None, 1);
        let b = good.baseline();
        assert_eq!(b.get("modem-online"), Some(&true));
        let r = evaluate(&offline, Some(&b), 2);
        assert!(!r.passed);
        assert!(r.failures()[0].starts_with("modem-online"));
        // NetBird connected on the last good boot
        let r = evaluate(&healthy(), Some(&b), 2);
        assert!(!r.passed && r.failures()[0].starts_with("netbird-connected"), "{:?}", r.failures());
        // NetBird not configured
        assert!(evaluate(&Facts { netbird_active: None, netbird_connected: None, ..healthy() }, Some(&b), 2).checks.iter().all(|c| c.name != "netbird-connected"));
    }

    fn pending(mode: &str) -> Pending {
        Pending { mode: mode.into(), slot: "_a".into(), toplevel: "/nix/store/new".into(), previous_slot: "_b".into(),
                  previous_toplevel: "/nix/store/old".into(), attempts: 0, max_attempts: 2, ..Default::default() }
    }

    #[test]
    fn decisions() {
        let ok = evaluate(&healthy(), None, 1);
        let bad = evaluate(&Facts { display_manager_active: false, ..healthy() }, None, 1);
        assert_eq!(decide(None, "_b", "/nix/store/old", &bad), Action::Record);
        let p = pending("ab");
        assert_eq!(decide(Some(&p), "_a", "/nix/store/new", &ok), Action::MarkSuccessful);
        assert_eq!(decide(Some(&p), "_a", "/nix/store/new", &bad), Action::Retry { attempt: 1 });
        let p1 = Pending { attempts: 1, ..p.clone() };
        assert_eq!(decide(Some(&p1), "_a", "/nix/store/new", &bad), Action::RollBack);
        let p0 = Pending { max_attempts: 0, ..p.clone() };
        assert_eq!(decide(Some(&p0), "_a", "/nix/store/new", &bad), Action::RollBack, "max 0 behaves as 1");
        assert_eq!(decide(Some(&p), "_b", "/nix/store/old", &ok), Action::FellBack);
        assert_eq!(decide(Some(&p), "_a", "/nix/store/old", &ok), Action::FellBack, "slot right, system wrong");
        let s = Pending { slot: "_b".into(), previous_slot: "_b".into(), ..pending("single") };
        assert_eq!(decide(Some(&s), "_b", "/nix/store/new", &ok), Action::MarkSuccessful);
        assert_eq!(decide(Some(&s), "_b", "/nix/store/new", &bad), Action::ReportOnly);
        assert_eq!(decide(Some(&s), "_b", "/nix/store/old", &ok), Action::FellBack);
    }
}
