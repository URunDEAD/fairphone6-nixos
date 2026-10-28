//! Resource permissions: what the agent may touch, how long a grant lasts,
//! and when an action needs the user's explicit confirmation.
//!
//! Model (README "Permission model"):
//!  * Every tool declares the resources a call needs, computed from its
//!    arguments (e.g. files_read {path} -> files.read:<dir>). Before the
//!    call runs, each resource must be granted; otherwise the user sees a
//!    system dialog: Allow once / Allow for this chat / Always allow / Deny.
//!  * The model can never grant anything: grants only come from the user's
//!    answer to a dialog (or from Settings). Tool results are data.
//!  * Files are scoped to directories: a grant for ~/Documents covers files
//!    below it (after resolving "..", symlinks), nothing else. A few paths
//!    are never grantable (the assistant's own data, keys, ~/.ssh, ...).
//!  * Side-effecting actions (send, delete, call, post, write, toggle)
//!    additionally need a per-action confirmation showing the exact content
//!    and recipient, by default. Settings can relax this per tool, except
//!    that once the chat has read untrusted content (messages, e-mail, web,
//!    files, notifications, OCR) the confirmation is always shown again
//!    for the rest of that turn ("tainted" turn).
//!  * Before even asking, the assistant itself must hold the corresponding
//!    system permission in the per-app permissions broker (see `system`).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

/// Static description of a resource class.
pub struct ResourceInfo {
    pub class: &'static str,
    pub title: &'static str,
    pub explain: &'static str,
    /// The broker permission the assistant app needs first (None: no
    /// system-level permission exists for it).
    pub system: Option<&'static str>,
    /// "Always allow" offered? (false for the most sensitive classes)
    pub allow_always: bool,
}

pub const RESOURCES: &[ResourceInfo] = &[
    ResourceInfo { class: "messages.read", title: "Messages: read", explain: "Read your text messages and conversations", system: Some("messages"), allow_always: true },
    ResourceInfo { class: "messages.send", title: "Messages: send", explain: "Send text messages (each one is shown to you first)", system: Some("messages"), allow_always: true },
    ResourceInfo { class: "calls.read", title: "Call history", explain: "See who called and whom you called", system: Some("messages"), allow_always: true },
    ResourceInfo { class: "calls.place", title: "Phone calls", explain: "Start a phone call (you confirm each call)", system: None, allow_always: false },
    ResourceInfo { class: "contacts.read", title: "Contacts", explain: "Look up names and numbers in your contacts", system: Some("contacts"), allow_always: true },
    ResourceInfo { class: "email.read", title: "E-mail: read", explain: "Read messages in this e-mail account", system: None, allow_always: true },
    ResourceInfo { class: "email.send", title: "E-mail: send", explain: "Send e-mail from this account (each one is shown to you first)", system: None, allow_always: true },
    ResourceInfo { class: "files.read", title: "Files: read", explain: "List and read files in this folder", system: Some("files"), allow_always: true },
    ResourceInfo { class: "files.write", title: "Files: write", explain: "Create and change files in this folder", system: Some("files"), allow_always: true },
    ResourceInfo { class: "camera", title: "Camera", explain: "Open the camera so you can take a photo for the assistant", system: None, allow_always: true },
    ResourceInfo { class: "screenshot", title: "Screenshots", explain: "Take or read screenshots of your screen", system: None, allow_always: true },
    ResourceInfo { class: "calendar.read", title: "Calendar: read", explain: "See your calendar events", system: Some("calendar"), allow_always: true },
    ResourceInfo { class: "calendar.write", title: "Calendar: change", explain: "Add calendar events (you confirm each one)", system: Some("calendar"), allow_always: true },
    ResourceInfo { class: "alarms", title: "Alarms and timers", explain: "Set alarms and timers", system: None, allow_always: true },
    ResourceInfo { class: "settings", title: "Phone settings", explain: "Switch Wi-Fi, Bluetooth, flashlight, Do Not Disturb, volume", system: None, allow_always: true },
    ResourceInfo { class: "notifications.read", title: "Notifications", explain: "Read your recent notifications", system: None, allow_always: true },
    ResourceInfo { class: "location", title: "Location", explain: "Know where the phone is", system: None, allow_always: false },
    ResourceInfo { class: "web", title: "Internet", explain: "Fetch web pages and search the web", system: None, allow_always: true },
    ResourceInfo { class: "apps", title: "Open apps and links", explain: "Start apps and open links", system: None, allow_always: true },
    ResourceInfo { class: "ocr", title: "Text recognition", explain: "Read text in images on the phone", system: None, allow_always: true },
    ResourceInfo { class: "share", title: "Share", explain: "Open the share sheet or send to your KDE Connect devices", system: None, allow_always: true },
    ResourceInfo { class: "clipboard", title: "Clipboard", explain: "Read and set the clipboard", system: None, allow_always: true },
];

pub fn info(class: &str) -> Option<&'static ResourceInfo> {
    RESOURCES.iter().find(|r| r.class == class)
}

/// A concrete resource a tool call needs: a class plus an optional scope
/// (a directory for files, an account id for e-mail).
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Resource {
    pub class: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

impl Resource {
    pub fn new(class: &str) -> Self {
        Resource { class: class.into(), scope: None }
    }
    pub fn scoped(class: &str, scope: impl Into<String>) -> Self {
        Resource { class: class.into(), scope: Some(scope.into()) }
    }
    pub fn key(&self) -> String {
        match &self.scope {
            Some(s) => format!("{}:{}", self.class, s),
            None => self.class.clone(),
        }
    }
    pub fn title(&self) -> String {
        let base = info(&self.class).map(|i| i.title).unwrap_or(self.class.as_str()).to_string();
        match &self.scope {
            Some(s) if self.class.starts_with("files.") => format!("{base} {}", display_path(s)),
            Some(s) => format!("{base} ({s})"),
            None => base,
        }
    }
    /// Does a grant for `self` cover the request `need`?
    pub fn covers(&self, need: &Resource) -> bool {
        let class_ok = self.class == need.class || (self.class == "files.write" && need.class == "files.read");
        if !class_ok {
            return false;
        }
        match (&self.scope, &need.scope) {
            (None, None) => true,
            (Some(g), Some(n)) if self.class.starts_with("files.") => path_within(Path::new(n), Path::new(g)),
            (Some(g), Some(n)) => g == n,
            _ => false,
        }
    }
}

/// ~/Documents for display.
pub fn display_path(p: &str) -> String {
    if let Ok(home) = std::env::var("HOME") {
        if let Some(rest) = p.strip_prefix(&home) {
            return format!("~{rest}");
        }
    }
    p.to_string()
}

/// Lexically normalise (resolve "." and "..") without touching the disk.
pub fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Resolve a user/model supplied path: "~" expansion, relative to $HOME,
/// symlinks resolved for the longest existing prefix, then normalised.
pub fn resolve_path(input: &str) -> PathBuf {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()));
    let raw = if input == "~" {
        home.clone()
    } else if let Some(rest) = input.strip_prefix("~/") {
        home.join(rest)
    } else if input.starts_with('/') {
        PathBuf::from(input)
    } else {
        home.join(input)
    };
    let raw = normalize(&raw);
    // Canonicalise the existing part so a symlink cannot leave the scope.
    let mut existing = raw.clone();
    let mut tail = Vec::new();
    while !existing.exists() {
        match (existing.file_name().map(|s| s.to_os_string()), existing.parent()) {
            (Some(name), Some(parent)) => {
                tail.push(name);
                existing = parent.to_path_buf();
            }
            _ => break,
        }
    }
    let mut out = std::fs::canonicalize(&existing).unwrap_or(existing);
    for t in tail.into_iter().rev() {
        out.push(t);
    }
    normalize(&out)
}

pub fn path_within(p: &Path, dir: &Path) -> bool {
    let p = normalize(p);
    let d = normalize(dir);
    p == d || p.starts_with(&d)
}

/// Paths the agent can never be granted, whatever the user picks.
pub fn forbidden_path(p: &Path) -> Option<&'static str> {
    forbidden_path_in(p, &PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/nonexistent".into())))
}

pub fn forbidden_path_in(p: &Path, home: &Path) -> Option<&'static str> {
    let deny: &[(&str, &str)] = &[
        (".ssh", "SSH keys"),
        (".gnupg", "encryption keys"),
        (".local/share/keyrings", "the keyring"),
        (".local/share/fp6-assistant", "the assistant's own data"),
        (".config/fp6-assistant", "the assistant's settings"),
        (".pki", "certificates"),
    ];
    for (rel, why) in deny {
        if path_within(p, &home.join(rel)) {
            return Some(why);
        }
    }
    if !path_within(p, home) && !path_within(p, Path::new("/media")) && !path_within(p, Path::new("/run/media")) {
        return Some("folders outside your home folder and USB drives");
    }
    None
}

/// The user's answer to a permission dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    Once,
    Chat,
    Always,
    Deny,
}

impl Answer {
    pub fn parse(s: &str) -> Answer {
        match s {
            "once" => Answer::Once,
            "chat" => Answer::Chat,
            "always" => Answer::Always,
            // Anything else (dismissed, timeout, unknown) is a denial.
            _ => Answer::Deny,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Grant {
    pub resource: Resource,
    /// Unix seconds.
    pub granted_at: u64,
}

/// Persisted "Always allow" grants (~/.config/fp6-assistant/grants.json).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AlwaysGrants {
    pub grants: Vec<Grant>,
}

impl AlwaysGrants {
    pub fn covers(&self, need: &Resource) -> bool {
        self.grants.iter().any(|g| g.resource.covers(need))
    }
    pub fn add(&mut self, r: Resource, now: u64) {
        if !self.covers(&r) {
            self.grants.retain(|g| !r.covers(&g.resource));
            self.grants.push(Grant { resource: r, granted_at: now });
        }
    }
    pub fn revoke(&mut self, key: &str) -> bool {
        let before = self.grants.len();
        self.grants.retain(|g| g.resource.key() != key);
        before != self.grants.len()
    }
}

/// Grants that live as long as one chat (kept with the chat on disk) and
/// denials remembered for the chat so the model cannot nag.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ChatGrants {
    pub allowed: Vec<Resource>,
    pub denied: Vec<Resource>,
}

impl ChatGrants {
    pub fn covers(&self, need: &Resource) -> bool {
        self.allowed.iter().any(|g| g.covers(need))
    }
    pub fn denied(&self, need: &Resource) -> bool {
        self.denied.iter().any(|d| d.covers(need) || d == need)
    }
}

/// Per-tool confirmation policy (Settings > Assistant > Tools).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmPolicy {
    /// Show every action first (the default for side effects).
    Always,
    /// Run without asking (only while the turn is not tainted).
    Relaxed,
    /// The tool is switched off.
    Disabled,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ToolPolicies(pub BTreeMap<String, ConfirmPolicy>);

/// Does this action need a confirmation dialog?
pub fn needs_confirmation(side_effect: bool, policy: Option<ConfirmPolicy>, tainted: bool) -> bool {
    if !side_effect {
        return false;
    }
    match policy {
        Some(ConfirmPolicy::Relaxed) => tainted,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_scopes() {
        let g = Resource::scoped("files.read", "/home/u/Documents");
        assert!(g.covers(&Resource::scoped("files.read", "/home/u/Documents/a/b.txt")));
        assert!(g.covers(&Resource::scoped("files.read", "/home/u/Documents")));
        assert!(!g.covers(&Resource::scoped("files.read", "/home/u/Documents2/x")));
        assert!(!g.covers(&Resource::scoped("files.read", "/home/u/Documents/../.ssh/id")));
        assert!(!g.covers(&Resource::scoped("files.write", "/home/u/Documents/x")));
        let w = Resource::scoped("files.write", "/home/u/Documents");
        assert!(w.covers(&Resource::scoped("files.read", "/home/u/Documents/x")));
    }

    #[test]
    fn symlink_cannot_escape_scope() {
        let tmp = tempfile::tempdir().unwrap();
        let docs = tmp.path().join("Documents");
        let secret = tmp.path().join("secret");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::create_dir_all(&secret).unwrap();
        std::fs::write(secret.join("k"), "x").unwrap();
        std::os::unix::fs::symlink(&secret, docs.join("link")).unwrap();
        let resolved = resolve_path(docs.join("link/k").to_str().unwrap());
        let grant = Resource::scoped("files.read", resolve_path(docs.to_str().unwrap()).to_string_lossy());
        assert!(!grant.covers(&Resource::scoped("files.read", resolved.to_string_lossy())));
    }

    #[test]
    fn forbidden_paths() {
        let h = Path::new("/home/u");
        assert!(forbidden_path_in(Path::new("/home/u/.ssh/id_ed25519"), h).is_some());
        assert!(forbidden_path_in(Path::new("/home/u/.local/share/fp6-assistant/chats"), h).is_some());
        assert!(forbidden_path_in(Path::new("/etc/shadow"), h).is_some());
        assert!(forbidden_path_in(Path::new("/home/u/Documents/a.txt"), h).is_none());
        assert!(forbidden_path_in(Path::new("/media/usb/a.txt"), h).is_none());
    }

    #[test]
    fn answers_default_to_deny() {
        assert_eq!(Answer::parse("always"), Answer::Always);
        assert_eq!(Answer::parse(""), Answer::Deny);
        assert_eq!(Answer::parse("yes please"), Answer::Deny);
    }

    #[test]
    fn confirmation_policy() {
        assert!(!needs_confirmation(false, None, true));
        assert!(needs_confirmation(true, None, false));
        assert!(needs_confirmation(true, Some(ConfirmPolicy::Always), false));
        assert!(!needs_confirmation(true, Some(ConfirmPolicy::Relaxed), false));
        // Untrusted content in the turn brings the dialog back.
        assert!(needs_confirmation(true, Some(ConfirmPolicy::Relaxed), true));
    }

    #[test]
    fn always_grants_dedupe_and_revoke() {
        let mut a = AlwaysGrants::default();
        a.add(Resource::scoped("files.read", "/home/u/Documents/sub"), 1);
        a.add(Resource::scoped("files.read", "/home/u/Documents"), 2);
        assert_eq!(a.grants.len(), 1);
        assert!(a.covers(&Resource::scoped("files.read", "/home/u/Documents/sub/x")));
        assert!(a.revoke("files.read:/home/u/Documents"));
        assert!(!a.covers(&Resource::scoped("files.read", "/home/u/Documents/sub/x")));
    }
}
