use fp6_bluetooth_audio::{pulse::profile, Backend, Device, Service, NAME, PATH};
use std::sync::{Arc, Mutex};
#[derive(Default)]
struct State {
    devices: Vec<Device>,
    writes: Vec<(String, String)>,
    reject: bool,
}
struct Mock(Arc<Mutex<State>>);
impl Backend for Mock {
    fn devices(&mut self) -> Result<Vec<Device>, String> {
        Ok(self.0.lock().unwrap().devices.clone())
    }
    fn select(&mut self, d: &str, p: &str) -> Result<(), String> {
        let mut s = self.0.lock().unwrap();
        if s.reject {
            return Err("Disconnected".into());
        }
        s.writes.push((d.into(), p.into()));
        s.devices[0].profile = p.into();
        Ok(())
    }
}
fn fixture() -> (Service<Mock>, Arc<Mutex<State>>) {
    let state = Arc::new(Mutex::new(State {
        devices: vec![Device {
            id: "bluez_card.00_00_00_00_00_01".into(),
            address: "00:00:00:00:00:01".into(),
            name: "Test Headphones".into(),
            profile: "a2dp-sink-sbc".into(),
            codec: "sbc".into(),
            bitrate: None,
            bitrate_source: "not reported".into(),
            profiles: vec![
                profile(
                    "a2dp-sink",
                    "High Fidelity Playback (A2DP Sink, codec LDAC)",
                    true,
                )
                .unwrap(),
                profile("a2dp-sink-sbc", "SBC", true).unwrap(),
                profile("headset-head-unit-msbc", "mSBC", true).unwrap(),
                profile("a2dp-sink-aac", "AAC", false).unwrap(),
            ],
        }],
        ..Default::default()
    }));
    (
        Service::new(Mock(state.clone()), vec!["ldac".into(), "sbc".into()]),
        state,
    )
}
const ID: &str = "bluez_card.00_00_00_00_00_01";
#[test]
fn reads_never_write() {
    let (s, b) = fixture();
    for _ in 0..4 {
        let j: serde_json::Value = serde_json::from_str(&s.list_devices().unwrap()).unwrap();
        assert_eq!(j["devices"][0]["name"], "Test Headphones");
        assert!(j["devices"][0]["bitrate"].is_null());
    }
    assert!(b.lock().unwrap().writes.is_empty());
}
#[test]
fn validate_current_capabilities_and_noops() {
    let (s, b) = fixture();
    assert!(s.set_profile(ID, "a2dp-sink-aac").is_err());
    assert!(s.set_profile("gone", "a2dp-sink").is_err());
    assert!(s.set_profile(ID, "off").is_err());
    s.set_profile(ID, "a2dp-sink-sbc").unwrap();
    assert!(b.lock().unwrap().writes.is_empty());
    s.set_profile(ID, "headset-head-unit-msbc").unwrap();
    assert_eq!(b.lock().unwrap().writes.len(), 1);
    assert!(s.set_preference(ID, "quality").is_err());
}
#[test]
fn preferences_resolve_base_alias_and_stable_sbc() {
    let (s, b) = fixture();
    s.set_preference(ID, "quality").unwrap();
    s.set_preference(ID, "stable").unwrap();
    assert_eq!(
        b.lock().unwrap().writes,
        vec![
            (ID.into(), "a2dp-sink".into()),
            (ID.into(), "a2dp-sink-sbc".into())
        ]
    );
    assert!(s.set_preference(ID, "arbitrary").is_err());
}
#[test]
fn failure_and_disconnect_are_not_success() {
    let (s, b) = fixture();
    b.lock().unwrap().reject = true;
    assert!(s.set_profile(ID, "a2dp-sink").is_err());
    b.lock().unwrap().devices.clear();
    assert!(s.set_preference(ID, "stable").is_err());
    assert!(b.lock().unwrap().writes.is_empty());
}
#[test]
fn base_alias_is_never_assumed_sbc() {
    assert_eq!(
        profile("a2dp-sink", "Translated (codec aptX HD)", true)
            .unwrap()
            .codec,
        "aptx_hd"
    );
    assert_eq!(
        profile("a2dp-sink", "New format", true).unwrap().codec,
        "unknown"
    );
    assert!(profile("output:analog-stereo", "Internal", true).is_none());
    assert_eq!(
        profile("headset-head-unit-lc3-swb", "LC3-SWB", true)
            .unwrap()
            .codec,
        "lc3_swb"
    );
}
#[test]
fn dbus_contract_on_private_bus() {
    // Runner supplies a private bus; no system/session desktop service is used.
    let private_bus = PrivateBus::start();
    let address = private_bus.address.clone();
    let (s, b) = fixture();
    let server = zbus::blocking::connection::Builder::address(address.as_str())
        .unwrap()
        .name(NAME)
        .unwrap()
        .serve_at(PATH, s)
        .unwrap()
        .build()
        .unwrap();
    let client = zbus::blocking::connection::Builder::address(address.as_str())
        .unwrap()
        .build()
        .unwrap();
    let proxy = zbus::blocking::Proxy::new(&client, NAME, PATH, NAME).unwrap();
    let _: String = proxy.call("ListDevices", &()).unwrap();
    assert!(b.lock().unwrap().writes.is_empty());
    let _: () = proxy.call("SetPreference", &(ID, "quality")).unwrap();
    assert!(proxy.call::<_, _, ()>("SetProfile", &(ID, "off")).is_err());
    assert_eq!(b.lock().unwrap().writes.len(), 1);
    drop(server);
}

struct PrivateBus {
    child: std::process::Child,
    address: String,
    config: std::path::PathBuf,
}
impl PrivateBus {
    fn start() -> Self {
        use std::io::BufRead;
        let config = std::env::temp_dir().join(format!(
            "fp6-bt-test-bus-{}-{}.conf",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&config, r#"<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth><policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy></busconfig>"#).unwrap();
        let mut child = std::process::Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address=1"])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("dbus-daemon required for private-bus tests");
        let mut address = String::new();
        std::io::BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        Self {
            child,
            address: address.trim().into(),
            config,
        }
    }
}
impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.config);
    }
}

struct MockBluez(Arc<std::sync::atomic::AtomicBool>);
#[zbus::interface(name = "org.freedesktop.DBus.ObjectManager")]
impl MockBluez {
    fn get_managed_objects(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::ManagedObjects {
        self.0.store(
            header
                .primary()
                .flags()
                .contains(zbus::message::Flags::NoAutoStart),
            std::sync::atomic::Ordering::SeqCst,
        );
        let props = std::collections::HashMap::from([
            ("Connected".into(), zbus::zvariant::OwnedValue::from(true)),
            (
                "Address".into(),
                zbus::zvariant::OwnedValue::from(zbus::zvariant::Str::from("00:00:00:00:00:01")),
            ),
        ]);
        std::collections::HashMap::from([(
            zbus::zvariant::OwnedObjectPath::try_from("/org/bluez/hci0/dev_00_00_00_00_00_01")
                .unwrap(),
            std::collections::HashMap::from([(
                zbus::names::OwnedInterfaceName::try_from("org.bluez.Device1").unwrap(),
                props,
            )]),
        )])
    }
}
#[test]
fn bluez_status_disables_service_activation() {
    let bus = PrivateBus::start();
    let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let _server = zbus::blocking::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .name("org.bluez")
        .unwrap()
        .serve_at("/", MockBluez(flag.clone()))
        .unwrap()
        .build()
        .unwrap();
    let client = zbus::blocking::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .build()
        .unwrap();
    let addresses = fp6_bluetooth_audio::pulse::connected_addresses(&client).unwrap();
    assert!(addresses.contains("00:00:00:00:00:01"));
    assert!(flag.load(std::sync::atomic::Ordering::SeqCst));
}
