// SPDX-License-Identifier: GPL-3.0-or-later
//! The broker's state and decisions, without D-Bus (bin/permd.rs is the
//! D-Bus front end). Everything that touches the system goes through the
//! paths in `Config`, so tests run on a scratch directory.

use crate::appid;
use crate::model::{Answer, Decision, Event, EventKind, NetAccess, Permission, Store};
use crate::policy::{self, Outcome, Subject, Usage};
use crate::render;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub path: String,
    #[serde(default)]
    pub complain: bool,
}

/// Root configuration (/etc via the NixOS module; never user-writable).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The phone user (only this user's session agent may answer prompts
    /// and change decisions).
    pub uid: u32,
    /// App ids that keep their access without prompts (Settings, phone,
    /// messaging, contacts, terminal, ...).
    #[serde(default)]
    pub system_apps: Vec<String>,
    /// User units (basenames) that may call Authorize/AuthorizePid on behalf
    /// of their callers (an NFC daemon, a contacts service).
    #[serde(default)]
    pub trusted_units: Vec<String>,
    #[serde(default = "default_agent_unit")]
    pub agent_unit: String,
    /// Confined apps: AppArmor profile name (= app id) -> source.
    #[serde(default)]
    pub apparmor: BTreeMap<String, Profile>,
    #[serde(default = "default_parser")]
    pub apparmor_parser: String,
    #[serde(default = "default_state_dir")]
    pub state_dir: PathBuf,
    #[serde(default = "default_run_dir")]
    pub run_dir: PathBuf,
    /// systemd user-manager drop-in root (root-owned, read by every user's
    /// manager).
    #[serde(default = "default_dropin_root")]
    pub dropin_root: PathBuf,
    /// "While using" grace after the app lost focus, seconds.
    #[serde(default = "default_grace")]
    pub grace_secs: u64,
    #[serde(default = "default_prompt_timeout")]
    pub prompt_timeout_secs: u64,
    /// Handing out descriptors (false: Request/Check still work, OpenDevice
    /// and OpenSocket are refused).
    #[serde(default = "yes")]
    pub devices: bool,
    /// Write /run/fp6-permissions/network.json for netpolicy.
    #[serde(default = "yes")]
    pub network: bool,
    /// Generate AppArmor includes / systemd drop-ins.
    #[serde(default = "yes")]
    pub enforce: bool,
    /// Camera / microphone / location permissions that are enforced (the
    /// module's media option): shown in Settings, rendered into AppArmor
    /// includes. Requests for the others still work but change nothing.
    #[serde(default)]
    pub media: Vec<Permission>,
    /// Decisions an app starts with until the user decides otherwise
    /// (camera app: camera + microphone allowed, ...). Reset goes back here.
    #[serde(default)]
    pub defaults: BTreeMap<String, BTreeMap<Permission, Decision>>,
}

fn default_agent_unit() -> String { "fp6-permission-agent.service".into() }
fn default_parser() -> String { "apparmor_parser".into() }
fn default_state_dir() -> PathBuf { "/var/lib/fp6-permissions".into() }
fn default_run_dir() -> PathBuf { "/run/fp6-permissions".into() }
fn default_dropin_root() -> PathBuf { "/run/systemd/user".into() }
fn default_grace() -> u64 { 30 }
fn default_prompt_timeout() -> u64 { 60 }
fn yes() -> bool { true }

impl Config {
    pub fn validate(&self) -> Result<(), String> {
        if self.uid < 1000 {
            return Err("uid must be a normal user".into());
        }
        for a in self.system_apps.iter().chain(self.apparmor.keys()).chain(self.defaults.keys()) {
            if !appid::valid_app_id(a) {
                return Err(format!("invalid app id {a:?}"));
            }
        }
        for u in self.trusted_units.iter().chain(std::iter::once(&self.agent_unit)) {
            if !appid::valid_app_id(u) || !(u.ends_with(".service") || u.ends_with(".scope")) {
                return Err(format!("invalid unit {u:?}"));
            }
        }
        if !(5..=600).contains(&self.prompt_timeout_secs) || self.grace_secs > 3600 {
            return Err("timeouts out of range".into());
        }
        Ok(())
    }
    pub fn is_system_app(&self, id: &str) -> bool {
        self.system_apps.iter().any(|a| a == id)
    }
    pub fn preset(&self, id: &str, perm: Permission) -> Option<Decision> {
        self.defaults.get(id).and_then(|m| m.get(&perm)).copied()
    }
    /// Is this permission enforced here (media ones only with the option)?
    pub fn enforced(&self, perm: Permission) -> bool {
        !perm.is_media() || self.media.contains(&perm)
    }
}

/// Caller credentials (from D-Bus GetConnectionCredentials).
#[derive(Clone, Debug, Default)]
pub struct Creds {
    pub uid: Option<u32>,
    pub pid: Option<u32>,
    pub label: Option<Vec<u8>>,
}

/// A descriptor handed out (for revocation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    pub app: String,
    pub perm: Permission,
    pub node: String,
    pub usb_sysfs: PathBuf,
    pub since: u64,
}

#[derive(Default)]
pub struct Focus {
    pub known: bool,
    pub current: Option<String>,
    pub last: BTreeMap<String, u64>,
}

pub struct Broker {
    pub cfg: Config,
    pub store: Store,
    pub focus: Focus,
    pub grants: Vec<Grant>,
    pub agent: Option<String>,
    /// "Only this time" answers: (app, permission) -> since. In memory only.
    pub once: BTreeMap<(String, Permission), u64>,
    /// Root of /proc (tests: a fake tree).
    pub proc_root: PathBuf,
    last_written: BTreeMap<PathBuf, String>,
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Atomic write (tmp + fsync + rename). Returns whether the content changed.
pub fn write_atomic(path: &Path, content: &str, mode: u32) -> std::io::Result<bool> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    if fs::read_to_string(path).is_ok_and(|old| old == content) {
        return Ok(false);
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp~");
    {
        let mut f = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(mode).open(&tmp)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
    }
    fs::set_permissions(&tmp, fs::Permissions::from_mode(mode))?;
    fs::rename(&tmp, path)?;
    Ok(true)
}

impl Broker {
    pub fn new(cfg: Config) -> Result<Broker, String> {
        cfg.validate()?;
        let path = cfg.state_dir.join("store.json");
        let store = match fs::read_to_string(&path) {
            Ok(t) => serde_json::from_str(&t).map_err(|e| format!("{}: {e}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Store::new(),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        Ok(Broker {
            cfg,
            store,
            focus: Focus::default(),
            grants: vec![],
            agent: None,
            once: BTreeMap::new(),
            proc_root: "/proc".into(),
            last_written: BTreeMap::new(),
        })
    }

    pub fn save(&self) -> Result<(), String> {
        let text = serde_json::to_string(&self.store).map_err(|e| e.to_string())?;
        write_atomic(&self.cfg.state_dir.join("store.json"), &text, 0o600)
            .map(|_| ())
            .map_err(|e| format!("saving store: {e}"))
    }

    fn proc_cgroup(&self, pid: u32) -> Option<String> {
        let text = fs::read_to_string(self.proc_root.join(format!("{pid}/cgroup"))).ok()?;
        appid::cgroup_of(&text)
    }

    /// Who is calling.
    pub fn subject(&self, c: &Creds) -> Subject {
        if let Some(profile) = c.label.as_deref().and_then(appid::apparmor_profile) {
            if appid::valid_app_id(&profile) {
                return Subject::App { id: profile, confined: true };
            }
            return Subject::System { what: format!("profile {profile}") };
        }
        let uid = c.uid.unwrap_or(u32::MAX);
        if uid != self.cfg.uid {
            return Subject::System { what: if uid == 0 { "root".into() } else { format!("uid {uid}") } };
        }
        let Some(cg) = c.pid.and_then(|p| self.proc_cgroup(p)) else {
            // Without a cgroup we cannot tell it apart from an app: treat it
            // as an unknown app (prompted, never silently trusted).
            return Subject::App { id: "unknown".into(), confined: false };
        };
        if let Some((id, _)) = appid::app_from_cgroup(&cg) {
            return Subject::App { id, confined: false };
        }
        if appid::is_login_session(&cg) {
            return Subject::System { what: "session".into() };
        }
        Subject::System { what: appid::leaf_unit(&cg).unwrap_or("user").to_owned() }
    }

    /// The caller is this user's permission agent.
    pub fn is_agent_unit(&self, c: &Creds) -> bool {
        c.uid == Some(self.cfg.uid)
            && c.label.as_deref().and_then(appid::apparmor_profile).is_none()
            && c.pid
                .and_then(|p| self.proc_cgroup(p))
                .is_some_and(|cg| appid::leaf_unit(&cg) == Some(self.cfg.agent_unit.as_str()))
    }

    /// May call Authorize on behalf of others: root, system users, or a
    /// trusted user unit of the phone user.
    pub fn is_trusted_daemon(&self, c: &Creds) -> bool {
        if c.label.as_deref().and_then(appid::apparmor_profile).is_some() {
            return false;
        }
        match c.uid {
            Some(0) => true,
            Some(u) if u < 1000 => true,
            Some(u) if u == self.cfg.uid => c
                .pid
                .and_then(|p| self.proc_cgroup(p))
                .and_then(|cg| appid::leaf_unit(&cg).map(str::to_owned))
                .is_some_and(|unit| self.cfg.trusted_units.contains(&unit)),
            _ => false,
        }
    }

    pub fn usage(&self, app: &str, now: u64) -> Usage {
        if !self.focus.known {
            return Usage::UNKNOWN;
        }
        let focused = self.focus.current.as_deref() == Some(app);
        let since = if focused {
            0
        } else {
            self.focus.last.get(app).map_or(u64::MAX, |t| now.saturating_sub(*t))
        };
        Usage { focused: Some(focused), since_focus: since }
    }

    pub fn set_focus(&mut self, app: Option<String>, now: u64) {
        self.focus.known = true;
        if let Some(prev) = self.focus.current.take() {
            self.focus.last.insert(prev, now);
        }
        if let Some(a) = &app {
            self.focus.last.insert(a.clone(), now);
        }
        self.focus.current = app.filter(|a| !a.is_empty());
    }

    /// The decision in force: the user's own, else the configured preset,
    /// else Ask.
    pub fn decision(&self, app: &str, perm: Permission) -> Decision {
        self.store
            .explicit_decision(app, perm)
            .or_else(|| self.cfg.preset(app, perm))
            .unwrap_or_default()
    }

    /// A subject named by a trusted daemon that only knows the app id
    /// (GeoClue's agent).
    pub fn app_subject(&self, id: &str) -> Subject {
        Subject::App { id: id.to_owned(), confined: self.cfg.apparmor.contains_key(id) }
    }

    fn once_valid(&self, app: &str, perm: Permission, now: u64) -> bool {
        self.once.contains_key(&(app.to_owned(), perm)) && policy::once_valid(self.usage(app, now), self.cfg.grace_secs)
    }

    pub fn evaluate(&self, s: &Subject, perm: Permission, now: u64) -> Outcome {
        match s {
            Subject::System { .. } => Outcome::Granted,
            Subject::App { id, .. } if self.once_valid(id, perm, now) && !self.cfg.is_system_app(id) => {
                // an explicit Deny set in Settings meanwhile wins (set_decision
                // drops the grant, this is belt and braces)
                if self.decision(id, perm) == Decision::Deny { Outcome::Denied } else { Outcome::Granted }
            }
            Subject::App { id, .. } => policy::evaluate(
                self.decision(id, perm),
                self.cfg.is_system_app(id),
                self.usage(id, now),
                self.cfg.grace_secs,
            ),
        }
    }

    /// Drop "only this time" grants of apps that went to the background.
    /// Returns whether any expired.
    pub fn expire_once(&mut self, now: u64) -> bool {
        let before = self.once.len();
        let expired: Vec<(String, Permission)> = self
            .once
            .keys()
            .filter(|(app, _)| !policy::once_valid(self.usage(app, now), self.cfg.grace_secs))
            .cloned()
            .collect();
        for k in &expired {
            self.once.remove(k);
            self.store.log(Event { time: now, app: k.0.clone(), perm: k.1.id().into(), kind: EventKind::Revoked, detail: "only this time: ended".into() });
        }
        self.once.len() != before
    }

    pub fn record(&mut self, s: &Subject, perm: Permission, granted: bool, detail: &str, now: u64) {
        let detail = match s {
            Subject::System { what } if detail.is_empty() => what.clone(),
            Subject::System { what } => format!("{detail} · {what}"),
            _ => detail.to_owned(),
        };
        let key = match s {
            Subject::App { id, .. } => id.clone(),
            // System use is history only (no per-app entry to toggle).
            Subject::System { .. } => {
                self.store.log(Event {
                    time: now,
                    app: "system".into(),
                    perm: perm.id().into(),
                    kind: if granted { EventKind::Used } else { EventKind::Denied },
                    detail,
                });
                return;
            }
        };
        self.store.note(&key, perm, granted, &detail, now);
    }

    pub fn log_prompt(&mut self, app: &str, perm: Permission, answer: Answer, now: u64) {
        let what = match answer {
            Answer::Allow => "allowed",
            Answer::WhileUsing => "allowed while using",
            Answer::Once => "allowed this time",
            Answer::Deny => "denied",
            Answer::Dismissed => "dismissed",
        };
        self.store.log(Event { time: now, app: app.into(), perm: perm.id().into(), kind: EventKind::Prompted, detail: what.into() });
        if let Some(d) = answer.remembered() {
            self.store.set_decision(app, perm, d, now);
        }
        if answer == Answer::Once {
            self.once.insert((app.to_owned(), perm), now);
        }
    }

    /// Settings change. Returns grants that must be revoked now.
    pub fn set_decision(&mut self, app: &str, perm: Permission, d: Decision, now: u64) -> Vec<Grant> {
        self.store.set_decision(app, perm, d, now);
        self.once.remove(&(app.to_owned(), perm));
        self.store.log(Event { time: now, app: app.into(), perm: perm.id().into(), kind: EventKind::Changed, detail: d.id().into() });
        self.take_revocable(now)
    }

    pub fn set_network(&mut self, app: &str, class: &str, on: bool, now: u64) -> Result<(), String> {
        if self.cfg.is_system_app(app) && !on {
            return Err(format!("{app} is a system component; its network access cannot be removed"));
        }
        if !self.store.app(app, now).network.set(class, on) {
            return Err(format!("unknown network class {class:?}"));
        }
        self.store.log(Event {
            time: now,
            app: app.into(),
            perm: "network".into(),
            kind: EventKind::Changed,
            detail: format!("{class} {}", if on { "allowed" } else { "blocked" }),
        });
        Ok(())
    }

    pub fn reset_app(&mut self, app: &str, now: u64) -> Vec<Grant> {
        self.store.reset(app);
        self.once.retain(|(a, _), _| a != app);
        self.store.log(Event { time: now, app: app.into(), perm: "all".into(), kind: EventKind::Changed, detail: "reset".into() });
        self.take_revocable(now)
    }

    /// Remove and return the grants whose decision no longer covers them
    /// (Settings change, or "while using" app in the background).
    pub fn take_revocable(&mut self, now: u64) -> Vec<Grant> {
        let mut out = vec![];
        let mut keep = vec![];
        for g in std::mem::take(&mut self.grants) {
            let due = !self.cfg.is_system_app(&g.app)
                && policy::revoke_due(self.decision(&g.app, g.perm), self.usage(&g.app, now), self.cfg.grace_secs);
            if due {
                out.push(g);
            } else {
                keep.push(g);
            }
        }
        self.grants = keep;
        for g in &out {
            self.store.log(Event { time: now, app: g.app.clone(), perm: g.perm.id().into(), kind: EventKind::Revoked, detail: g.node.clone() });
        }
        out
    }

    fn write(&mut self, path: PathBuf, content: String, mode: u32) -> Result<bool, String> {
        if self.last_written.get(&path) == Some(&content) && path.exists() {
            return Ok(false);
        }
        // A missing file and an empty one mean the same to AppArmor's
        // `include if exists` and to systemd: not a change.
        let was_missing = !path.exists();
        let changed = write_atomic(&path, &content, mode).map_err(|e| format!("{}: {e}", path.display()))?;
        let changed = changed && !(was_missing && content.is_empty());
        self.last_written.insert(path, content);
        Ok(changed)
    }

    /// Regenerate every enforcement file. Returns (AppArmor profiles whose
    /// include changed, whether any systemd drop-in changed).
    pub fn render_all(&mut self) -> Result<(Vec<String>, bool), String> {
        let mut changed_profiles = vec![];
        let mut dropins_changed = false;
        if self.cfg.network {
            let rules: BTreeMap<String, NetAccess> = self
                .store
                .network_rules()
                .into_iter()
                .filter(|(a, _)| !self.cfg.is_system_app(a))
                .collect();
            let text = render::network_json(self.cfg.uid, &rules);
            self.write(self.cfg.run_dir.join("network.json"), text, 0o644)?;
        }
        if !self.cfg.enforce {
            return Ok((changed_profiles, dropins_changed));
        }
        // Effective decisions (presets included) of the enforced permissions.
        let decisions = |b: &Broker, app: &str| -> render::Decisions {
            Permission::ALL
                .into_iter()
                .filter(|p| b.cfg.enforced(*p))
                .map(|p| (p, b.decision(app, p)))
                .filter(|(_, d)| *d != Decision::Ask)
                .collect()
        };
        let profiles: Vec<String> = self.cfg.apparmor.keys().cloned().collect();
        for profile in profiles {
            let d = decisions(self, &profile);
            let net = self.store.apps.get(&profile).map(|a| a.network).unwrap_or_default();
            let text = if self.cfg.is_system_app(&profile) { String::new() } else { render::apparmor_include(&profile, &d, &net) };
            if self.write(self.cfg.state_dir.join("apparmor").join(&profile), text, 0o644)? {
                changed_profiles.push(profile);
            }
        }
        // Drop-ins: write wanted ones, remove stale ones of ours.
        let mut wanted = BTreeSet::new();
        let apps: Vec<String> = self.store.apps.keys().cloned().collect();
        for app in apps {
            if self.cfg.is_system_app(&app) || self.cfg.apparmor.contains_key(&app) {
                continue;
            }
            let (Some(dir), Some(text)) = (render::dropin_dir(&app), render::systemd_dropin(&decisions(self, &app))) else { continue };
            let path = self.cfg.dropin_root.join(&dir).join("60-fp6-permissions.conf");
            dropins_changed |= self.write(path, text, 0o644)?;
            wanted.insert(dir);
        }
        if let Ok(entries) = fs::read_dir(&self.cfg.dropin_root) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                let ours = e.path().join("60-fp6-permissions.conf");
                if name.starts_with("lomiri-app-launch--") && !wanted.contains(&name) && ours.exists() {
                    let _ = fs::remove_file(&ours);
                    let _ = fs::remove_dir(e.path());
                    self.last_written.remove(&ours);
                    dropins_changed = true;
                }
            }
        }
        Ok((changed_profiles, dropins_changed))
    }

    /// Reload changed AppArmor profiles (they include the generated file).
    pub fn reload_profiles(&self, names: &[String]) -> Vec<String> {
        let mut errors = vec![];
        for n in names {
            let Some(p) = self.cfg.apparmor.get(n) else { continue };
            let mut cmd = std::process::Command::new(&self.cfg.apparmor_parser);
            cmd.arg("--replace").arg("--quiet");
            if p.complain {
                cmd.arg("--complain");
            }
            match cmd.arg(&p.path).output() {
                Ok(o) if o.status.success() => {}
                Ok(o) => errors.push(format!("{n}: {}", String::from_utf8_lossy(&o.stderr).trim())),
                Err(e) => errors.push(format!("{n}: {e}")),
            }
        }
        errors
    }

    /// JSON for Settings (through the session agent).
    pub fn state_json(&self, now: u64) -> String {
        #[derive(Serialize)]
        struct PermOut<'a> {
            decision: &'a str,
            /// The decision comes from the configuration, not the user.
            preset: bool,
            /// An "only this time" grant is active.
            once: bool,
            changed: u64,
            last_used: u64,
            last_denied: u64,
            uses: u64,
            detail: &'a str,
        }
        #[derive(Serialize)]
        struct AppOut<'a> {
            system: bool,
            confined: bool,
            perms: BTreeMap<&'a str, PermOut<'a>>,
            network: NetAccess,
            last_seen: u64,
            in_use: bool,
        }
        let mut apps = BTreeMap::new();
        let empty = crate::model::AppEntry::default();
        let none = crate::model::PermEntry::default();
        let ids: BTreeSet<&String> = self.store.apps.keys().chain(self.cfg.defaults.keys()).collect();
        for id in ids {
            let a = self.store.apps.get(id).unwrap_or(&empty);
            let mut wanted: BTreeSet<Permission> = a.perms.keys().copied().collect();
            if let Some(d) = self.cfg.defaults.get(id) {
                wanted.extend(d.keys().copied());
            }
            let perms = wanted
                .into_iter()
                .filter(|p| self.cfg.enforced(*p) || a.perms.contains_key(p))
                .map(|p| {
                    let e = a.perms.get(&p).unwrap_or(&none);
                    (p.id(), PermOut {
                        decision: self.decision(id, p).id(),
                        preset: !e.is_explicit() && self.cfg.preset(id, p).is_some(),
                        once: self.once_valid(id, p, now),
                        changed: e.changed,
                        last_used: e.last_used,
                        last_denied: e.last_denied,
                        uses: e.uses,
                        detail: &e.last_detail,
                    })
                })
                .collect();
            apps.insert(id.as_str(), AppOut {
                system: self.cfg.is_system_app(id),
                confined: self.cfg.apparmor.contains_key(id),
                perms,
                network: a.network,
                last_seen: a.last_seen,
                in_use: self.grants.iter().any(|g| &g.app == id),
            });
        }
        serde_json::json!({
            "now": now,
            "apps": apps,
            "system_apps": self.cfg.system_apps,
            "confined": self.cfg.apparmor.keys().collect::<Vec<_>>(),
            "history": self.store.history.iter().rev().take(200).collect::<Vec<_>>(),
            "grants": self.grants.iter().map(|g| serde_json::json!({"app": g.app, "perm": g.perm.id(), "node": g.node, "since": g.since})).collect::<Vec<_>>(),
            "agent": self.agent.is_some(),
            "media": self.cfg.media.iter().map(|p| p.id()).collect::<Vec<_>>(),
            "focus_known": self.focus.known,
        })
        .to_string()
    }
}
