use crate::{
    config::{Settings, SystemConfig},
    nft::{read_counters, AppNet, PermScope, Rules, Scope},
    nm,
    policy::app_allowed,
    runtime::{read_json, run, save_json},
    usage::Usage,
};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub const NAME: &str = "org.fairphone.NetPolicy.Helper1";
pub const PATH: &str = "/org/fairphone/NetPolicy/Helper1";
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Mobile {
    pub available: bool,
    pub active: bool,
    pub owned: bool,
    pub interface: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub usage: Usage,
    pub mobile: Mobile,
    pub network: nm::Network,
    pub apps: Vec<String>,
    pub error: String,
    pub metrics: Option<crate::policy::Metrics>,
    pub sample_id: u64,
}
pub struct Engine {
    pub config: SystemConfig,
    pub settings: Settings,
    pub foreground: String,
    pub last_seen: Option<Instant>,
    pub snapshot: Snapshot,
    last_rules: Option<Rules>,
    last_system_inodes: Vec<u64>,
    /// App network permission (last successfully read permissions file).
    perm_apps: BTreeMap<String, AppNet>,
    state_path: PathBuf,
    bus: zbus::blocking::Connection,
}
impl Engine {
    pub fn new(config: SystemConfig, state_path: PathBuf) -> Result<Self> {
        config.validate()?;
        let usage = if state_path.exists() {
            read_json(&state_path)?
        } else {
            Usage::default()
        };
        let apps = config.apps.iter().map(|a| a.id.clone()).collect();
        Ok(Self {
            config,
            settings: Settings::default(),
            foreground: String::new(),
            last_seen: None,
            snapshot: Snapshot {
                usage,
                apps,
                ..Snapshot::default()
            },
            last_rules: None,
            last_system_inodes: vec![],
            perm_apps: BTreeMap::new(),
            state_path,
            bus: crate::runtime::system()?,
        })
    }
    fn adapter(&self, action: &str) -> Result<Mobile> {
        let Some(program) = &self.config.mobile_adapter else {
            return Ok(Mobile::default());
        };
        let m: Mobile = serde_json::from_str(&run(program, &[action], None)?)?;
        ensure!(
            !(m.active || m.available) || crate::config::valid_iface(&m.interface),
            "adapter returned invalid interface"
        );
        Ok(m)
    }
    pub fn route(&mut self, mobile: bool) -> Result<()> {
        if mobile {
            ensure!(
                self.last_seen
                    .is_some_and(|t| t.elapsed() < Duration::from_secs(30)),
                "policy lease expired"
            );
            ensure!(
                self.settings.smart_switching
                    && (!self.settings.capped_plan || self.settings.allow_capped),
                "mobile fallback disallowed"
            );
            ensure!(self.snapshot.mobile.available, "mobile adapter unavailable");
            // Install restrictions on the adapter's reserved interface before it
            // becomes a default route. This also applies the latest settings.
            self.reconcile()?;
            ensure!(
                self.snapshot
                    .network
                    .costly_interfaces
                    .contains(&self.snapshot.mobile.interface),
                "mobile interface is not protected"
            );
        }
        let result = self.adapter(if mobile {
            "prefer-mobile"
        } else {
            "prefer-wifi"
        })?;
        ensure!(
            if mobile {
                result.active && result.owned
            } else {
                !result.owned
            },
            "adapter did not confirm route transition"
        );
        self.snapshot.mobile = result;
        Ok(())
    }
    pub fn reconcile(&mut self) -> Result<()> {
        let mut observation_error = None;
        match self.adapter("status") {
            Ok(mobile) => self.snapshot.mobile = mobile,
            Err(e) => observation_error = Some(e),
        }
        if self
            .last_seen
            .is_none_or(|t| t.elapsed() >= Duration::from_secs(15))
        {
            self.foreground.clear();
        }
        if self.snapshot.mobile.owned
            && (!self.settings.smart_switching
                || (self.settings.capped_plan && !self.settings.allow_capped)
                || self
                    .last_seen
                    .is_none_or(|t| t.elapsed() >= Duration::from_secs(30)))
        {
            if let Err(e) = self.route(false) {
                observation_error = Some(e);
            }
        }
        let mut network = match nm::read(&self.bus) {
            Ok(network) => network,
            Err(e) => {
                observation_error = Some(e);
                self.snapshot.network.clone()
            }
        };
        if self.snapshot.mobile.active || self.snapshot.mobile.available {
            network
                .costly_interfaces
                .push(self.snapshot.mobile.interface.clone());
        }
        // Classify original sockets before a VPN re-encapsulates them. This is
        // conservative when a chargeable and an unmetered uplink coexist.
        if !network.costly_interfaces.is_empty() {
            network
                .costly_interfaces
                .extend(network.tunnel_interfaces.clone());
        }
        network.costly_interfaces.sort();
        network.costly_interfaces.dedup();
        let exists = run("nft", &["list", "table", "inet", "fp6_netpolicy"], None).is_ok();
        let boot_id = fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
        if !exists || self.snapshot.usage.boot_id != boot_id {
            self.snapshot.usage.last.clear();
            self.snapshot.usage.boot_id = boot_id;
        }
        let (names, raw) = if exists {
            read_counters(
                &run(
                    "nft",
                    &["-j", "list", "counters", "table", "inet", "fp6_netpolicy"],
                    None,
                )?,
                &self.snapshot.apps,
            )?
        } else {
            (Default::default(), BTreeMap::new())
        };
        self.snapshot
            .usage
            .sample(&chrono::Utc::now().format("%Y-%m").to_string(), &raw);
        save_json(&self.state_path, &self.snapshot.usage)?;
        let mut scopes = discover(&self.config, Path::new("/sys/fs/cgroup"))?;
        for scope in &mut scopes {
            scope.allowed = app_allowed(
                &self.settings,
                true,
                &scope.app,
                Some(&self.foreground),
                *self.snapshot.usage.totals.get(&scope.app).unwrap_or(&0),
            );
        }
        let mut system_allow: Vec<String> = self
            .config
            .system_allow
            .iter()
            .map(|u| format!("system.slice/{u}"))
            .filter(|p| Path::new("/sys/fs/cgroup").join(p).is_dir())
            .collect();
        // The fixed helper service runs interface-bound connectivity probes.
        let own_scope = "system.slice/fp6-netpolicy-helper.service";
        if Path::new("/sys/fs/cgroup").join(own_scope).is_dir() {
            system_allow.push(own_scope.into());
        }
        // App network permission (fp6-permd's file). A broken file keeps the
        // previous rules: a denial must not silently lapse.
        if let Some(path) = &self.config.permissions_file {
            match read_permissions(Path::new(path), self.config.uid) {
                Ok(map) => self.perm_apps = map,
                Err(e) => observation_error = Some(e.context("app network permissions")),
            }
        }
        let perm = discover_perm(
            self.config.uid,
            &self.perm_apps,
            Path::new("/sys/fs/cgroup"),
        )?;
        let mut mobile_interfaces = network.mobile_interfaces.clone();
        if self.snapshot.mobile.available || self.snapshot.mobile.active {
            mobile_interfaces.push(self.snapshot.mobile.interface.clone());
        }
        mobile_interfaces.sort();
        mobile_interfaces.dedup();
        let rules = Rules {
            uid: self.config.uid,
            interfaces: network.costly_interfaces.clone(),
            scopes,
            system_allow,
            restrict_unknown: self.settings.data_saver,
            perm,
            mobile_interfaces,
            vpn_interfaces: network.tunnel_interfaces.clone(),
        };
        let system_inodes: Vec<u64> = rules
            .system_allow
            .iter()
            .map(|p| {
                fs::metadata(Path::new("/sys/fs/cgroup").join(p))
                    .map(|m| m.ino())
                    .unwrap_or(0)
            })
            .collect();
        if !exists
            || self.last_rules.as_ref() != Some(&rules)
            || self.last_system_inodes != system_inodes
        {
            let previous = if exists {
                self.last_rules.as_ref().map_or(0, |r| r.perm_apps().len())
            } else {
                0
            };
            let script = rules.render_with(&names, exists, previous)?;
            run("nft", &["-f", "-"], Some(&script))?;
            self.last_rules = Some(rules);
            self.last_system_inodes = system_inodes;
        }
        self.snapshot.metrics = None;
        if let Some(error) = observation_error {
            return Err(error);
        }
        self.snapshot.metrics = if self.settings.smart_switching {
            let endpoint = if self.settings.endpoint.is_empty() {
                network
                    .connectivity_enabled
                    .then_some(network.connectivity_uri.as_str())
            } else {
                Some(self.settings.endpoint.as_str())
            };
            network
                .wifi
                .as_ref()
                .map(|wifi| nm::probe(wifi, endpoint))
                .transpose()?
        } else {
            None
        };
        self.snapshot.network = network;
        self.snapshot.sample_id = self.snapshot.sample_id.wrapping_add(1);
        Ok(())
    }
}
pub fn discover(config: &SystemConfig, root: &Path) -> Result<Vec<Scope>> {
    let prefix = format!(
        "user.slice/user-{}.slice/user@{}.service",
        config.uid, config.uid
    );
    let base = root.join(&prefix);
    let mut result = vec![];
    if config.apps.is_empty() || !base.exists() {
        return Ok(result);
    }
    fn walk(
        config: &SystemConfig,
        root: &Path,
        dir: &Path,
        depth: usize,
        result: &mut Vec<Scope>,
    ) -> Result<()> {
        ensure!(result.len() <= 1024, "too many app scopes");
        if depth > 8 {
            return Ok(());
        }
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let path = entry.path();
            let unit = entry.file_name().to_string_lossy().into_owned();
            let matches: Vec<_> = config
                .apps
                .iter()
                .filter(|a| glob::Pattern::new(&a.unit_pattern).is_ok_and(|p| p.matches(&unit)))
                .collect();
            ensure!(matches.len() <= 1, "ambiguous app unit mapping for {unit}");
            if let Some(app) = matches.first() {
                let rel = path.strip_prefix(root)?.to_string_lossy().into_owned();
                ensure!(crate::config::valid_cgroup(&rel), "unsupported cgroup path");
                result.push(Scope {
                    app: app.id.clone(),
                    path: rel,
                    inode: entry.metadata()?.ino(),
                    allowed: false,
                });
            }
            walk(config, root, &path, depth + 1, result)?;
        }
        Ok(())
    }
    walk(config, root, &base, 0, &mut result)?;
    result.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(result)
}
/// /run/fp6-permissions/network.json: `{"version":1,"uid":N,"apps":{"<app
/// id>":{"wifi":b,"mobile":b,"vpn":b,"local":b}}}`, written by fp6-permd.
/// A missing file means no restrictions.
pub fn read_permissions(path: &Path, uid: u32) -> Result<BTreeMap<String, AppNet>> {
    #[derive(Deserialize)]
    struct File {
        version: u32,
        uid: u32,
        apps: BTreeMap<String, AppNet>,
    }
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(e) => return Err(e).with_context(|| path.display().to_string()),
    };
    ensure!(text.len() <= 1 << 20, "permissions file too large");
    let file: File = serde_json::from_str(&text).with_context(|| path.display().to_string())?;
    ensure!(file.version == 1, "unsupported permissions file version");
    ensure!(file.uid == uid, "permissions file is for another user");
    ensure!(file.apps.len() <= 512, "too many apps in permissions file");
    ensure!(
        file.apps.keys().all(|a| crate::appscope::valid_app_id(a)),
        "invalid app id in permissions file"
    );
    Ok(file
        .apps
        .into_iter()
        .filter(|(_, n)| n.restricted())
        .collect())
}
/// Running instances of restricted apps: lomiri-app-launch services and
/// app-*.scope / app-*.service units anywhere under the user manager.
pub fn discover_perm(
    uid: u32,
    apps: &BTreeMap<String, AppNet>,
    root: &Path,
) -> Result<Vec<PermScope>> {
    let base = root.join(format!("user.slice/user-{uid}.slice/user@{uid}.service"));
    let mut result = vec![];
    if apps.is_empty() || !base.exists() {
        return Ok(result);
    }
    fn walk(
        apps: &BTreeMap<String, AppNet>,
        root: &Path,
        dir: &Path,
        depth: usize,
        result: &mut Vec<PermScope>,
    ) -> Result<()> {
        ensure!(result.len() <= 1024, "too many app scopes");
        if depth > 8 {
            return Ok(());
        }
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(id) = crate::appscope::app_id_from_unit(&name) {
                if let Some(access) = apps.get(&id) {
                    let rel = entry
                        .path()
                        .strip_prefix(root)?
                        .to_string_lossy()
                        .into_owned();
                    if crate::config::valid_cgroup(&rel) {
                        result.push(PermScope {
                            app: id,
                            path: rel,
                            inode: entry.metadata()?.ino(),
                            access: *access,
                        });
                    }
                    // An app unit's own sub-cgroups are covered by its match.
                    continue;
                }
            }
            walk(apps, root, &entry.path(), depth + 1, result)?;
        }
        Ok(())
    }
    walk(apps, root, &base, 0, &mut result)?;
    result.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(result)
}
struct Api {
    engine: Arc<Mutex<Engine>>,
    uid: u32,
}
async fn authorize(
    uid: u32,
    header: &zbus::message::Header<'_>,
    bus: &zbus::Connection,
) -> zbus::fdo::Result<()> {
    let sender = header
        .sender()
        .ok_or_else(|| zbus::fdo::Error::AccessDenied("missing sender".into()))?;
    let proxy = zbus::fdo::DBusProxy::new(bus).await?;
    let caller = proxy
        .get_connection_unix_user(sender.clone().into())
        .await?;
    if caller != uid {
        return Err(zbus::fdo::Error::AccessDenied("wrong user".into()));
    }
    Ok(())
}
fn failed(e: impl std::fmt::Display) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(e.to_string())
}
#[zbus::interface(name = "org.fairphone.NetPolicy.Helper1")]
impl Api {
    async fn sync(
        &self,
        settings: &str,
        foreground: &str,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(connection)] bus: &zbus::Connection,
    ) -> zbus::fdo::Result<String> {
        authorize(self.uid, &header, bus).await?;
        if settings.len() > 65536 {
            return Err(failed("settings too large"));
        }
        let settings: Settings = serde_json::from_str(settings).map_err(failed)?;
        settings.validate().map_err(failed)?;
        let mut engine = self.engine.lock().map_err(failed)?;
        if !foreground.is_empty() && !engine.snapshot.apps.iter().any(|a| a == foreground) {
            return Err(failed("unknown foreground app"));
        }
        engine.settings = settings;
        engine.foreground = foreground.into();
        engine.last_seen = Some(Instant::now());
        serde_json::to_string(&engine.snapshot).map_err(failed)
    }
    async fn set_mobile_preferred(
        &self,
        enabled: bool,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(connection)] bus: &zbus::Connection,
    ) -> zbus::fdo::Result<()> {
        authorize(self.uid, &header, bus).await?;
        self.engine
            .lock()
            .map_err(failed)?
            .route(enabled)
            .map_err(failed)
    }
}
pub fn serve(config: &Path, state: &Path) -> Result<()> {
    let config: SystemConfig = read_json(config)?;
    let uid = config.uid;
    let engine = Arc::new(Mutex::new(Engine::new(config, state.to_owned())?));
    let _server = zbus::blocking::connection::Builder::system()?
        .name(NAME)?
        .serve_at(
            PATH,
            Api {
                engine: engine.clone(),
                uid,
            },
        )?
        .build()?;
    loop {
        {
            let mut engine = engine
                .lock()
                .map_err(|_| anyhow::anyhow!("helper mutex poisoned"))?;
            engine.snapshot.error = match engine.reconcile() {
                Ok(()) => String::new(),
                Err(e) => {
                    eprintln!("helper: {e:#}");
                    format!("{e:#}")
                }
            };
        }
        std::thread::sleep(Duration::from_secs(5));
    }
}
pub fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|s| s == "--cleanup") {
        let config: SystemConfig = read_json(Path::new(
            args.get(2).context("cleanup requires CONFIG.json")?,
        ))?;
        config.validate()?;
        if let Some(adapter) = config.mobile_adapter {
            let mobile: Mobile = serde_json::from_str(&run(&adapter, &["status"], None)?)?;
            if mobile.owned {
                let restored: Mobile =
                    serde_json::from_str(&run(&adapter, &["prefer-wifi"], None)?)?;
                ensure!(
                    !restored.owned,
                    "adapter failed to restore routing; retaining firewall"
                );
            }
        }
        run("nft", &["delete", "table", "inet", "fp6_netpolicy"], None)?;
        return Ok(());
    }
    let config = args
        .get(1)
        .context("usage: netpolicy-helper CONFIG.json [STATE.json]")?;
    let state = args
        .get(2)
        .map(String::as_str)
        .unwrap_or("/var/lib/fp6-netpolicy/usage.json");
    serve(Path::new(config), Path::new(state))
}
