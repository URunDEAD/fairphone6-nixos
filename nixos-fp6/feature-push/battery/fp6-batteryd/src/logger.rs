//! Battery history, charging sessions and usage accounting.
//!
//! Storage (state directory, default /var/lib/fp6-battery), plain CSV so it
//! stays inspectable; each line is appended once (one ~40 byte write per
//! sample), files are compacted when they hold twice what is kept:
//!   samples.csv   t,capacity,current_mA,voltage_mV,temp_dC,flags   (8 days)
//!   sessions.csv  start,end,start%,end%,max_mW,added_mAh,limit,charger (60)
//!   usage.csv     hour,A,name,kind,cpu_ms,wakeups | hour,R,name,seconds |
//!                 hour,D,condition,uAh,seconds                     (8 days)
//! Usage per app: cgroup v2 cpu.stat usage_usec deltas per leaf cgroup
//! (lomiri-app-launch units = apps, user services, system services,
//! Waydroid), "wakeups" = sum over the cgroup's threads of
//! voluntary_ctxt_switches deltas (each one is a sleep + wakeup).
//! Kernel and unattributed = /proc/stat busy time minus the cgroups.
//! Consumers: screen on time (display state from powerd signals), GPS (the
//! GNSS unit's cgroup has processes), cellular (oFono modem Online), Wi-Fi
//! (wlan0 up), Bluetooth (rfkill unblocked). Drain is measured, not
//! modelled: charge-counter deltas on battery, split by condition
//! (screen on/off, GPS on), so the page can show mA per condition.
//! Nothing here assumes suspend: intervals are wall-clock deltas between
//! samples, and a gap (e.g. a future suspend) is simply a longer interval.

use std::collections::{HashMap, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::json::{array, esc, Obj};

pub const KEEP_S: i64 = 8 * 86400;
pub const KEEP_SESSIONS: usize = 60;

#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub t: i64,
    pub cap: i32,
    pub ma: i32,
    pub mv: i32,
    pub temp: i32,
    pub plugged: bool,
    pub screen: bool,
}

impl Sample {
    fn line(&self) -> String {
        let flags = (self.plugged as u8) | ((self.screen as u8) << 1);
        format!("{},{},{},{},{},{}\n", self.t, self.cap, self.ma, self.mv, self.temp, flags)
    }
    fn parse(l: &str) -> Option<Sample> {
        let v: Vec<i64> = l.split(',').map(|s| s.trim().parse().ok()).collect::<Option<_>>()?;
        if v.len() != 6 {
            return None;
        }
        Some(Sample { t: v[0], cap: v[1] as i32, ma: v[2] as i32, mv: v[3] as i32, temp: v[4] as i32,
                      plugged: v[5] & 1 != 0, screen: v[5] & 2 != 0 })
    }
}

fn append(path: &Path, line: &str) {
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
}

fn rewrite(path: &Path, lines: impl Iterator<Item = String>) {
    let tmp = path.with_extension("tmp");
    let body: String = lines.collect();
    if fs::write(&tmp, body).is_ok() {
        let _ = fs::rename(&tmp, path);
    }
}

// ----------------------------------------------------------------- history
pub struct History {
    path: PathBuf,
    pub samples: VecDeque<Sample>,
    lines_in_file: usize,
}

impl History {
    pub fn load(dir: &Path, now: i64) -> History {
        let path = dir.join("samples.csv");
        let text = fs::read_to_string(&path).unwrap_or_default();
        let lines_in_file = text.lines().count();
        let mut samples: VecDeque<Sample> = text.lines().filter_map(Sample::parse)
            .filter(|s| s.t > now - KEEP_S && s.t <= now + 3600).collect();
        samples.make_contiguous().sort_by_key(|s| s.t);
        let mut h = History { path, samples, lines_in_file };
        h.compact_if_needed(true);
        h
    }

    pub fn push(&mut self, s: Sample) {
        if let Some(last) = self.samples.back() {
            if s.t <= last.t {
                return;
            }
        }
        append(&self.path, &s.line());
        self.lines_in_file += 1;
        let cutoff = s.t - KEEP_S;
        self.samples.push_back(s);
        while self.samples.front().is_some_and(|x| x.t < cutoff) {
            self.samples.pop_front();
        }
        self.compact_if_needed(false);
    }

    fn compact_if_needed(&mut self, force: bool) {
        if force && self.lines_in_file == self.samples.len() {
            return;
        }
        if force || self.lines_in_file > 2 * self.samples.len().max(64) {
            rewrite(&self.path, self.samples.iter().map(Sample::line));
            self.lines_in_file = self.samples.len();
        }
    }

    pub fn last(&self) -> Option<&Sample> {
        self.samples.back()
    }

    /// [[t, capacity, plugged], ...] over `span` seconds, at most one point per `step`.
    pub fn points(&self, now: i64, span: i64, step: i64) -> String {
        let mut out = Vec::new();
        let mut last_t: Option<i64> = None;
        let mut last_plug = None;
        for s in self.samples.iter().filter(|s| s.t >= now - span) {
            // Keep plug transitions exact so sessions show on the graph.
            if last_t.is_none_or(|l| s.t - l >= step) || last_plug != Some(s.plugged) {
                out.push(format!("[{},{},{}]", s.t, s.cap, s.plugged as u8));
                last_t = Some(s.t);
                last_plug = Some(s.plugged);
            }
        }
        array(out)
    }
}

// ---------------------------------------------------------------- sessions
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChargeSession {
    pub start: i64,
    pub end: i64,
    pub start_cap: i32,
    pub end_cap: i32,
    pub max_mw: i64,
    pub added_uah: i64,
    pub limit: u32,
    pub charger: String,
    last_counter: i64,
}

impl ChargeSession {
    fn line(&self) -> String {
        format!("{},{},{},{},{},{},{},{}\n", self.start, self.end, self.start_cap, self.end_cap, self.max_mw,
                self.added_uah / 1000, self.limit, self.charger.replace([',', '\n'], " "))
    }
    fn parse(l: &str) -> Option<ChargeSession> {
        let p: Vec<&str> = l.splitn(8, ',').collect();
        if p.len() != 8 {
            return None;
        }
        Some(ChargeSession {
            start: p[0].parse().ok()?, end: p[1].parse().ok()?, start_cap: p[2].parse().ok()?,
            end_cap: p[3].parse().ok()?, max_mw: p[4].parse().ok()?, added_uah: p[5].parse::<i64>().ok()? * 1000,
            limit: p[6].parse().ok()?, charger: p[7].trim().to_string(), last_counter: 0,
        })
    }
    pub fn json(&self) -> String {
        Obj::new().i("start", self.start).i("end", self.end).i("startLevel", self.start_cap as i64)
            .i("endLevel", self.end_cap as i64).i("maxMilliwatts", self.max_mw).i("addedMah", self.added_uah / 1000)
            .i("limit", self.limit as i64).s("charger", &self.charger).finish()
    }
}

pub struct Sessions {
    path: PathBuf,
    pub done: VecDeque<ChargeSession>,
    pub current: Option<ChargeSession>,
}

impl Sessions {
    pub fn load(dir: &Path) -> Sessions {
        let path = dir.join("sessions.csv");
        let text = fs::read_to_string(&path).unwrap_or_default();
        let mut done: VecDeque<ChargeSession> = text.lines().filter_map(ChargeSession::parse).collect();
        let n = done.len();
        while done.len() > KEEP_SESSIONS {
            done.pop_front();
        }
        let s = Sessions { path, done, current: None };
        if n > KEEP_SESSIONS * 2 {
            rewrite(&s.path, s.done.iter().map(ChargeSession::line));
        }
        s
    }

    /// Feed every reading. `charger` empty = unplugged.
    pub fn update(&mut self, t: i64, plugged: bool, cap: i32, counter_uah: i64, charge_mw: i64, limit: u32, charger: &str) {
        match (&mut self.current, plugged) {
            (None, true) => {
                self.current = Some(ChargeSession {
                    start: t, end: t, start_cap: cap, end_cap: cap, max_mw: charge_mw, added_uah: 0,
                    limit, charger: charger.to_string(), last_counter: counter_uah,
                });
            }
            (Some(c), true) => {
                c.end = t;
                c.end_cap = cap;
                c.max_mw = c.max_mw.max(charge_mw);
                c.limit = limit;
                if !charger.is_empty() {
                    c.charger = charger.to_string();
                }
                let d = counter_uah - c.last_counter;
                if d > 0 && d < 2_000_000 {
                    c.added_uah += d;
                }
                c.last_counter = counter_uah;
            }
            (Some(_), false) => {
                let mut c = self.current.take().unwrap();
                c.end = t;
                // Ignore blips (cable wiggle) shorter than a minute.
                if c.end - c.start >= 60 {
                    let l = c.line();
                    self.done.push_back(c);
                    while self.done.len() > KEEP_SESSIONS {
                        self.done.pop_front();
                    }
                    append(&self.path, &l);
                }
            }
            (None, false) => {}
        }
    }

    /// Mean charge current (µA) of recent sessions between 20 % and 80 %,
    /// for the adaptive-charging lead time. 0 if unknown.
    pub fn typical_rate_ua(&self, full_uah: i64) -> i64 {
        let mut rates: Vec<i64> = self.done.iter().rev().take(10).filter_map(|s| {
            let dt = s.end - s.start;
            let dcap = s.end_cap - s.start_cap;
            (dt > 600 && dcap >= 10 && s.start_cap < 80).then(|| dcap as i64 * full_uah / 100 * 3600 / dt)
        }).collect();
        if rates.is_empty() {
            return 0;
        }
        rates.sort_unstable();
        rates[rates.len() / 2]
    }

    pub fn json(&self, n: usize) -> String {
        let mut v: Vec<String> = self.done.iter().rev().take(n).map(ChargeSession::json).collect();
        if let Some(c) = &self.current {
            v.insert(0, Obj::new().raw("session", &c.json()).b("ongoing", true).finish());
        }
        array(v)
    }
}

// ------------------------------------------------------------------- usage
#[derive(Clone, Debug, Default)]
pub struct Bucket {
    pub hour: i64,
    /// name -> (kind, cpu ms, wakeups)
    pub apps: HashMap<String, (String, u64, u64)>,
    /// resource -> seconds on
    pub res: HashMap<String, u64>,
    /// condition -> (µAh drained, seconds on battery)
    pub drain: HashMap<String, (i64, i64)>,
}

impl Bucket {
    fn lines(&self) -> Vec<String> {
        let mut v = Vec::new();
        for (n, (k, c, w)) in &self.apps {
            v.push(format!("{},A,{},{},{},{}\n", self.hour, n.replace([',', '\n'], "_"), k, c, w));
        }
        for (n, s) in &self.res {
            v.push(format!("{},R,{},{}\n", self.hour, n, s));
        }
        for (n, (u, s)) in &self.drain {
            v.push(format!("{},D,{},{},{}\n", self.hour, n, u, s));
        }
        v
    }
}

#[derive(Clone, Debug, Default)]
struct CgroupSnap {
    usage_us: u64,
}

pub struct Usage {
    path: PathBuf,
    pub buckets: VecDeque<Bucket>,
    lines_in_file: usize,
    cg: HashMap<String, CgroupSnap>,
    tids: HashMap<u32, u64>,
    total_busy_ms: Option<u64>,
    baseline: bool,
    pub resources: Vec<(String, bool)>,
    last_t: Option<i64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Leaf {
    pub name: String,
    pub kind: String,
    pub usage_us: u64,
    pub dir: PathBuf,
}

/// Display name and kind for a leaf cgroup path relative to the cgroup root.
pub fn classify(rel: &str) -> (String, String) {
    let leaf = rel.rsplit('/').next().unwrap_or(rel);
    if rel.contains("lxc.payload") || rel.contains("waydroid") {
        return ("Waydroid (Android)".into(), "app".into());
    }
    for pfx in ["lomiri-app-launch--application-legacy--", "lomiri-app-launch--application-click--",
                "lomiri-app-launch--application-snap--"] {
        if let Some(rest) = leaf.strip_prefix(pfx) {
            let id = rest.split("--").next().unwrap_or(rest);
            return (id.to_string(), "app".into());
        }
    }
    if rel.contains("/user@") {
        if let Some(rest) = leaf.strip_prefix("dbus-:1.2-").or_else(|| leaf.strip_prefix("dbus-:1.1-")) {
            let name = rest.split('@').next().unwrap_or(rest);
            return (name.to_string(), "user-service".into());
        }
        if let Some(rest) = leaf.strip_prefix("app-") {
            if let Some(scope) = rest.strip_suffix(".scope") {
                let name = scope.rsplit_once('-').map(|(a, b)| if b.chars().all(|c| c.is_ascii_hexdigit()) { a } else { scope }).unwrap_or(scope);
                return (name.replace("\\x2d", "-"), "app".into());
            }
        }
        let name = leaf.strip_suffix(".service").or_else(|| leaf.strip_suffix(".scope")).unwrap_or(leaf);
        return (name.to_string(), "user-service".into());
    }
    if leaf.starts_with("session-") && leaf.ends_with(".scope") {
        return ("Login sessions (SSH, greeter)".into(), "session".into());
    }
    let name = leaf.strip_suffix(".service").or_else(|| leaf.strip_suffix(".scope"))
        .or_else(|| leaf.strip_suffix(".mount")).unwrap_or(leaf);
    (name.to_string(), "system".into())
}

fn cpu_usage_us(dir: &Path) -> Option<u64> {
    let text = fs::read_to_string(dir.join("cpu.stat")).ok()?;
    text.lines().find_map(|l| l.strip_prefix("usage_usec ")).and_then(|v| v.trim().parse().ok())
}

/// Leaf cgroups (no child cgroup directories) with their CPU usage.
pub fn leaves(root: &Path) -> Vec<Leaf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        let mut has_child = false;
        for e in rd.flatten() {
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                has_child = true;
                stack.push(e.path());
            }
        }
        if !has_child && dir != root {
            let rel = dir.strip_prefix(root).unwrap_or(&dir).to_string_lossy().to_string();
            if let Some(u) = cpu_usage_us(&dir) {
                let (name, kind) = classify(&rel);
                out.push(Leaf { name, kind, usage_us: u, dir });
            }
        }
    }
    out
}

fn threads(dir: &Path) -> Vec<u32> {
    fs::read_to_string(dir.join("cgroup.threads")).unwrap_or_default().lines().filter_map(|l| l.trim().parse().ok()).collect()
}

/// Wakeups of a thread: voluntary context switches (it slept and was woken).
/// /proc/<tid>/schedstat would be cheaper but needs CONFIG_SCHED_INFO, which
/// the FP6 kernels do not have.
fn pcount(tid: u32) -> Option<u64> {
    let s = fs::read_to_string(format!("/proc/{tid}/status")).ok()?;
    voluntary_switches(&s)
}

pub fn voluntary_switches(status: &str) -> Option<u64> {
    status.lines().find_map(|l| l.strip_prefix("voluntary_ctxt_switches:")).and_then(|v| v.trim().parse().ok())
}

/// Busy CPU time (ms) from /proc/stat: everything except idle and iowait.
pub fn busy_ms(stat: &str) -> Option<u64> {
    let l = stat.lines().next()?;
    let v: Vec<u64> = l.split_whitespace().skip(1).filter_map(|x| x.parse().ok()).collect();
    if v.len() < 7 {
        return None;
    }
    let ticks: u64 = v.iter().take(8).enumerate().filter(|(i, _)| *i != 3 && *i != 4).map(|(_, x)| *x).sum();
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) }.max(1) as u64;
    Some(ticks * 1000 / hz)
}

impl Usage {
    pub fn load(dir: &Path, now: i64) -> Usage {
        let path = dir.join("usage.csv");
        let text = fs::read_to_string(&path).unwrap_or_default();
        let lines_in_file = text.lines().count();
        let mut map: HashMap<i64, Bucket> = HashMap::new();
        for l in text.lines() {
            let p: Vec<&str> = l.split(',').collect();
            let Some(hour) = p.first().and_then(|h| h.parse::<i64>().ok()) else { continue };
            if hour < now - KEEP_S {
                continue;
            }
            let b = map.entry(hour).or_insert_with(|| Bucket { hour, ..Default::default() });
            match (p.get(1).copied(), p.len()) {
                (Some("A"), 6) => {
                    let e = b.apps.entry(p[2].to_string()).or_insert((p[3].to_string(), 0, 0));
                    e.1 += p[4].parse::<u64>().unwrap_or(0);
                    e.2 += p[5].parse::<u64>().unwrap_or(0);
                }
                (Some("R"), 4) => *b.res.entry(p[2].to_string()).or_insert(0) += p[3].parse::<u64>().unwrap_or(0),
                (Some("D"), 5) => {
                    let e = b.drain.entry(p[2].to_string()).or_insert((0, 0));
                    e.0 += p[3].parse::<i64>().unwrap_or(0);
                    e.1 += p[4].parse::<i64>().unwrap_or(0);
                }
                _ => {}
            }
        }
        let mut buckets: Vec<Bucket> = map.into_values().collect();
        buckets.sort_by_key(|b| b.hour);
        let mut u = Usage { path, buckets: buckets.into(), lines_in_file, cg: HashMap::new(), tids: HashMap::new(),
                            total_busy_ms: None, baseline: true, resources: Vec::new(), last_t: None };
        // The current hour continues in memory; drop its partial lines from disk on the next flush.
        u.compact();
        u
    }

    fn bucket(&mut self, t: i64) -> &mut Bucket {
        let hour = t - t.rem_euclid(3600);
        if self.buckets.back().is_none_or(|b| b.hour != hour) {
            self.flush_closed();
            self.buckets.push_back(Bucket { hour, ..Default::default() });
            while self.buckets.front().is_some_and(|b| b.hour < t - KEEP_S) {
                self.buckets.pop_front();
            }
        }
        self.buckets.back_mut().unwrap()
    }

    /// Appends the last (now closed) bucket to disk.
    fn flush_closed(&mut self) {
        if let Some(b) = self.buckets.back() {
            let lines = b.lines();
            self.lines_in_file += lines.len();
            for l in lines {
                append(&self.path, &l);
            }
        }
        let kept: usize = self.buckets.iter().map(|b| b.lines().len()).sum();
        if self.lines_in_file > 2 * kept.max(256) {
            self.compact();
        }
    }

    fn compact(&mut self) {
        let n = self.buckets.len();
        let lines: Vec<String> = self.buckets.iter().take(n.saturating_sub(1)).flat_map(Bucket::lines).collect();
        self.lines_in_file = lines.len();
        rewrite(&self.path, lines.into_iter());
    }

    /// At shutdown: write the current partial hour too (merged on reload).
    pub fn flush_all(&mut self) {
        let lines: Vec<String> = self.buckets.iter().flat_map(Bucket::lines).collect();
        self.lines_in_file = lines.len();
        rewrite(&self.path, lines.into_iter());
    }

    /// One accounting step: CPU/wakeups per cgroup leaf, resource on-times
    /// and battery drain since the previous call.
    pub fn sample(&mut self, t: i64, cg_root: &Path, stat: &str, screen_on_s: i64, dt_drain: Option<(i64, i64)>, screen_frac: f64) {
        let dt = self.last_t.map(|l| (t - l).max(0)).unwrap_or(0);
        self.last_t = Some(t);
        let leaves = leaves(cg_root);
        let mut new_tids = HashMap::with_capacity(self.tids.len());
        let mut apps: HashMap<String, (String, u64, u64)> = HashMap::new();
        let mut attributed_ms = 0u64;
        let mut seen = HashMap::with_capacity(leaves.len());
        for l in &leaves {
            let key = l.dir.to_string_lossy().to_string();
            let prev = self.cg.get(&key).map(|s| s.usage_us);
            let cpu_ms = match prev {
                Some(p) => l.usage_us.saturating_sub(p) / 1000,
                None if self.baseline => 0,
                None => l.usage_us / 1000,
            };
            let mut wake = 0u64;
            for tid in threads(&l.dir) {
                if let Some(c) = pcount(tid) {
                    match self.tids.get(&tid) {
                        Some(p) => wake += c.saturating_sub(*p),
                        None if !self.baseline => wake += c,
                        None => {}
                    }
                    new_tids.insert(tid, c);
                }
            }
            seen.insert(key, CgroupSnap { usage_us: l.usage_us });
            if cpu_ms > 0 || wake > 0 {
                let e = apps.entry(l.name.clone()).or_insert((l.kind.clone(), 0, 0));
                e.1 += cpu_ms;
                e.2 += wake;
                attributed_ms += cpu_ms;
            }
        }
        self.cg = seen;
        self.tids = new_tids;
        let busy = busy_ms(stat);
        let other = match (busy, self.total_busy_ms) {
            (Some(b), Some(p)) if !self.baseline => b.saturating_sub(p).saturating_sub(attributed_ms),
            _ => 0,
        };
        self.total_busy_ms = busy;
        let was_baseline = self.baseline;
        self.baseline = false;
        let resources = self.resources.clone();
        let b = self.bucket(t);
        if !was_baseline {
            for (n, (k, c, w)) in apps {
                let e = b.apps.entry(n).or_insert((k, 0, 0));
                e.1 += c;
                e.2 += w;
            }
            if other > 0 {
                b.apps.entry("Kernel and other".into()).or_insert(("kernel".into(), 0, 0)).1 += other;
            }
        }
        if screen_on_s > 0 {
            *b.res.entry("screen".into()).or_insert(0) += screen_on_s as u64;
        }
        if dt > 0 && dt < 3 * 3600 {
            for (name, on) in &resources {
                if *on {
                    *b.res.entry(name.clone()).or_insert(0) += dt as u64;
                }
            }
        }
        if let Some((uah, secs)) = dt_drain {
            let mut add = |k: &str, u: f64, s: f64| {
                let e = b.drain.entry(k.to_string()).or_insert((0, 0));
                e.0 += u.round() as i64;
                e.1 += s.round() as i64;
            };
            let (u, s) = (uah as f64, secs as f64);
            add("all", u, s);
            // Split by the screen-on fraction of the interval; a mixed
            // interval is rare (samples follow display changes).
            if screen_frac >= 0.5 {
                add("screen-on", u, s);
            } else {
                add("screen-off", u, s);
            }
            if resources.iter().any(|(n, on)| n == "gps" && *on) {
                add("gps-on", u, s);
            }
        }
    }

    /// {"apps":[...], "resources":{...}, "drain":{...}} over the last `span` seconds.
    pub fn json(&self, now: i64, span: i64, top: usize) -> String {
        let mut apps: HashMap<&str, (&str, u64, u64)> = HashMap::new();
        let mut res: HashMap<&str, u64> = HashMap::new();
        let mut drain: HashMap<&str, (i64, i64)> = HashMap::new();
        for b in self.buckets.iter().filter(|b| b.hour + 3600 > now - span) {
            for (n, (k, c, w)) in &b.apps {
                let e = apps.entry(n.as_str()).or_insert((k.as_str(), 0, 0));
                e.1 += c;
                e.2 += w;
            }
            for (n, s) in &b.res {
                *res.entry(n.as_str()).or_insert(0) += s;
            }
            for (n, (u, s)) in &b.drain {
                let e = drain.entry(n.as_str()).or_insert((0, 0));
                e.0 += u;
                e.1 += s;
            }
        }
        let total: u64 = apps.values().map(|v| v.1).sum();
        let mut list: Vec<(&str, (&str, u64, u64))> = apps.into_iter().collect();
        list.sort_by(|a, b| b.1 .1.cmp(&a.1 .1).then(b.1 .2.cmp(&a.1 .2)));
        let apps_json = array(list.iter().take(top).map(|(n, (k, c, w))| {
            Obj::new().s("name", n).s("kind", k).i("cpuMs", *c as i64).i("wakeups", *w as i64)
                .f("share", if total > 0 { *c as f64 / total as f64 } else { 0.0 }).finish()
        }));
        let res_json = {
            let mut o = String::from("{");
            let mut keys: Vec<_> = res.iter().collect();
            keys.sort();
            for (i, (k, v)) in keys.iter().enumerate() {
                if i > 0 {
                    o.push(',');
                }
                o.push_str(&format!("{}:{}", esc(k), v));
            }
            o.push('}');
            o
        };
        let drain_json = {
            let mut o = String::from("{");
            let mut keys: Vec<_> = drain.iter().collect();
            keys.sort();
            for (i, (k, (u, s))) in keys.iter().enumerate() {
                if i > 0 {
                    o.push(',');
                }
                let ma = if *s > 0 { *u as f64 * 3600.0 / *s as f64 / 1000.0 } else { 0.0 };
                o.push_str(&format!("{}:{}", esc(k), Obj::new().i("uah", *u).i("seconds", *s).f("mA", ma).finish()));
            }
            o.push('}');
            o
        };
        Obj::new().i("span", span).i("totalCpuMs", total as i64).raw("apps", &apps_json)
            .raw("resources", &res_json).raw("drain", &drain_json).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fp6-batt-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn classify_names() {
        assert_eq!(classify("user.slice/user-10000.slice/user@10000.service/app.slice/lomiri-app-launch--application-legacy--morph-browser--.service"),
                   ("morph-browser".into(), "app".into()));
        assert_eq!(classify("user.slice/user-10000.slice/user@10000.service/app.slice/app-dbus\\x2d:1.2\\x2dcom.lomiri.Thumbnailer.slice/dbus-:1.2-com.lomiri.Thumbnailer@1.service").0,
                   "com.lomiri.Thumbnailer");
        assert_eq!(classify("user.slice/user-10000.slice/user@10000.service/session.slice/lomiri.service"), ("lomiri".into(), "user-service".into()));
        assert_eq!(classify("system.slice/ofono.service"), ("ofono".into(), "system".into()));
        assert_eq!(classify("user.slice/user-10000.slice/session-19.scope").1, "session");
        assert_eq!(classify("lxc.payload.waydroid2").0, "Waydroid (Android)");
    }

    #[test]
    fn voluntary() {
        let s = "Name:\tx\nvoluntary_ctxt_switches:\t42\nnonvoluntary_ctxt_switches:\t7\n";
        assert_eq!(voluntary_switches(s), Some(42));
    }

    #[test]
    fn busy_from_proc_stat() {
        let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as u64;
        // user nice system idle iowait irq softirq steal
        let s = format!("cpu  {} 0 {} 999999 5555 0 0 0 0 0\ncpu0 1 2 3\n", hz, hz);
        assert_eq!(busy_ms(&s), Some(2000));
    }

    #[test]
    fn history_roundtrip_and_points() {
        let d = tmp("hist");
        let mut h = History::load(&d, 1000);
        for i in 0..10 {
            h.push(Sample { t: 1000 + i * 60, cap: 50 + i as i32, ma: -100, mv: 4000, temp: 270, plugged: i >= 5, screen: true });
        }
        h.push(Sample { t: 1000, cap: 1, ma: 0, mv: 0, temp: 0, plugged: false, screen: false }); // out of order: dropped
        let h2 = History::load(&d, 2000);
        assert_eq!(h2.samples.len(), 10);
        assert_eq!(h2.last().unwrap().cap, 59);
        let p = h2.points(2000, 3600, 300);
        // every 5 minutes plus the plug transition at i=5
        assert_eq!(p, "[[1000,50,0],[1300,55,1]]");
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn sessions_track_and_rate() {
        let d = tmp("sess");
        let mut s = Sessions::load(&d);
        s.update(0, true, 20, 1_000_000, 5000, 100, "USB PD");
        s.update(1800, true, 50, 2_400_000, 9000, 100, "USB PD");
        s.update(3600, true, 80, 3_800_000, 7000, 80, "USB PD");
        s.update(3700, false, 80, 3_800_000, 0, 80, "");
        assert_eq!(s.done.len(), 1);
        let c = &s.done[0];
        assert_eq!((c.start_cap, c.end_cap, c.max_mw, c.added_uah, c.limit), (20, 80, 9000, 2_800_000, 80));
        let s2 = Sessions::load(&d);
        assert_eq!(s2.done.len(), 1);
        // 60 % of 4.7 Ah in 3700 s = 2.74 A
        let r = s2.typical_rate_ua(4_700_000);
        assert!((2_700_000..2_800_000).contains(&r), "{r}");
        s.update(4000, true, 80, 0, 0, 80, "USB");
        s.update(4030, false, 80, 0, 0, 80, "");
        assert_eq!(s.done.len(), 1, "short blip ignored");
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn usage_accounting_on_fake_cgroups() {
        let d = tmp("usage");
        let cg = d.join("cg");
        let app = cg.join("user.slice/user-10000.slice/user@10000.service/app.slice/lomiri-app-launch--application-legacy--foo--.service");
        let sys = cg.join("system.slice/ofono.service");
        for p in [&app, &sys] {
            fs::create_dir_all(p).unwrap();
            fs::write(p.join("cgroup.threads"), "").unwrap();
        }
        let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as u64;
        let stat = |busy_s: u64| format!("cpu  {} 0 0 100 0 0 0 0\n", busy_s * hz);
        fs::write(app.join("cpu.stat"), "usage_usec 1000000\n").unwrap();
        fs::write(sys.join("cpu.stat"), "usage_usec 500000\n").unwrap();
        let mut u = Usage::load(&d, 7200);
        u.resources = vec![("gps".into(), true)];
        u.sample(7200, &cg, &stat(10), 0, None, 0.0);
        fs::write(app.join("cpu.stat"), "usage_usec 3000000\n").unwrap();
        fs::write(sys.join("cpu.stat"), "usage_usec 600000\n").unwrap();
        u.sample(7500, &cg, &stat(15), 300, Some((50_000, 300)), 1.0);
        let j = u.json(7500, 86400, 10);
        assert!(j.contains(r#"{"name":"foo","kind":"app","cpuMs":2000"#), "{j}");
        assert!(j.contains(r#""name":"ofono","kind":"system","cpuMs":100"#), "{j}");
        assert!(j.contains(r#""name":"Kernel and other","kind":"kernel","cpuMs":2900"#), "{j}");
        assert!(j.contains(r#""gps":300"#) && j.contains(r#""screen":300"#), "{j}");
        assert!(j.contains(r#""screen-on":{"uah":50000,"seconds":300,"mA":600.000}"#), "{j}");
        // Hour rollover writes the closed hour; reload merges it.
        u.sample(11000, &cg, &stat(16), 0, None, 0.0);
        let u2 = Usage::load(&d, 11000);
        assert!(u2.json(11000, 86400, 10).contains(r#""name":"foo""#));
        fs::remove_dir_all(&d).unwrap();
    }
}
