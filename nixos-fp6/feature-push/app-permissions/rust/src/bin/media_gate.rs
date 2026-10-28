// SPDX-License-Identifier: GPL-3.0-or-later
//! fp6-media-gate: camera and microphone permissions for PipeWire clients
//! (systemd user service fp6-media-gate.service of the phone user).
//!
//!  * Follows the PipeWire graph (`pw-dump --monitor`), works out which app
//!    each client is (src/media.rs `identify`; apps by AppArmor label or app
//!    cgroup, everything else is a system component) and publishes the
//!    broker's verdict per client in the metadata object `fp6-permissions`
//!    (`pw-metadata`), which WirePlumber's fp6-media-permissions.lua
//!    enforces: no link from a microphone / camera source to a capture
//!    stream unless the verdict is "granted"; sources hidden from denied
//!    clients.
//!  * A new capture stream of an app asks fp6-permd (AuthorizePid /
//!    AuthorizeApp): the prompt when undecided, the use or refusal in the
//!    history otherwise. The answer updates the verdict, WirePlumber links.
//!  * Verdicts are re-checked when the broker signals a change and every few
//!    seconds ("while using" and "only this time" end in the background).
//!  * `fp6.gate` = `active:<serial>` on the gate's own PipeWire client (its
//!    pw-dump connection, on the manager socket): WirePlumber enforces only
//!    while that client exists, so a stopped or crashed gate never blocks
//!    audio.

use fp6_permissions::media::{self, Graph, Who};
use fp6_permissions::model::Permission;
use fp6_permissions::{appid, BUS_NAME, PATH};
use std::collections::{BTreeMap, BTreeSet};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn log(msg: impl AsRef<str>) {
    eprintln!("fp6-media-gate: {}", msg.as_ref());
}

const RECHECK_SECS: u64 = 5;
const OWN_UNIT: &str = "fp6-media-gate.service";

struct State {
    graph: Graph,
    who: BTreeMap<u64, Who>,
    /// (client, permission) -> verdict we want published
    verdicts: BTreeMap<(u64, Permission), String>,
    /// (client, key) -> value in the metadata object
    published: BTreeMap<(u64, &'static str), String>,
    gate_published: bool,
    /// pid of our pw-dump (its client carries the gate key)
    dump_pid: u32,
    inflight: BTreeSet<(String, Permission)>,
}

struct Gate {
    st: Mutex<State>,
    /// one pw-metadata at a time, in order
    writer: Mutex<()>,
    bus: Option<zbus::blocking::Connection>,
    proc_root: PathBuf,
    pw_metadata: String,
    remote: String,
}

/// The app a process belongs to, or None for a system component (mirrors
/// fp6-permd's identity rules, so system components never wait for it).
fn local_app(proc_root: &Path, pid: u32) -> Option<String> {
    let base = proc_root.join(pid.to_string());
    let label = std::fs::read(base.join("attr/apparmor/current")).or_else(|_| std::fs::read(base.join("attr/current"))).ok();
    if let Some(p) = label.as_deref().and_then(appid::apparmor_profile) {
        return Some(if appid::valid_app_id(&p) { p } else { "unknown".into() });
    }
    let Some(cg) = std::fs::read_to_string(base.join("cgroup")).ok().and_then(|t| appid::cgroup_of(&t)) else {
        // gone or unreadable: an unknown app, never silently trusted
        return Some("unknown".into());
    };
    appid::app_from_cgroup(&cg).map(|(id, _)| id)
}

fn run_timeout(cmd: &mut Command, limit: Duration) -> bool {
    let Ok(mut child) = cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else { return false };
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(s)) => return s.success(),
            Ok(None) if start.elapsed() < limit => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

impl Gate {
    fn call(&self, method: &str, body: &(impl serde::Serialize + zbus::zvariant::DynamicType)) -> Option<String> {
        let bus = self.bus.as_ref()?;
        match bus.call_method(Some(BUS_NAME), PATH, Some(BUS_NAME), method, body) {
            Ok(m) => m.body().deserialize::<String>().ok(),
            Err(e) => {
                log(format!("{method}: {e}"));
                None
            }
        }
    }

    /// Key for de-duplicating prompts and the app it names.
    fn app_key(&self, who: &Who) -> Option<String> {
        match who {
            Who::System(_) => None,
            Who::App(a) => Some(a.clone()),
            Who::Pid { pid, .. } => local_app(&self.proc_root, *pid),
        }
    }

    fn verdict(&self, who: &Who, perm: Permission) -> String {
        let v = match who {
            Who::System(_) => Some("granted".to_owned()),
            Who::App(a) => self.call("CheckApp", &(perm.id(), a.as_str())),
            Who::Pid { pid, .. } => match local_app(&self.proc_root, *pid) {
                None => Some("granted".to_owned()),
                Some(_) => self.call("CheckPid", &(perm.id(), *pid)),
            },
        };
        media::valid_verdict(v.as_deref().unwrap_or("ask")).to_owned()
    }

    fn own_client(&self, props: &BTreeMap<String, serde_json::Value>) -> bool {
        media::prop_u64(props, "pipewire.sec.pid")
            .and_then(|p| u32::try_from(p).ok())
            .and_then(|p| media::unit_of(&self.proc_root, p))
            .is_some_and(|u| u == OWN_UNIT)
    }

    /// Work out verdicts for these clients (all when None).
    fn recheck(&self, only: Option<&[u64]>) {
        let todo: Vec<(u64, Who)> = {
            let mut st = self.st.lock().unwrap();
            let ids: Vec<u64> = match only {
                Some(ids) => ids.to_vec(),
                None => st.graph.clients.keys().copied().collect(),
            };
            let mut out = vec![];
            for id in ids {
                let Some(c) = st.graph.clients.get(&id) else { continue };
                if self.own_client(&c.props) {
                    continue;
                }
                let who = match st.who.get(&id) {
                    Some(w) => w.clone(),
                    None => {
                        let w = media::identify(&c.props, &self.proc_root);
                        if let Who::Pid { pid, claimed: true } = &w {
                            log(format!("client {id}: pid {pid} as claimed by the client (pipewire-pulse without the peer-pid patch)"));
                        }
                        st.who.insert(id, w.clone());
                        w
                    }
                };
                out.push((id, who));
            }
            out
        };
        // Broker calls without the lock (they can take a moment).
        let mut results = vec![];
        for (id, who) in &todo {
            for perm in media::GATED {
                results.push(((*id, perm), self.verdict(who, perm)));
            }
        }
        {
            let mut st = self.st.lock().unwrap();
            for (k, v) in results {
                if st.graph.clients.contains_key(&k.0) {
                    st.verdicts.insert(k, v);
                }
            }
        }
        self.publish();
    }

    /// Bring the metadata object in line with the wanted verdicts.
    fn publish(&self) {
        let _w = self.writer.lock().unwrap();
        let (writes, deletes, gate) = {
            let mut st = self.st.lock().unwrap();
            if st.graph.metadata.is_none() {
                return;
            }
            let dump_pid = st.dump_pid;
            let own = st.graph.clients.iter().find(|(_, c)| media::prop_u64(&c.props, "pipewire.sec.pid") == Some(dump_pid as u64))
                .map(|(id, c)| (*id, media::prop_u64(&c.props, "object.serial").unwrap_or(0)));
            let mut writes = vec![];
            let wanted: Vec<((u64, Permission), String)> = st.verdicts.iter().map(|(k, v)| (*k, v.clone())).collect();
            for ((id, perm), v) in wanted {
                let serial = st.graph.clients.get(&id).and_then(|c| media::prop_u64(&c.props, "object.serial"));
                let val = media::value(serial, &v);
                let k = (id, media::key(perm));
                if st.published.get(&k) != Some(&val) {
                    st.published.insert(k, val.clone());
                    writes.push((id, k.1, val));
                }
            }
            let gone: Vec<u64> = st
                .published
                .keys()
                .map(|(id, _)| *id)
                .filter(|id| !st.graph.clients.contains_key(id))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            for id in &gone {
                st.published.retain(|(i, _), _| i != id);
            }
            // Active only once every client present has its verdict.
            let gate = if st.gate_published { None } else { own };
            if gate.is_some() {
                st.gate_published = true;
            }
            (writes, gone, gate)
        };
        for (id, k, v) in writes {
            self.meta(&[&id.to_string(), k, &v]);
        }
        for id in deletes {
            self.meta(&["-d", &id.to_string()]);
        }
        if let Some((id, serial)) = gate {
            if self.meta(&[&id.to_string(), media::GATE_KEY, &format!("active:{serial}")]) {
                log("enforcing (fp6.gate active)");
            } else {
                self.st.lock().unwrap().gate_published = false;
            }
        }
    }

    fn meta(&self, args: &[&str]) -> bool {
        let mut cmd = Command::new(&self.pw_metadata);
        cmd.args(["-r", &self.remote, "-n", media::METADATA_NAME]).args(args);
        let ok = run_timeout(&mut cmd, Duration::from_secs(3));
        if !ok {
            log(format!("pw-metadata {} failed", args.join(" ")));
            // try again with the next publish
            let mut st = self.st.lock().unwrap();
            if let (Some(id), Some(k)) = (args.first().and_then(|a| a.parse::<u64>().ok()), args.get(1)) {
                st.published.retain(|(i, kk), _| !(*i == id && kk == k));
            }
        }
        ok
    }

    /// A capture stream appeared: ask the broker (prompt when undecided,
    /// history entry otherwise), then re-check that app's clients.
    fn on_capture(self: &Arc<Self>, node: u64, client: u64, perm: Permission) {
        let who = {
            let st = self.st.lock().unwrap();
            st.who.get(&client).cloned()
        };
        let Some(who) = who else { return };
        let Some(app) = self.app_key(&who) else { return };
        {
            let mut st = self.st.lock().unwrap();
            if !st.inflight.insert((app.clone(), perm)) {
                return;
            }
        }
        let g = self.clone();
        std::thread::spawn(move || {
            let detail = String::new();
            let r = match &who {
                Who::App(a) => g.call("AuthorizeApp", &(perm.id(), a.as_str(), detail.as_str())),
                Who::Pid { pid, .. } => g.call("AuthorizePid", &(perm.id(), *pid, detail.as_str())),
                Who::System(_) => None,
            };
            log(format!("{app} {} (stream {node}): {}", perm.id(), r.as_deref().unwrap_or("no answer")));
            g.st.lock().unwrap().inflight.remove(&(app, perm));
            g.recheck(None);
        });
    }
}

fn watch_broker(g: Arc<Gate>) {
    let Some(bus) = g.bus.clone() else { return };
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(BUS_NAME)
        .and_then(|b| b.interface(BUS_NAME))
        .and_then(|b| b.member("Changed"))
        .map(|b| b.build());
    let rule = match rule {
        Ok(r) => r,
        Err(e) => return log(format!("match rule: {e}")),
    };
    match zbus::blocking::MessageIterator::for_match_rule(rule, &bus, Some(64)) {
        Ok(it) => {
            for _ in it {
                g.recheck(None);
            }
        }
        Err(e) => log(format!("broker signal: {e}")),
    }
}

fn main() {
    let pw_dump = std::env::var("FP6_PW_DUMP").unwrap_or_else(|_| "pw-dump".into());
    let pw_metadata = std::env::var("FP6_PW_METADATA").unwrap_or_else(|_| "pw-metadata".into());
    let remote = std::env::var("FP6_PW_REMOTE").unwrap_or_else(|_| "pipewire-0-manager".into());
    let bus = match zbus::blocking::Connection::system() {
        Ok(c) => Some(c),
        Err(e) => {
            log(format!("system bus: {e}; apps get \"ask\" (no access) until it is back"));
            None
        }
    };
    let g = Arc::new(Gate {
        st: Mutex::new(State {
            graph: Graph::default(),
            who: BTreeMap::new(),
            verdicts: BTreeMap::new(),
            published: BTreeMap::new(),
            gate_published: false,
            dump_pid: 0,
            inflight: BTreeSet::new(),
        }),
        writer: Mutex::new(()),
        bus,
        proc_root: "/proc".into(),
        pw_metadata,
        remote,
    });
    {
        let g = g.clone();
        std::thread::spawn(move || watch_broker(g));
    }
    {
        let g = g.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(RECHECK_SECS));
            g.recheck(None);
        });
    }
    let mut child = match Command::new(&pw_dump).args(["--monitor", "--no-colors"]).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() {
        Ok(c) => c,
        Err(e) => {
            log(format!("{pw_dump}: {e}"));
            std::process::exit(1);
        }
    };
    g.st.lock().unwrap().dump_pid = child.id();
    let out = child.stdout.take().unwrap();
    let stream = serde_json::Deserializer::from_reader(BufReader::new(out)).into_iter::<serde_json::Value>();
    for doc in stream {
        let Ok(doc) = doc else { break };
        let ch = {
            let mut st = g.st.lock().unwrap();
            let ch = st.graph.apply(&doc);
            for id in &ch.clients_removed {
                st.who.remove(id);
                st.verdicts.retain(|(c, _), _| c != id);
            }
            if ch.metadata_added {
                // WirePlumber (re)started: its metadata object is empty.
                st.published.clear();
                st.gate_published = false;
            }
            ch
        };
        if ch.metadata_added {
            log("fp6-permissions metadata present: publishing");
            g.recheck(None);
        } else if !ch.clients_added.is_empty() {
            g.recheck(Some(&ch.clients_added));
        } else if !ch.clients_removed.is_empty() {
            g.publish();
        }
        for (node, client, perm) in ch.captures {
            g.on_capture(node, client, perm);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    log("pw-dump ended (PipeWire restarted?); exiting for a restart");
    std::process::exit(1);
}
