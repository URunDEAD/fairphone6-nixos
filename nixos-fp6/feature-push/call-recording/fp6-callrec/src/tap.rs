// SPDX-License-Identifier: GPL-3.0-only
//! Where the two directions of a call come from. One `CallTap` per call
//! path:
//!
//!  * `PwTap::volte`: imsd decodes RTP itself and plays the far end with
//!    `pw-play --media-role Phone` and reads the microphone with
//!    `pw-record --media-role Phone`. The downlink recorder is linked to the
//!    output port of imsd's playback stream (exactly what imsd decoded,
//!    before the speaker volume); the uplink recorder is linked to whatever
//!    feeds imsd's capture stream (the mic, or the echo canceller's source
//!    when one is inserted: "mic after processing" as far as PipeWire sees
//!    it; imsd's own AMR encoding happens after that).
//!  * `PwTap::fallback` (debug): the default source (mic) and the monitor of
//!    the default sink (whatever is being played). For a CS call whose audio
//!    runs DSP-to-codec this is mostly silence; the sidecar says so.
//!  * `AlsaTap` (CS, optional): an AudioReach in-call record front end
//!    (Qualcomm "incall record", VOICE_UPLINK/DOWNLINK taps) opened as an
//!    ALSA capture PCM, configured in /etc/fp6-callrec/cs-incall-record.conf;
//!    see cs-incall-record/ for the topology side. Not usable until the CS
//!    voice graph (cs-call-audio) works.
//!
//! The recorder streams are `pw-record --raw` children with
//! node.autoconnect=false; the tap links them with pw-link and re-checks
//! the links about once a second (imsd restarting its streams, the default
//! device changing, a stream appearing only after the call connects).

use crate::calls::CallPath;
use crate::pwgraph::{self, Graph, OWN_PREFIX};
use crate::track::{s16le, Msg};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnnounceResult {
    pub uplink: bool,
    pub local: bool,
    pub error: Option<String>,
}

pub trait CallTap: Send {
    fn path(&self) -> CallPath;
    /// The rate the call audio has natively, if the tap knows it.
    fn native_rate(&self) -> Option<u32>;
    fn start(&mut self, rate: u32, uplink: Sender<Msg>, downlink: Sender<Msg>) -> Result<(), String>;
    /// Called about once a second while recording. Returns log lines.
    fn maintain(&mut self) -> Vec<String>;
    /// Play the announcement tone into the call (and the recording).
    fn announce(&mut self, wav: &Path) -> AnnounceResult;
    /// Devices/streams the recording was taken from (for the sidecar).
    fn devices(&self) -> Value;
    fn stop(&mut self);
}

/// One `pw-record --raw` child feeding a track.
pub struct PwRecorder {
    pub node_name: String,
    child: Child,
    reader: Option<JoinHandle<()>>,
}

impl PwRecorder {
    pub fn spawn(node_name: &str, description: &str, rate: u32, sink: Sender<Msg>) -> Result<PwRecorder, String> {
        let props = format!(
            "{{ node.name = \"{node_name}\" node.description = \"{description}\" application.name = \"Call recording\" \
             application.id = \"fp6-callrec\" node.autoconnect = false node.dont-reconnect = true }}"
        );
        let mut child = Command::new("pw-record")
            .args(["--raw", "--rate", &rate.to_string(), "--channels", "1", "--format", "s16", "--latency", "20ms", "--target", "0", "-P", &props, "-"])
            // fp6-privacy-indicator names microphone users by APP_ID and the
            // desktop entry fp6-callrec.desktop ("Call recording").
            .env("APP_ID", "fp6-callrec")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("pw-record: {e}"))?;
        let mut out = child.stdout.take().ok_or("pw-record: no stdout")?;
        let reader = std::thread::Builder::new()
            .name(format!("rd-{node_name}"))
            .spawn(move || {
                let mut buf = vec![0u8; 8192];
                let mut carry = Vec::new();
                loop {
                    match out.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let pcm = s16le(&mut carry, &buf[..n]);
                            if !pcm.is_empty() && sink.send(Msg::Pcm(Instant::now(), pcm)).is_err() {
                                break;
                            }
                        }
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(PwRecorder { node_name: node_name.into(), child, reader: Some(reader) })
    }

    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub fn stop(&mut self) {
        // SIGTERM: pw-record flushes and exits; then the reader sees EOF.
        unsafe { libc::kill(self.child.id() as i32, libc::SIGTERM) };
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if !matches!(self.child.try_wait(), Ok(None)) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(r) = self.reader.take() {
            let _ = r.join();
        }
    }
}

impl Drop for PwRecorder {
    fn drop(&mut self) {
        if self.reader.is_some() {
            self.stop();
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PwKind {
    Volte,
    Fallback,
}

pub struct PwTap {
    kind: PwKind,
    session: String,
    up: Option<PwRecorder>,
    down: Option<PwRecorder>,
    up_devices: BTreeSet<String>,
    down_devices: BTreeSet<String>,
    native: Option<u32>,
    announce_player: Option<Child>,
    warned: BTreeSet<String>,
}

/// Which output ports should feed the recorder's uplink / downlink input.
/// (output port, label) pairs.
pub type PortList = Vec<(u32, String)>;

pub fn desired_links(kind_volte: bool, g: &Graph) -> (PortList, PortList) {
    let label = |id: u32| {
        g.ports
            .get(&id)
            .and_then(|p| g.nodes.get(&p.node))
            .map(|n| {
                let what = if n.description.is_empty() { n.name.clone() } else { format!("{} ({})", n.description, n.name) };
                if n.pid > 0 {
                    format!("{what} [{} pid {}]", n.binary, n.pid)
                } else {
                    what
                }
            })
            .unwrap_or_else(|| format!("port {id}"))
    };
    let mut up = Vec::new();
    let mut down = Vec::new();
    if kind_volte {
        let (play, cap) = g.phone_streams();
        if let Some(p) = play.first().and_then(|n| g.tap_port(n.id, false)) {
            down.push((p, label(p)));
        }
        if let Some(c) = cap.first() {
            for p in g.feeding_ports(c.id) {
                up.push((p, label(p)));
            }
        }
    } else {
        // A virtual source (null sink in source mode, e.g. a loopback) only
        // has monitor-type capture ports.
        if let Some(p) = g.default_source_node().and_then(|n| g.tap_port(n.id, false).or_else(|| g.tap_port(n.id, true))) {
            up.push((p, label(p)));
        }
        if let Some(p) = g.default_sink_node().and_then(|n| g.tap_port(n.id, true)) {
            down.push((p, format!("monitor of {}", label(p))));
        }
    }
    (up, down)
}

impl PwTap {
    fn new(kind: PwKind) -> PwTap {
        static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
        let session = format!("{}-{}-{}", OWN_PREFIX, std::process::id(), SEQ.fetch_add(1, Ordering::SeqCst));
        let native = match kind {
            PwKind::Volte => Graph::dump().ok().and_then(|g| g.phone_streams().0.first().and_then(|n| n.rate)),
            PwKind::Fallback => None,
        };
        PwTap {
            kind,
            session,
            up: None,
            down: None,
            up_devices: BTreeSet::new(),
            down_devices: BTreeSet::new(),
            native,
            announce_player: None,
            warned: BTreeSet::new(),
        }
    }

    pub fn volte() -> PwTap {
        PwTap::new(PwKind::Volte)
    }

    pub fn fallback() -> PwTap {
        PwTap::new(PwKind::Fallback)
    }

    fn ensure(g: &Graph, target: &str, want: &[(u32, String)], log: &mut Vec<String>, seen: &mut BTreeSet<String>) {
        let Some(node) = g.node_by_name(target) else { return };
        let Some(&inp) = g.input_ports(node.id).first() else { return };
        let wanted: Vec<u32> = want.iter().map(|(p, _)| *p).collect();
        for (p, label) in want {
            if !g.linked(*p, inp) {
                match pwgraph::link(*p, inp) {
                    Ok(()) => log.push(format!("{target}: linked {label}")),
                    Err(e) => log.push(format!("{target}: {e}")),
                }
            }
            seen.insert(label.clone());
        }
        // Drop links from sources that are no longer wanted (device switch),
        // but keep the announcement player's.
        for (id, out) in g.links_into(inp) {
            let from_own = g.ports.get(&out).and_then(|p| g.nodes.get(&p.node)).is_some_and(|n| n.name.starts_with(OWN_PREFIX));
            if !wanted.contains(&out) && !from_own {
                match pwgraph::unlink(id) {
                    Ok(()) => log.push(format!("{target}: unlinked port {out}")),
                    Err(e) => log.push(format!("{target}: {e}")),
                }
            }
        }
    }
}

impl CallTap for PwTap {
    fn path(&self) -> CallPath {
        match self.kind {
            PwKind::Volte => CallPath::Volte,
            PwKind::Fallback => CallPath::Fallback,
        }
    }

    fn native_rate(&self) -> Option<u32> {
        self.native
    }

    fn start(&mut self, rate: u32, uplink: Sender<Msg>, downlink: Sender<Msg>) -> Result<(), String> {
        self.up = Some(PwRecorder::spawn(&format!("{}-uplink", self.session), "Call recording (uplink)", rate, uplink)?);
        self.down = Some(PwRecorder::spawn(&format!("{}-downlink", self.session), "Call recording (downlink)", rate, downlink)?);
        // Wait (briefly) for the nodes, then link at once.
        for _ in 0..25 {
            if let Ok(g) = Graph::dump() {
                let names = [format!("{}-uplink", self.session), format!("{}-downlink", self.session)];
                if names.iter().all(|n| g.node_by_name(n).is_some_and(|node| !g.input_ports(node.id).is_empty())) {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(80));
        }
        for l in self.maintain() {
            eprintln!("{l}");
        }
        Ok(())
    }

    fn maintain(&mut self) -> Vec<String> {
        let mut log = Vec::new();
        for (rec, which) in [(&mut self.up, "uplink"), (&mut self.down, "downlink")] {
            if let Some(r) = rec.as_mut() {
                if !r.alive() && self.warned.insert(format!("dead-{which}")) {
                    log.push(format!("{which} recorder (pw-record) exited"));
                }
            }
        }
        let g = match Graph::dump() {
            Ok(g) => g,
            Err(e) => {
                if self.warned.insert(e.clone()) {
                    log.push(e);
                }
                return log;
            }
        };
        let (up, down) = desired_links(self.kind == PwKind::Volte, &g);
        if self.kind == PwKind::Volte && down.is_empty() && self.warned.insert("no-imsd-play".into()) {
            log.push("waiting for imsd's playback stream (media.role Phone)".into());
        }
        if self.kind == PwKind::Volte && up.is_empty() && self.warned.insert("no-imsd-cap".into()) {
            log.push("waiting for imsd's capture stream to be linked to a source".into());
        }
        if self.kind == PwKind::Fallback && up.is_empty() && self.warned.insert("no-source".into()) {
            log.push(format!("no default source to record (default.audio.source = {:?})", g.default_source));
        }
        if self.kind == PwKind::Fallback && down.is_empty() && self.warned.insert("no-sink".into()) {
            log.push(format!("no default sink monitor to record (default.audio.sink = {:?})", g.default_sink));
        }
        if self.native.is_none() {
            self.native = g.phone_streams().0.first().and_then(|n| n.rate);
        }
        if let Some(r) = &self.up {
            Self::ensure(&g, &r.node_name, &up, &mut log, &mut self.up_devices);
        }
        if let Some(r) = &self.down {
            Self::ensure(&g, &r.node_name, &down, &mut log, &mut self.down_devices);
        }
        if let Some(p) = self.announce_player.as_mut() {
            if !matches!(p.try_wait(), Ok(None)) {
                self.announce_player = None;
            }
        }
        log
    }

    fn announce(&mut self, wav: &Path) -> AnnounceResult {
        let name = format!("{}-announce", self.session);
        let props = format!("{{ node.name = \"{name}\" node.description = \"Call recording announcement\" application.name = \"Call recording\" node.autoconnect = false }}");
        let mut res = AnnounceResult::default();
        let child = Command::new("pw-play")
            .args(["--target", "0", "-P", &props])
            .arg(wav)
            .env("APP_ID", "fp6-callrec")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        let child = match child {
            Ok(c) => c,
            Err(e) => {
                res.error = Some(format!("pw-play: {e}"));
                return res;
            }
        };
        self.announce_player = Some(child);
        let mut graph = None;
        for _ in 0..25 {
            if let Ok(g) = Graph::dump() {
                if g.node_by_name(&name).is_some_and(|n| g.tap_port(n.id, false).is_some()) {
                    graph = Some(g);
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(40));
        }
        let Some(g) = graph else {
            res.error = Some("announcement stream did not appear".into());
            return res;
        };
        let out = g.node_by_name(&name).and_then(|n| g.tap_port(n.id, false)).unwrap();
        let mut targets: Vec<(u32, &str)> = Vec::new();
        if self.kind == PwKind::Volte {
            if let Some(c) = g.phone_streams().1.first() {
                targets.extend(g.input_ports(c.id).into_iter().map(|p| (p, "uplink")));
            }
        }
        if let Some(r) = &self.up {
            if let Some(n) = g.node_by_name(&r.node_name) {
                targets.extend(g.input_ports(n.id).into_iter().map(|p| (p, "recording")));
            }
        }
        if let Some(s) = g.default_sink_node() {
            targets.extend(g.input_ports(s.id).into_iter().take(2).map(|p| (p, "local")));
        }
        for (inp, what) in targets {
            match pwgraph::link(out, inp) {
                Ok(()) => match what {
                    "uplink" => res.uplink = true,
                    "local" => res.local = true,
                    _ => {}
                },
                Err(e) => res.error = Some(e),
            }
        }
        if self.kind != PwKind::Volte && res.error.is_none() {
            res.error = Some("this call path has no uplink injection; the tone was only played locally".into());
        }
        res
    }

    fn devices(&self) -> Value {
        json!({
            "uplink": self.up_devices.iter().collect::<Vec<_>>(),
            "downlink": self.down_devices.iter().collect::<Vec<_>>(),
            "recorder_nodes": [format!("{}-uplink", self.session), format!("{}-downlink", self.session)],
        })
    }

    fn stop(&mut self) {
        if let Some(mut p) = self.announce_player.take() {
            let _ = p.kill();
            let _ = p.wait();
        }
        if let Some(mut r) = self.up.take() {
            r.stop();
        }
        if let Some(mut r) = self.down.take() {
            r.stop();
        }
    }
}

// ---------------------------------------------------------------- CS / ALSA

/// /etc/fp6-callrec/cs-incall-record.conf (`key = value`, `#` comments):
///   enable = true
///   card = F6                        (ALSA card id)
///   pcm = MultiMedia3                (record FE, matched as a prefix of the
///                                     /proc/asound/pcm id at a word boundary)
///   channels = 2                     (2: one stereo stream, see below)
///   rate = 48000
///   uplink_channel = 0               (channel index of the uplink)
///   downlink_channel = 1             (channel index of the downlink; or
///   downlink_pcm = MultiMedia4        a second mono FE for the downlink)
///   control = 'MultiMedia3 Mixer VOICEMMODE1' 1     (set before opening)
///   off_control = 'MultiMedia3 Mixer VOICEMMODE1' 0 (set after closing)
#[derive(Clone, Debug, PartialEq)]
pub struct CsConfig {
    pub enable: bool,
    pub card: String,
    pub pcm: String,
    pub channels: u32,
    pub rate: u32,
    pub uplink_channel: usize,
    pub downlink_channel: Option<usize>,
    pub downlink_pcm: Option<String>,
    pub on_controls: Vec<(String, i64)>,
    pub off_controls: Vec<(String, i64)>,
}

pub const CS_CONFIG: &str = "/etc/fp6-callrec/cs-incall-record.conf";

impl Default for CsConfig {
    fn default() -> Self {
        CsConfig {
            enable: false,
            card: "F6".into(),
            pcm: String::new(),
            channels: 2,
            rate: 48000,
            uplink_channel: 0,
            downlink_channel: Some(1),
            downlink_pcm: None,
            on_controls: Vec::new(),
            off_controls: Vec::new(),
        }
    }
}

fn parse_control(v: &str) -> Result<(String, i64), String> {
    let v = v.trim();
    let rest = v.strip_prefix('\'').ok_or_else(|| format!("control needs 'Name' value: {v}"))?;
    let (name, val) = rest.split_once('\'').ok_or_else(|| format!("unterminated control name: {v}"))?;
    let val = val.trim();
    let n = match val {
        "on" | "true" => 1,
        "off" | "false" => 0,
        _ => val.parse().map_err(|_| format!("bad control value '{val}'"))?,
    };
    Ok((name.to_owned(), n))
}

impl CsConfig {
    pub fn parse(text: &str) -> Result<CsConfig, String> {
        let mut c = CsConfig::default();
        for (i, line) in text.lines().enumerate() {
            let line = line.split_once('#').map_or(line, |(a, _)| a).trim();
            if line.is_empty() {
                continue;
            }
            let (k, v) = line.split_once('=').ok_or_else(|| format!("line {}: expected key = value", i + 1))?;
            let (k, v) = (k.trim(), v.trim());
            let num = |v: &str| v.parse::<u32>().map_err(|_| format!("line {}: bad number '{v}'", i + 1));
            match k {
                "enable" => c.enable = v == "true" || v == "1" || v == "yes",
                "card" => c.card = v.into(),
                "pcm" => c.pcm = v.into(),
                "channels" => c.channels = num(v)?.clamp(1, 8),
                "rate" => c.rate = num(v)?,
                "uplink_channel" => c.uplink_channel = num(v)? as usize,
                "downlink_channel" => c.downlink_channel = if v == "none" { None } else { Some(num(v)? as usize) },
                "downlink_pcm" => c.downlink_pcm = Some(v.into()).filter(|s: &String| !s.is_empty()),
                "control" | "on_control" => c.on_controls.push(parse_control(v).map_err(|e| format!("line {}: {e}", i + 1))?),
                "off_control" => c.off_controls.push(parse_control(v).map_err(|e| format!("line {}: {e}", i + 1))?),
                _ => return Err(format!("line {}: unknown key '{k}'", i + 1)),
            }
        }
        if c.uplink_channel >= c.channels as usize || c.downlink_channel.is_some_and(|d| d >= c.channels as usize) {
            return Err("channel index out of range".into());
        }
        Ok(c)
    }

    pub fn load() -> Option<CsConfig> {
        let path = std::env::var("FP6_CALLREC_CS_CONFIG").unwrap_or_else(|_| CS_CONFIG.into());
        let text = std::fs::read_to_string(&path).ok()?;
        match CsConfig::parse(&text) {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("{path}: {e}");
                None
            }
        }
    }

    fn device(&self, pcm: &str) -> Result<String, String> {
        use crate::alsautil::{find_pcm, parse_proc_cards, parse_proc_pcm};
        let cards = std::fs::read_to_string("/proc/asound/cards").map_err(|e| format!("/proc/asound/cards: {e}"))?;
        let card = parse_proc_cards(&cards, &self.card).ok_or_else(|| format!("card {} not present", self.card))?;
        let text = std::fs::read_to_string("/proc/asound/pcm").map_err(|e| format!("/proc/asound/pcm: {e}"))?;
        let entries = parse_proc_pcm(&text);
        let e = find_pcm(&entries, card, pcm, true).ok_or_else(|| format!("no capture PCM '{pcm}' on card {}", self.card))?;
        Ok(format!("hw:{},{}", e.card, e.device))
    }

    /// Enabled and its front end(s) present.
    pub fn available(&self) -> Result<(), String> {
        if !self.enable || self.pcm.is_empty() {
            return Err("CS in-call record is not configured".into());
        }
        self.device(&self.pcm)?;
        if let Some(d) = &self.downlink_pcm {
            self.device(d)?;
        }
        Ok(())
    }
}

pub struct AlsaTap {
    cfg: CsConfig,
    stop: Arc<AtomicBool>,
    readers: Vec<JoinHandle<Result<(), String>>>,
    devices: Vec<String>,
}

impl AlsaTap {
    pub fn new(cfg: CsConfig) -> AlsaTap {
        AlsaTap { cfg, stop: Arc::new(AtomicBool::new(false)), readers: Vec::new(), devices: Vec::new() }
    }

    fn open(dev: &str, channels: u32, rate: u32) -> Result<alsa::pcm::PCM, String> {
        use alsa::pcm::{Access, Format, HwParams, PCM};
        let pcm = PCM::new(dev, alsa::Direction::Capture, false).map_err(|e| format!("open {dev}: {e}"))?;
        {
            let hwp = HwParams::any(&pcm).map_err(|e| e.to_string())?;
            hwp.set_access(Access::RWInterleaved).map_err(|e| format!("access: {e}"))?;
            hwp.set_format(Format::s16()).map_err(|e| format!("format: {e}"))?;
            hwp.set_channels(channels).map_err(|e| format!("channels {channels}: {e}"))?;
            hwp.set_rate(rate, alsa::ValueOr::Nearest).map_err(|e| format!("rate {rate}: {e}"))?;
            pcm.hw_params(&hwp).map_err(|e| format!("hw_params {dev}: {e}"))?;
        }
        pcm.prepare().map_err(|e| format!("prepare {dev}: {e}"))?;
        Ok(pcm)
    }

    /// Reader: interleaved frames -> one or two tracks.
    fn read_loop(pcm: alsa::pcm::PCM, channels: usize, outs: Vec<(usize, Sender<Msg>)>, stop: Arc<AtomicBool>) -> Result<(), String> {
        let io = pcm.io_i16().map_err(|e| e.to_string())?;
        pcm.start().map_err(|e| format!("start: {e}"))?;
        let mut buf = vec![0i16; 960 * channels];
        while !stop.load(Ordering::SeqCst) {
            match io.readi(&mut buf) {
                Ok(frames) => {
                    let now = Instant::now();
                    for (ch, tx) in &outs {
                        let mono: Vec<i16> = buf[..frames * channels].chunks_exact(channels).map(|f| f[*ch]).collect();
                        let _ = tx.send(Msg::Pcm(now, mono));
                    }
                }
                Err(e) => {
                    // Overrun etc.: recover once, the timeline pads the gap.
                    pcm.try_recover(e, true).map_err(|e| format!("read: {e}"))?;
                }
            }
        }
        let _ = pcm.drop();
        Ok(())
    }
}

impl CallTap for AlsaTap {
    fn path(&self) -> CallPath {
        CallPath::Cs
    }

    fn native_rate(&self) -> Option<u32> {
        Some(self.cfg.rate)
    }

    fn start(&mut self, _rate: u32, uplink: Sender<Msg>, downlink: Sender<Msg>) -> Result<(), String> {
        // The PCM runs at the configured rate; the session uses native_rate().
        for (n, v) in &self.cfg.on_controls {
            crate::alsautil::set_control(&self.cfg.card, n, *v)?;
        }
        let main = self.cfg.device(&self.cfg.pcm)?;
        let ch = self.cfg.channels as usize;
        let mut outs = vec![(self.cfg.uplink_channel, uplink)];
        if self.cfg.downlink_pcm.is_none() {
            if let Some(d) = self.cfg.downlink_channel {
                outs.push((d, downlink.clone()));
            }
        }
        let pcm = Self::open(&main, self.cfg.channels, self.cfg.rate)?;
        self.devices.push(format!("{main} ({})", self.cfg.pcm));
        let stop = self.stop.clone();
        self.readers.push(std::thread::spawn(move || Self::read_loop(pcm, ch, outs, stop)));
        if let Some(dl) = self.cfg.downlink_pcm.clone() {
            let dev = self.cfg.device(&dl)?;
            let pcm = Self::open(&dev, 1, self.cfg.rate)?;
            self.devices.push(format!("{dev} ({dl})"));
            let stop = self.stop.clone();
            self.readers.push(std::thread::spawn(move || Self::read_loop(pcm, 1, vec![(0, downlink)], stop)));
        }
        Ok(())
    }

    fn maintain(&mut self) -> Vec<String> {
        let mut log = Vec::new();
        let (done, running): (Vec<_>, Vec<_>) = std::mem::take(&mut self.readers).into_iter().partition(|h| h.is_finished());
        self.readers = running;
        for h in done {
            if let Ok(Err(e)) = h.join() {
                log.push(format!("CS in-call record: {e}"));
            }
        }
        log
    }

    fn announce(&mut self, wav: &Path) -> AnnounceResult {
        // Uplink injection on CS needs the in-call music graph (staged in
        // cs-incall-record/, not usable yet). Play it locally so the user
        // knows recording started.
        let local = Command::new("pw-play").arg(wav).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().is_ok();
        AnnounceResult { uplink: false, local, error: Some("CS calls have no uplink injection yet (in-call music graph); played locally".into()) }
    }

    fn devices(&self) -> Value {
        json!({ "uplink": self.devices, "downlink": self.devices, "config": CS_CONFIG })
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        for h in self.readers.drain(..) {
            let _ = h.join();
        }
        for (n, v) in &self.cfg.off_controls {
            if let Err(e) = crate::alsautil::set_control(&self.cfg.card, n, *v) {
                eprintln!("off control '{n}': {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pwgraph::tests::fixture;

    #[test]
    fn volte_links() {
        let g = Graph::from_value(&fixture());
        let (up, down) = desired_links(true, &g);
        assert_eq!(up.iter().map(|x| x.0).collect::<Vec<_>>(), vec![501]);
        assert_eq!(down.iter().map(|x| x.0).collect::<Vec<_>>(), vec![601]);
        assert!(down[0].1.contains("pw-play pid 777"), "{}", down[0].1);
    }

    #[test]
    fn fallback_links() {
        let g = Graph::from_value(&fixture());
        let (up, down) = desired_links(false, &g);
        assert_eq!(up.iter().map(|x| x.0).collect::<Vec<_>>(), vec![501]);
        assert_eq!(down.iter().map(|x| x.0).collect::<Vec<_>>(), vec![403]);
        assert!(down[0].1.starts_with("monitor of"));
    }

    #[test]
    fn cs_config() {
        let c = CsConfig::parse(
            "# test\nenable = true\npcm = MultiMedia3\nchannels = 2\nrate = 48000\nuplink_channel = 0\ndownlink_channel = 1\n\
             control = 'MultiMedia3 Mixer VOICEMMODE1' 1\noff_control = 'MultiMedia3 Mixer VOICEMMODE1' off\n",
        )
        .unwrap();
        assert!(c.enable);
        assert_eq!(c.on_controls, vec![("MultiMedia3 Mixer VOICEMMODE1".to_string(), 1)]);
        assert_eq!(c.off_controls[0].1, 0);
        assert!(CsConfig::parse("channels = 1\ndownlink_channel = 1\n").is_err());
        assert!(CsConfig::parse("bogus = 1\n").is_err());
        assert!(CsConfig::parse("control = Name 1\n").is_err());
        let m = CsConfig::parse("enable = yes\npcm = A\nchannels = 1\ndownlink_channel = none\ndownlink_pcm = B\n").unwrap();
        assert_eq!((m.downlink_channel, m.downlink_pcm.as_deref()), (None, Some("B")));
        assert!(CsConfig::default().available().is_err());
    }
}
