//! Private bus only. Run via scripts/dbus-test.sh; never reads the host system bus.
use fp6_netpolicy::{
    config::Settings,
    daemon,
    helper::{Mobile, Snapshot},
    nm,
};
use std::{process::Command, time::Duration};
use zbus::zvariant::OwnedObjectPath;
struct MockHelper;
#[zbus::interface(name = "org.fairphone.NetPolicy.Helper1")]
impl MockHelper {
    fn sync(&self, json: &str, foreground: &str) -> String {
        let settings: Settings = serde_json::from_str(json).unwrap();
        settings.validate().unwrap();
        assert!(foreground.is_empty() || foreground == "test.chat");
        serde_json::to_string(&Snapshot {
            apps: vec!["test.chat".into()],
            mobile: Mobile::default(),
            ..Snapshot::default()
        })
        .unwrap()
    }
    fn set_mobile_preferred(&self, _enabled: bool) {
        panic!("test must not switch mobile")
    }
}
struct Manager;
#[zbus::interface(name = "org.freedesktop.NetworkManager")]
impl Manager {
    fn get_devices(&self) -> Vec<OwnedObjectPath> {
        vec!["/org/freedesktop/NetworkManager/Devices/1"
            .try_into()
            .unwrap()]
    }
    #[zbus(property)]
    fn connectivity_check_uri(&self) -> &str {
        "http://127.0.0.1/check"
    }
    #[zbus(property)]
    fn connectivity_check_enabled(&self) -> bool {
        false
    }
}
struct Device;
#[zbus::interface(name = "org.freedesktop.NetworkManager.Device")]
impl Device {
    #[zbus(property)]
    fn state(&self) -> u32 {
        100
    }
    #[zbus(property)]
    fn ip_interface(&self) -> &str {
        "wlan0"
    }
    #[zbus(property)]
    fn device_type(&self) -> u32 {
        2
    }
    #[zbus(property)]
    fn metered(&self) -> u32 {
        1
    }
    #[zbus(property)]
    fn ip4_config(&self) -> OwnedObjectPath {
        "/org/freedesktop/NetworkManager/IP4Config/1"
            .try_into()
            .unwrap()
    }
    #[zbus(property)]
    fn ip4_connectivity(&self) -> u32 {
        2
    }
    #[zbus(property)]
    fn ip6_connectivity(&self) -> u32 {
        0
    }
}
struct Wireless;
#[zbus::interface(name = "org.freedesktop.NetworkManager.Device.Wireless")]
impl Wireless {
    #[zbus(property)]
    fn active_access_point(&self) -> OwnedObjectPath {
        "/org/freedesktop/NetworkManager/AccessPoint/1"
            .try_into()
            .unwrap()
    }
}
struct AccessPoint;
#[zbus::interface(name = "org.freedesktop.NetworkManager.AccessPoint")]
impl AccessPoint {
    #[zbus(property)]
    fn strength(&self) -> u8 {
        42
    }
}
struct IpConfig;
#[zbus::interface(name = "org.freedesktop.NetworkManager.IP4Config")]
impl IpConfig {
    #[zbus(property)]
    fn gateway(&self) -> &str {
        "192.0.2.1"
    }
}
struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
#[ignore = "requires a private dbus-run-session; use scripts/dbus-test.sh"]
fn private_bus_nm_adapter_and_settings_roundtrip() {
    assert_eq!(
        std::env::var("NETPOLICY_TEST_BUS").as_deref(),
        Ok("private")
    );
    let _mock = zbus::blocking::connection::Builder::session()
        .unwrap()
        .name(fp6_netpolicy::helper::NAME)
        .unwrap()
        .name("org.freedesktop.NetworkManager")
        .unwrap()
        .serve_at(fp6_netpolicy::helper::PATH, MockHelper)
        .unwrap()
        .serve_at("/org/freedesktop/NetworkManager", Manager)
        .unwrap()
        .serve_at("/org/freedesktop/NetworkManager/Devices/1", Device)
        .unwrap()
        .serve_at("/org/freedesktop/NetworkManager/Devices/1", Wireless)
        .unwrap()
        .serve_at("/org/freedesktop/NetworkManager/AccessPoint/1", AccessPoint)
        .unwrap()
        .serve_at("/org/freedesktop/NetworkManager/IP4Config/1", IpConfig)
        .unwrap()
        .build()
        .unwrap();
    let bus = fp6_netpolicy::runtime::session().unwrap();
    let network = nm::read(&bus).unwrap();
    assert_eq!(network.costly_interfaces, ["wlan0"]);
    let wifi = network.wifi.unwrap();
    assert_eq!(wifi.signal, 42);
    assert!(wifi.captive);
    assert_eq!(wifi.gateway, "192.0.2.1");
    assert!(!network.connectivity_enabled);
    let config = std::env::temp_dir().join(format!("netpolicy-bus-{}", std::process::id()));
    let _child = Child(
        Command::new(env!("CARGO_BIN_EXE_netpolicyd"))
            .env(
                "DBUS_SYSTEM_BUS_ADDRESS",
                std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap(),
            )
            .env("XDG_CONFIG_HOME", &config)
            .spawn()
            .unwrap(),
    );
    let mut ready = false;
    for _ in 0..100 {
        if let Ok(raw) = daemon::call("status", None) {
            let status: daemon::Status = serde_json::from_str(&raw).unwrap();
            if !status.snapshot.apps.is_empty() {
                ready = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(ready, "daemon did not become ready");
    daemon::call("saver", Some("off")).unwrap();
    daemon::call("foreground", Some("test.chat")).unwrap();
    assert!(daemon::call("foreground", Some("not.configured")).is_err());
    assert!(daemon::call("settings", Some(r#"{"unexpected":1}"#)).is_err());
    let status: daemon::Status =
        serde_json::from_str(&daemon::call("status", None).unwrap()).unwrap();
    assert!(!status.settings.data_saver);
    assert_eq!(status.foreground, "test.chat");
    let saved: Settings =
        fp6_netpolicy::runtime::read_json(&config.join("fp6-netpolicy/settings.json")).unwrap();
    assert!(!saved.data_saver);
    std::fs::remove_dir_all(config).unwrap();
}

struct EmptyManager;
#[zbus::interface(name = "org.freedesktop.NetworkManager")]
impl EmptyManager {
    fn get_devices(&self) -> Vec<OwnedObjectPath> {
        vec![]
    }
    #[zbus(property)]
    fn connectivity_check_uri(&self) -> &str {
        ""
    }
    #[zbus(property)]
    fn connectivity_check_enabled(&self) -> bool {
        false
    }
}
#[test]
#[ignore = "requires a private dbus-run-session; use scripts/dbus-test.sh"]
fn real_helper_rejects_other_uid_and_unapproved_mobile() {
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::env::var("NETPOLICY_TEST_BUS").as_deref(),
        Ok("private")
    );
    let _nm = zbus::blocking::connection::Builder::session()
        .unwrap()
        .name("org.freedesktop.NetworkManager")
        .unwrap()
        .serve_at("/org/freedesktop/NetworkManager", EmptyManager)
        .unwrap()
        .build()
        .unwrap();
    let bus = fp6_netpolicy::runtime::session().unwrap();
    let dbus = zbus::blocking::fdo::DBusProxy::new(&bus).unwrap();
    let uid = dbus
        .get_connection_unix_user(bus.unique_name().unwrap().clone().into())
        .unwrap();
    assert!(uid >= 1000, "run this fixture as a normal user");
    let dir = std::env::temp_dir().join(format!("netpolicy-auth-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    let nft = dir.join("bin/nft");
    // A fake command is intentional: authentication tests never mutate nftables.
    std::fs::write(
        &nft,
        "#!/bin/sh\nif [ \"$1\" = list ]; then exit 1; fi\ncat >/dev/null\n",
    )
    .unwrap();
    std::fs::set_permissions(&nft, std::fs::Permissions::from_mode(0o755)).unwrap();
    for configured_uid in [uid + 1, uid] {
        let config = fp6_netpolicy::config::SystemConfig {
            uid: configured_uid,
            apps: vec![],
            system_allow: vec![],
            mobile_adapter: None,
        };
        fp6_netpolicy::runtime::save_json(&dir.join("config.json"), &config).unwrap();
        let mut child = Child(
            Command::new(env!("CARGO_BIN_EXE_netpolicy-helper"))
                .arg(dir.join("config.json"))
                .arg(dir.join("usage.json"))
                .env(
                    "DBUS_SYSTEM_BUS_ADDRESS",
                    std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap(),
                )
                .env(
                    "PATH",
                    format!(
                        "{}:{}",
                        dir.join("bin").display(),
                        std::env::var("PATH").unwrap()
                    ),
                )
                .spawn()
                .unwrap(),
        );
        for _ in 0..100 {
            if dbus
                .name_has_owner(fp6_netpolicy::helper::NAME.try_into().unwrap())
                .unwrap()
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let helper = zbus::blocking::Proxy::new(
            &bus,
            fp6_netpolicy::helper::NAME,
            fp6_netpolicy::helper::PATH,
            fp6_netpolicy::helper::NAME,
        )
        .unwrap();
        let result: zbus::Result<String> = helper.call(
            "Sync",
            &(serde_json::to_string(&Settings::default()).unwrap(), ""),
        );
        if configured_uid == uid {
            assert!(result.is_ok(), "{result:?}");
            assert!(helper
                .call::<_, _, ()>("SetMobilePreferred", &true)
                .is_err());
            assert!(helper
                .call::<_, _, String>("Sync", &("{}", "unconfigured"))
                .is_err());
        } else {
            assert!(result.unwrap_err().to_string().contains("AccessDenied"));
        }
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        for _ in 0..100 {
            if !dbus
                .name_has_owner(fp6_netpolicy::helper::NAME.try_into().unwrap())
                .unwrap()
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}
