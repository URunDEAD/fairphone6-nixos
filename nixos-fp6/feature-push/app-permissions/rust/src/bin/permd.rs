// SPDX-License-Identifier: GPL-3.0-or-later
//! fp6-permd: the runtime-permission broker (system bus, root).
//!
//! Apps (any process on the system bus):
//!   Request(s permission) -> s "granted"|"denied"       (may prompt)
//!   Check(s permission) -> s "granted"|"denied"|"ask"     (never prompts)
//!   OpenDevice(s node, s mode "rw"|"r") -> h              (USB permission)
//!   OpenSocket(s family "bluetooth"|"nfc", i type, i protocol) -> h
//! Daemons that guard a resource for their callers (an NFC daemon, a
//! contacts service; root, system users or `trusted_units`):
//!   Authorize(s permission, s caller_bus_name, s detail) -> s
//!   AuthorizePid(s permission, u pid, s detail) -> s
//!   AuthorizeApp(s permission, s app_id, s detail) -> s    (GeoClue's agent:
//!       it only knows the app id GeoClue derived from the client's cgroup)
//!   CheckPid(s permission, u pid) -> s, CheckApp(s permission, s app_id) -> s
//!       "granted"|"denied"|"ask" without prompting or logging
//!       (fp6-media-gate: the verdict it publishes to WirePlumber)
//! The phone user's session agent (fp6-permission-agent.service only):
//!   RegisterAgent(), GetState() -> s, SetDecision(s app, s perm, s decision),
//!   SetNetwork(s app, s class, b allowed), ResetApp(s app),
//!   SetForeground(s app), LogUse(s app, s what, s detail)
//! Signal Changed(b dropins): state changed (dropins: re-read systemd
//! drop-ins, i.e. `systemctl --user daemon-reload`).
//! Prompts: org.fairphone.PermissionAgent1.Prompt(s json) -> s answer on the
//! registered agent (/org/fairphone/PermissionAgent).

use fp6_permissions::broker::{self, Broker, Config, Creds, Grant};
use fp6_permissions::model::{Answer, Decision, Permission};
use fp6_permissions::policy::{Outcome, Subject};
use fp6_permissions::{device, AGENT_IFACE, AGENT_PATH, BUS_NAME, PATH};
use std::os::fd::{FromRawFd, OwnedFd};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use zbus::fdo;
use zbus::object_server::SignalEmitter;

fn log(msg: impl AsRef<str>) {
    eprintln!("fp6-permd: {}", msg.as_ref());
}

struct Api {
    b: Arc<Mutex<Broker>>,
    prompt: Arc<async_lock::Mutex<()>>,
}

fn denied(msg: impl Into<String>) -> fdo::Error {
    fdo::Error::AccessDenied(msg.into())
}
fn failed(msg: impl Into<String>) -> fdo::Error {
    fdo::Error::Failed(msg.into())
}

async fn creds_of(conn: &zbus::Connection, name: &str) -> fdo::Result<Creds> {
    let proxy = fdo::DBusProxy::new(conn).await?;
    let bus_name = zbus::names::BusName::try_from(name).map_err(|e| fdo::Error::InvalidArgs(e.to_string()))?;
    let c = proxy.get_connection_credentials(bus_name).await?;
    Ok(Creds { uid: c.unix_user_id(), pid: c.process_id(), label: c.linux_security_label().cloned() })
}

async fn caller(conn: &zbus::Connection, hdr: &zbus::message::Header<'_>) -> fdo::Result<(String, Creds)> {
    let sender = hdr.sender().ok_or_else(|| denied("no sender"))?.to_string();
    let c = creds_of(conn, &sender).await?;
    Ok((sender, c))
}

fn creds_of_pid(pid: u32) -> Creds {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
    let uid = status
        .lines()
        .find_map(|l| l.strip_prefix("Uid:"))
        .and_then(|v| v.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok());
    let label = std::fs::read(format!("/proc/{pid}/attr/apparmor/current"))
        .or_else(|_| std::fs::read(format!("/proc/{pid}/attr/current")))
        .ok();
    Creds { uid, pid: Some(pid), label }
}

fn perm(s: &str) -> fdo::Result<Permission> {
    Permission::parse(s).ok_or_else(|| fdo::Error::InvalidArgs(format!("unknown permission {s:?}")))
}

impl Api {
    /// Re-render enforcement, reload AppArmor, persist, tell the agent.
    async fn apply(&self, emitter: &SignalEmitter<'_>) {
        let (profiles_err, dropins) = {
            let mut b = self.b.lock().unwrap();
            if let Err(e) = b.save() {
                log(e);
            }
            match b.render_all() {
                Ok((profiles, dropins)) => (b.reload_profiles(&profiles), dropins),
                Err(e) => (vec![e], false),
            }
        };
        for e in profiles_err {
            log(format!("enforcement: {e}"));
        }
        let _ = Api::changed(emitter, dropins).await;
    }

    async fn ask_agent(&self, conn: &zbus::Connection, agent: &str, json: String, timeout: u64) -> Answer {
        let call = async {
            let msg = conn
                .call_method(Some(agent), AGENT_PATH, Some(AGENT_IFACE), "Prompt", &(json,))
                .await?;
            msg.body().deserialize::<String>()
        };
        let timer = async {
            async_io::Timer::after(Duration::from_secs(timeout + 10)).await;
            Err(zbus::Error::Failure("prompt timed out".into()))
        };
        match futures_lite::future::or(call, timer).await {
            Ok(a) => Answer::parse(&a),
            Err(e) => {
                log(format!("prompt: {e}"));
                Answer::Dismissed
            }
        }
    }

    /// The decision for one request, prompting if needed. Logs the use.
    async fn gate(&self, conn: &zbus::Connection, emitter: &SignalEmitter<'_>, s: &Subject, p: Permission, detail: &str) -> bool {
        let first = self.b.lock().unwrap().evaluate(s, p, broker::now());
        let granted = match first {
            Outcome::Granted => true,
            Outcome::Denied => false,
            Outcome::Prompt => {
                // One prompt at a time; a second request for the same app and
                // permission waiting here gets the first one's answer.
                let _one = self.prompt.lock().await;
                let (again, agent, timeout) = {
                    let b = self.b.lock().unwrap();
                    (b.evaluate(s, p, broker::now()), b.agent.clone(), b.cfg.prompt_timeout_secs)
                };
                match (again, agent, s) {
                    (Outcome::Granted, _, _) => true,
                    (Outcome::Denied, _, _) => false,
                    (Outcome::Prompt, None, _) => {
                        log(format!("no prompt agent: {} {} refused", s.key(), p.id()));
                        false
                    }
                    (Outcome::Prompt, Some(agent), Subject::App { id, confined }) => {
                        let json = serde_json::json!({
                            "app": id,
                            "perm": p.id(),
                            "title": p.title(),
                            "phrase": p.phrase(),
                            "detail": detail,
                            "confined": confined,
                            "timeout": timeout,
                            "options": p.prompt_options(),
                        })
                        .to_string();
                        let answer = self.ask_agent(conn, &agent, json, timeout).await;
                        {
                            let mut b = self.b.lock().unwrap();
                            b.log_prompt(id, p, answer, broker::now());
                        }
                        if answer.remembered().is_some() {
                            // Before handing anything out: a confined app's
                            // profile must allow what it receives.
                            self.apply(emitter).await;
                        }
                        answer.granted()
                    }
                    (Outcome::Prompt, Some(_), Subject::System { .. }) => true,
                }
            }
        };
        {
            let mut b = self.b.lock().unwrap();
            b.record(s, p, granted, detail, broker::now());
            if let Err(e) = b.save() {
                log(e);
            }
        }
        let _ = Api::changed(emitter, false).await;
        granted
    }

    fn require_agent(&self, sender: &str) -> fdo::Result<()> {
        if self.b.lock().unwrap().agent.as_deref() == Some(sender) {
            Ok(())
        } else {
            Err(denied("only the permission agent may do this"))
        }
    }
}

fn outcome(o: Outcome) -> String {
    match o {
        Outcome::Granted => "granted",
        Outcome::Denied => "denied",
        Outcome::Prompt => "ask",
    }
    .into()
}

fn answer(g: bool) -> String {
    if g { "granted" } else { "denied" }.to_owned()
}

#[zbus::interface(name = "org.fairphone.Permissions1")]
impl Api {
    async fn request(
        &self,
        permission: &str,
        #[zbus(header)] hdr: zbus::message::Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<String> {
        let p = perm(permission)?;
        let (_, c) = caller(conn, &hdr).await?;
        let s = self.b.lock().unwrap().subject(&c);
        Ok(answer(self.gate(conn, &emitter, &s, p, "").await))
    }

    async fn check(
        &self,
        permission: &str,
        #[zbus(header)] hdr: zbus::message::Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> fdo::Result<String> {
        let p = perm(permission)?;
        let (_, c) = caller(conn, &hdr).await?;
        let b = self.b.lock().unwrap();
        let s = b.subject(&c);
        Ok(outcome(b.evaluate(&s, p, broker::now())))
    }

    async fn open_device(
        &self,
        node: &str,
        mode: &str,
        #[zbus(header)] hdr: zbus::message::Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<zbus::zvariant::OwnedFd> {
        if !self.b.lock().unwrap().cfg.devices {
            return Err(denied("device access is switched off"));
        }
        let write = match mode {
            "rw" | "" => true,
            "r" => false,
            _ => return Err(fdo::Error::InvalidArgs("mode must be rw or r".into())),
        };
        let info = device::inspect(node, Path::new("/"), Path::new("/")).map_err(fdo::Error::InvalidArgs)?;
        let (_, c) = caller(conn, &hdr).await?;
        let s = self.b.lock().unwrap().subject(&c);
        if !self.gate(conn, &emitter, &s, Permission::Usb, &info.label()).await {
            return Err(denied(format!("USB access for {} is not allowed", s.key())));
        }
        let f = device::open(&info, Path::new("/"), write).map_err(failed)?;
        if let Subject::App { id, .. } = &s {
            let mut b = self.b.lock().unwrap();
            if !b.cfg.is_system_app(id) {
                b.grants.push(Grant {
                    app: id.clone(),
                    perm: Permission::Usb,
                    node: info.node.clone(),
                    usb_sysfs: info.usb_sysfs.clone(),
                    since: broker::now(),
                });
            }
        }
        Ok(OwnedFd::from(f).into())
    }

    async fn open_socket(
        &self,
        family: &str,
        sock_type: i32,
        protocol: i32,
        #[zbus(header)] hdr: zbus::message::Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<zbus::zvariant::OwnedFd> {
        if !self.b.lock().unwrap().cfg.devices {
            return Err(denied("device access is switched off"));
        }
        // Only connection-level sockets: no raw HCI (adapter control /
        // sniffing) and no SCO (audio belongs to the audio stack).
        let (af, p) = match (family, sock_type, protocol) {
            ("bluetooth", libc::SOCK_SEQPACKET | libc::SOCK_STREAM | libc::SOCK_DGRAM, 0) => (libc::AF_BLUETOOTH, Permission::Bluetooth),
            ("bluetooth", libc::SOCK_STREAM, 3) => (libc::AF_BLUETOOTH, Permission::Bluetooth),
            ("nfc", libc::SOCK_SEQPACKET, 0) | ("nfc", libc::SOCK_STREAM | libc::SOCK_DGRAM, 1) => (libc::AF_NFC, Permission::Nfc),
            _ => return Err(fdo::Error::InvalidArgs("unsupported socket".into())),
        };
        let (_, c) = caller(conn, &hdr).await?;
        let s = self.b.lock().unwrap().subject(&c);
        if !self.gate(conn, &emitter, &s, p, "").await {
            return Err(denied(format!("{} for {} is not allowed", p.title(), s.key())));
        }
        // SAFETY: plain socket(2); the descriptor is owned right away.
        let fd = unsafe { libc::socket(af, sock_type | libc::SOCK_CLOEXEC, protocol) };
        if fd < 0 {
            return Err(failed(std::io::Error::last_os_error().to_string()));
        }
        Ok(unsafe { OwnedFd::from_raw_fd(fd) }.into())
    }

    async fn authorize(
        &self,
        permission: &str,
        subject: &str,
        detail: &str,
        #[zbus(header)] hdr: zbus::message::Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<String> {
        let p = perm(permission)?;
        let (_, c) = caller(conn, &hdr).await?;
        if !self.b.lock().unwrap().is_trusted_daemon(&c) {
            return Err(denied("caller may not authorize on behalf of others"));
        }
        let sc = creds_of(conn, subject).await?;
        let s = self.b.lock().unwrap().subject(&sc);
        Ok(answer(self.gate(conn, &emitter, &s, p, &detail.chars().take(120).collect::<String>()).await))
    }

    async fn authorize_pid(
        &self,
        permission: &str,
        pid: u32,
        detail: &str,
        #[zbus(header)] hdr: zbus::message::Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<String> {
        let p = perm(permission)?;
        let (_, c) = caller(conn, &hdr).await?;
        if !self.b.lock().unwrap().is_trusted_daemon(&c) {
            return Err(denied("caller may not authorize on behalf of others"));
        }
        let s = self.b.lock().unwrap().subject(&creds_of_pid(pid));
        Ok(answer(self.gate(conn, &emitter, &s, p, &detail.chars().take(120).collect::<String>()).await))
    }

    async fn authorize_app(
        &self,
        permission: &str,
        app_id: &str,
        detail: &str,
        #[zbus(header)] hdr: zbus::message::Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<String> {
        let p = perm(permission)?;
        let (_, c) = caller(conn, &hdr).await?;
        if !self.b.lock().unwrap().is_trusted_daemon(&c) {
            return Err(denied("caller may not authorize on behalf of others"));
        }
        let app = app_id.trim_end_matches(".desktop");
        if !fp6_permissions::appid::valid_app_id(app) {
            return Err(fdo::Error::InvalidArgs("bad app id".into()));
        }
        let s = self.b.lock().unwrap().app_subject(app);
        Ok(answer(self.gate(conn, &emitter, &s, p, &detail.chars().take(120).collect::<String>()).await))
    }

    async fn check_pid(
        &self,
        permission: &str,
        pid: u32,
        #[zbus(header)] hdr: zbus::message::Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> fdo::Result<String> {
        let p = perm(permission)?;
        let (_, c) = caller(conn, &hdr).await?;
        let b = self.b.lock().unwrap();
        if !b.is_trusted_daemon(&c) {
            return Err(denied("caller may not check on behalf of others"));
        }
        let s = b.subject(&creds_of_pid(pid));
        Ok(outcome(b.evaluate(&s, p, broker::now())))
    }

    async fn check_app(
        &self,
        permission: &str,
        app_id: &str,
        #[zbus(header)] hdr: zbus::message::Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> fdo::Result<String> {
        let p = perm(permission)?;
        let (_, c) = caller(conn, &hdr).await?;
        let b = self.b.lock().unwrap();
        if !b.is_trusted_daemon(&c) {
            return Err(denied("caller may not check on behalf of others"));
        }
        let app = app_id.trim_end_matches(".desktop");
        if !fp6_permissions::appid::valid_app_id(app) {
            return Err(fdo::Error::InvalidArgs("bad app id".into()));
        }
        Ok(outcome(b.evaluate(&b.app_subject(app), p, broker::now())))
    }

    async fn register_agent(
        &self,
        #[zbus(header)] hdr: zbus::message::Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        let (sender, c) = caller(conn, &hdr).await?;
        {
            let mut b = self.b.lock().unwrap();
            if !b.is_agent_unit(&c) {
                return Err(denied(format!("only {} of uid {} may register", b.cfg.agent_unit, b.cfg.uid)));
            }
            log(format!("agent {sender} registered"));
            b.agent = Some(sender);
        }
        let _ = Api::changed(&emitter, false).await;
        Ok(())
    }

    async fn get_state(&self, #[zbus(header)] hdr: zbus::message::Header<'_>) -> fdo::Result<String> {
        let sender = hdr.sender().ok_or_else(|| denied("no sender"))?.to_string();
        self.require_agent(&sender)?;
        Ok(self.b.lock().unwrap().state_json(broker::now()))
    }

    async fn set_decision(
        &self,
        app: &str,
        permission: &str,
        decision: &str,
        #[zbus(header)] hdr: zbus::message::Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        let sender = hdr.sender().ok_or_else(|| denied("no sender"))?.to_string();
        self.require_agent(&sender)?;
        let p = perm(permission)?;
        let d = Decision::parse(decision).ok_or_else(|| fdo::Error::InvalidArgs("bad decision".into()))?;
        if !fp6_permissions::appid::valid_app_id(app) {
            return Err(fdo::Error::InvalidArgs("bad app id".into()));
        }
        let revoke = self.b.lock().unwrap().set_decision(app, p, d, broker::now());
        revoke_all(&revoke);
        self.apply(&emitter).await;
        Ok(())
    }

    async fn set_network(
        &self,
        app: &str,
        class: &str,
        allowed: bool,
        #[zbus(header)] hdr: zbus::message::Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        let sender = hdr.sender().ok_or_else(|| denied("no sender"))?.to_string();
        self.require_agent(&sender)?;
        if !fp6_permissions::appid::valid_app_id(app) {
            return Err(fdo::Error::InvalidArgs("bad app id".into()));
        }
        self.b.lock().unwrap().set_network(app, class, allowed, broker::now()).map_err(fdo::Error::InvalidArgs)?;
        self.apply(&emitter).await;
        Ok(())
    }

    async fn reset_app(
        &self,
        app: &str,
        #[zbus(header)] hdr: zbus::message::Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        let sender = hdr.sender().ok_or_else(|| denied("no sender"))?.to_string();
        self.require_agent(&sender)?;
        let revoke = self.b.lock().unwrap().reset_app(app, broker::now());
        revoke_all(&revoke);
        self.apply(&emitter).await;
        Ok(())
    }

    async fn set_foreground(&self, app: &str, #[zbus(header)] hdr: zbus::message::Header<'_>) -> fdo::Result<()> {
        let sender = hdr.sender().ok_or_else(|| denied("no sender"))?.to_string();
        self.require_agent(&sender)?;
        let app = (!app.is_empty() && fp6_permissions::appid::valid_app_id(app)).then(|| app.to_owned());
        self.b.lock().unwrap().set_focus(app, broker::now());
        Ok(())
    }

    async fn log_use(&self, app: &str, what: &str, detail: &str, #[zbus(header)] hdr: zbus::message::Header<'_>) -> fdo::Result<()> {
        let sender = hdr.sender().ok_or_else(|| denied("no sender"))?.to_string();
        self.require_agent(&sender)?;
        if !["camera", "microphone", "location"].contains(&what) || app.len() > 180 {
            return Err(fdo::Error::InvalidArgs("bad usage event".into()));
        }
        let mut b = self.b.lock().unwrap();
        b.store.log(fp6_permissions::model::Event {
            time: broker::now(),
            app: app.into(),
            perm: what.into(),
            kind: fp6_permissions::model::EventKind::Used,
            detail: detail.chars().take(120).collect(),
        });
        Ok(())
    }

    #[zbus(signal)]
    async fn changed(emitter: &SignalEmitter<'_>, dropins: bool) -> zbus::Result<()>;
}

fn revoke_all(grants: &[Grant]) {
    for g in grants {
        match device::revoke(&g.usb_sysfs, Path::new("/")) {
            Ok(()) => log(format!("revoked {} from {}", g.node, g.app)),
            Err(e) => log(format!("revoking {} from {}: {e}", g.node, g.app)),
        }
    }
}

/// `--sample-include allow|deny`: the largest AppArmor include of each
/// kind, for the build-time profile compile check.
fn sample_include(kind: &str) {
    use fp6_permissions::model::NetAccess;
    use fp6_permissions::render;
    let d = if kind == "allow" { Decision::Allow } else { Decision::Deny };
    let all: render::Decisions = Permission::ALL.iter().map(|p| (*p, d)).collect();
    let mut net = NetAccess::default();
    if kind != "allow" {
        net.set("all", false);
    }
    print!("{}", render::apparmor_include("sample", &all, &net));
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|a| a == "--sample-include") {
        sample_include(args.get(2).map(String::as_str).unwrap_or("deny"));
        return;
    }
    let Some(path) = args.get(1).filter(|a| !a.starts_with('-')).or(args.get(2)) else {
        eprintln!("usage: fp6-permd [--check] CONFIG.json");
        std::process::exit(2);
    };
    let cfg: Config = match std::fs::read_to_string(path).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string())) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("fp6-permd: {path}: {e}");
            std::process::exit(2);
        }
    };
    let mut b = match Broker::new(cfg) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("fp6-permd: {e}");
            std::process::exit(2);
        }
    };
    if args.iter().any(|a| a == "--check") {
        println!("config ok");
        return;
    }
    match b.render_all() {
        Ok((profiles, _)) => {
            for e in b.reload_profiles(&profiles) {
                log(format!("enforcement: {e}"));
            }
        }
        Err(e) => log(format!("enforcement: {e}")),
    }
    let b = Arc::new(Mutex::new(b));
    let api = Api { b: b.clone(), prompt: Arc::new(async_lock::Mutex::new(())) };
    let r: zbus::Result<()> = async_io::block_on(async {
        let conn = zbus::connection::Builder::system()?.name(BUS_NAME)?.serve_at(PATH, api)?.build().await?;
        let emitter = SignalEmitter::new(&conn, PATH)?;
        loop {
            async_io::Timer::after(Duration::from_secs(2)).await;
            // "While using" grants of apps in the background, unplugged devices.
            let (due, once_ended) = {
                let mut b = b.lock().unwrap();
                b.grants.retain(|g| g.usb_sysfs.join("authorized").exists());
                let now = broker::now();
                (b.take_revocable(now), b.expire_once(now))
            };
            if once_ended && due.is_empty() {
                let _ = Api::changed(&emitter, false).await;
            }
            if !due.is_empty() {
                revoke_all(&due);
                if let Err(e) = b.lock().unwrap().save() {
                    log(e);
                }
                let _ = Api::changed(&emitter, false).await;
            }
        }
    });
    if let Err(e) = r {
        log(format!("{e}"));
        std::process::exit(1);
    }
}
