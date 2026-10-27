use fp6_selftest::{
    config::Config,
    dbus,
    process::{self, Failure},
    report::{Check, Status},
};
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};
struct Bus {
    process: Child,
    address: String,
    _dir: tempfile::TempDir,
}
impl Bus {
    fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("st-")
            .tempdir_in("/tmp")
            .unwrap();
        std::fs::write(dir.path().join("com.fairphone6.Stopped.service"), "[D-BUS Service]\nName=com.fairphone6.Stopped\nExec=/nonexistent-selftest-activation-must-never-run\n").unwrap();
        let cfg = dir.path().join("bus.conf");
        std::fs::write(&cfg, format!("<busconfig><type>session</type><listen>unix:tmpdir={}</listen><servicedir>{}</servicedir><policy context='default'><allow send_destination='*'/><allow receive_sender='*'/><allow own='*'/></policy></busconfig>", dir.path().display(), dir.path().display())).unwrap();
        let mut process = Command::new("dbus-daemon")
            .arg("--nofork")
            .arg("--print-address=1")
            .arg(format!("--config-file={}", cfg.display()))
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("dbus-daemon is a required test dependency");
        let mut address = String::new();
        BufReader::new(process.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        assert!(address.starts_with("unix:"));
        Self {
            process,
            address: address.trim().into(),
            _dir: dir,
        }
    }
    fn connection(&self) -> zbus::blocking::Connection {
        zbus::blocking::connection::Builder::address(self.address.as_str())
            .unwrap()
            .build()
            .unwrap()
    }
}
impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}
struct Mock {
    value: Arc<Mutex<String>>,
    calls: Arc<Mutex<usize>>,
}
#[zbus::interface(name = "com.fairphone6.Synthetic1")]
impl Mock {
    fn status(&self) -> zbus::fdo::Result<String> {
        *self.calls.lock().unwrap() += 1;
        let value = self.value.lock().unwrap().clone();
        if value == "deny" {
            Err(zbus::fdo::Error::AccessDenied(
                "synthetic@example.invalid".into(),
            ))
        } else {
            Ok(value)
        }
    }
    #[zbus(property)]
    fn state(&self) -> &str {
        "idle"
    }
}
struct Slow;
#[zbus::interface(name = "com.fairphone6.Synthetic1")]
impl Slow {
    fn status(&self) -> String {
        std::thread::sleep(Duration::from_secs(3));
        "late".into()
    }
}
fn config() -> String {
    "[[probe]]\nid='synthetic'\nname='com.fairphone6.Synthetic1'\npath='/com/fairphone6/Synthetic1'\ninterface='com.fairphone6.Synthetic1'\nmethod='Status'\nshape='json-object'\nfields={active='bool'}\n".into()
}
#[test]
fn private_bus_shapes_properties_absence_and_no_activation() {
    let bus = Bus::new();
    let client = bus.connection();
    let mut probe = Config::parse(&config()).unwrap().probe.remove(0);
    assert_eq!(dbus::check_on(&client, &probe).status, Status::Skip);
    probe.required = true;
    assert_eq!(dbus::check_on(&client, &probe).status, Status::Fail);
    probe.required = false;
    probe.name = "com.fairphone6.Stopped".into();
    assert_eq!(dbus::check_on(&client, &probe).status, Status::Warn);
    assert!(dbus::owner(&client, &probe.name).unwrap().is_none());
    probe.name = "com.fairphone6.Synthetic1".into();
    let value = Arc::new(Mutex::new(
        "{\"active\":false,\"identifier\":\"synthetic@example.invalid\"}".into(),
    ));
    let calls = Arc::new(Mutex::new(0));
    let _service = zbus::blocking::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .name(probe.name.as_str())
        .unwrap()
        .serve_at(
            probe.path.as_str(),
            Mock {
                value: value.clone(),
                calls: calls.clone(),
            },
        )
        .unwrap()
        .build()
        .unwrap();
    let result = dbus::check_on(&client, &probe);
    assert_eq!(result.status, Status::Pass, "{}", result.detail);
    assert!(!result.detail.contains("synthetic"));
    *value.lock().unwrap() = "{\"active\":\"false\"}".into();
    assert_eq!(dbus::check_on(&client, &probe).status, Status::Fail);
    *value.lock().unwrap() = "not JSON".into();
    assert_eq!(dbus::check_on(&client, &probe).status, Status::Fail);
    assert_eq!(*calls.lock().unwrap(), 3);
    *value.lock().unwrap() = "deny".into();
    let denied = dbus::check_on(&client, &probe);
    assert_eq!(denied.status, Status::Warn);
    assert!(!denied.detail.contains("synthetic"));
    *value.lock().unwrap() = "{}".into();
    probe.shape = Some("bool".into());
    assert_eq!(dbus::check_on(&client, &probe).status, Status::Fail);
    probe.method = None;
    probe.property = Some("State".into());
    probe.shape = Some("string".into());
    probe.values = vec!["idle".into()];
    assert_eq!(dbus::check_on(&client, &probe).status, Status::Pass);
    probe.values = vec!["running".into()];
    assert_eq!(dbus::check_on(&client, &probe).status, Status::Fail);
    probe.interface = "com.fairphone6.Missing".into();
    assert_eq!(dbus::check_on(&client, &probe).status, Status::Fail);
}
#[test]
fn worker_timeout_on_private_bus() {
    let bus = Bus::new();
    let _service = zbus::blocking::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .name("com.fairphone6.Synthetic1")
        .unwrap()
        .serve_at("/com/fairphone6/Synthetic1", Slow)
        .unwrap()
        .build()
        .unwrap();
    let cfg = bus._dir.path().join("probes.toml");
    std::fs::write(&cfg, config()).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fp6-selftest"));
    cmd.args(["--internal-check", "feature:0", "--config"])
        .arg(&cfg)
        .env("DBUS_SESSION_BUS_ADDRESS", &bus.address);
    assert!(matches!(
        process::run(cmd, Duration::from_millis(200)),
        Err(Failure::Timeout)
    ));
}
#[test]
fn cli_worker_discards_raw_replies() {
    let bus = Bus::new();
    let _service = zbus::blocking::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .name("com.fairphone6.Synthetic1")
        .unwrap()
        .serve_at(
            "/com/fairphone6/Synthetic1",
            Mock {
                value: Arc::new(Mutex::new(
                    "{\"active\":true,\"secret\":\"+999123456789\"}".into(),
                )),
                calls: Default::default(),
            },
        )
        .unwrap()
        .build()
        .unwrap();
    let cfg = bus._dir.path().join("probes.toml");
    std::fs::write(&cfg, config()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_fp6-selftest"))
        .args(["--internal-check", "feature:0", "--config"])
        .arg(cfg)
        .env("DBUS_SESSION_BUS_ADDRESS", &bus.address)
        .output()
        .unwrap();
    assert!(output.status.success());
    let check: Check = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(check.status, Status::Pass, "{}", check.detail);
    assert!(!String::from_utf8_lossy(&output.stdout).contains("999"));
}

#[derive(Default)]
struct ModemState {
    online: bool,
    sim: bool,
    data: bool,
    active: bool,
}
struct OfonoManager(Arc<Mutex<ModemState>>);
fn property_map(pairs: &[(&str, bool)]) -> dbus::Props {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), zbus::zvariant::OwnedValue::from(*v)))
        .collect()
}
#[zbus::interface(name = "org.ofono.Manager")]
impl OfonoManager {
    fn get_modems(&self) -> dbus::Objects {
        let s = self.0.lock().unwrap();
        vec![(
            "/synthetic_modem".try_into().unwrap(),
            property_map(&[("Powered", s.online), ("Online", s.online)]),
        )]
    }
}
struct Sim(Arc<Mutex<ModemState>>);
#[zbus::interface(name = "org.ofono.SimManager")]
impl Sim {
    fn get_properties(&self) -> dbus::Props {
        property_map(&[("Present", self.0.lock().unwrap().sim)])
    }
}
struct Registration;
#[zbus::interface(name = "org.ofono.NetworkRegistration")]
impl Registration {
    fn get_properties(&self) -> dbus::Props {
        [
            (
                "Status".into(),
                zbus::zvariant::Value::from("registered")
                    .try_to_owned()
                    .unwrap(),
            ),
            (
                "Name".into(),
                zbus::zvariant::Value::from("Synthetic private operator")
                    .try_to_owned()
                    .unwrap(),
            ),
        ]
        .into()
    }
}
struct Data(Arc<Mutex<ModemState>>);
#[zbus::interface(name = "org.ofono.ConnectionManager")]
impl Data {
    fn get_properties(&self) -> dbus::Props {
        property_map(&[("Powered", self.0.lock().unwrap().data)])
    }
    fn get_contexts(&self) -> dbus::Objects {
        let mut props = property_map(&[("Active", self.0.lock().unwrap().active)]);
        props.insert(
            "Type".into(),
            zbus::zvariant::Value::from("internet")
                .try_to_owned()
                .unwrap(),
        );
        props.insert(
            "AccessPointName".into(),
            zbus::zvariant::Value::from("synthetic.private.invalid")
                .try_to_owned()
                .unwrap(),
        );
        vec![("/synthetic_modem/context".try_into().unwrap(), props)]
    }
}
struct Network;
#[zbus::interface(name = "org.freedesktop.NetworkManager")]
impl Network {
    #[zbus(property)]
    fn state(&self) -> u32 {
        70
    }
}
fn base_worker(bus: &Bus, id: &str) -> Check {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fp6-selftest"));
    cmd.args(["--internal-check", id])
        .env("DBUS_SYSTEM_BUS_ADDRESS", &bus.address);
    let (ok, bytes) = process::run(cmd, Duration::from_secs(3))
        .ok()
        .expect("worker completes");
    assert!(ok);
    assert!(!String::from_utf8_lossy(&bytes).contains("private"));
    serde_json::from_slice(&bytes).unwrap()
}
#[test]
fn ofono_and_network_checks_use_only_private_bus_and_handle_disabled_data() {
    let bus = Bus::new();
    let state = Arc::new(Mutex::new(ModemState {
        online: true,
        sim: true,
        data: false,
        active: false,
    }));
    let _service = zbus::blocking::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .name("org.ofono")
        .unwrap()
        .name("org.freedesktop.NetworkManager")
        .unwrap()
        .serve_at("/", OfonoManager(state.clone()))
        .unwrap()
        .serve_at("/synthetic_modem", Sim(state.clone()))
        .unwrap()
        .serve_at("/synthetic_modem", Registration)
        .unwrap()
        .serve_at("/synthetic_modem", Data(state.clone()))
        .unwrap()
        .serve_at("/org/freedesktop/NetworkManager", Network)
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(base_worker(&bus, "network").status, Status::Pass);
    assert_eq!(base_worker(&bus, "modem").status, Status::Pass);
    assert_eq!(base_worker(&bus, "mobile-data").status, Status::Skip);
    state.lock().unwrap().data = true;
    assert_eq!(base_worker(&bus, "mobile-data").status, Status::Warn);
    state.lock().unwrap().active = true;
    assert_eq!(base_worker(&bus, "mobile-data").status, Status::Pass);
    state.lock().unwrap().sim = false;
    assert_eq!(base_worker(&bus, "modem").status, Status::Warn);
    state.lock().unwrap().online = false;
    assert_eq!(base_worker(&bus, "modem").status, Status::Warn);
}

#[test]
fn installed_unit_hint_prevents_skipping_stopped_or_failed_features() {
    use std::os::unix::fs::PermissionsExt;
    let bus = Bus::new();
    let shell = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|p| p.join("sh"))
        .find(|p| p.is_file())
        .unwrap();
    let systemctl = bus._dir.path().join("systemctl");
    let cfg = bus._dir.path().join("probes.toml");
    std::fs::write(&cfg, format!("{}unit='synthetic.service'\n", config())).unwrap();
    for (load, active, expected) in [
        ("loaded", "inactive", Status::Warn),
        ("loaded", "active", Status::Fail),
        ("loaded", "failed", Status::Fail),
        ("not-found", "inactive", Status::Skip),
    ] {
        std::fs::write(
            &systemctl,
            format!(
                "#!{}\nprintf 'LoadState={}\\nActiveState={}\\n'\n",
                shell.display(),
                load,
                active
            ),
        )
        .unwrap();
        std::fs::set_permissions(&systemctl, std::fs::Permissions::from_mode(0o700)).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_fp6-selftest"))
            .args(["--internal-check", "feature:0", "--config"])
            .arg(&cfg)
            .env("DBUS_SESSION_BUS_ADDRESS", &bus.address)
            .env("PATH", bus._dir.path())
            .output()
            .unwrap();
        assert!(output.status.success());
        let result: Check = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result.status, expected, "{}", result.detail);
    }
}
