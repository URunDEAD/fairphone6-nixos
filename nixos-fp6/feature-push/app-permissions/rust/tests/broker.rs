// Broker logic on a scratch tree: identity, prompting rules, while-using
// revocation, enforcement files (AppArmor include, drop-ins, network.json).
use fp6_permissions::broker::{Broker, Config, Creds, Grant, Profile};
use fp6_permissions::model::Store;
use fp6_permissions::model::{Answer, Decision, Permission};
use fp6_permissions::policy::{Outcome, Subject};
use std::fs;
use std::path::PathBuf;

const UID: u32 = 10000;
const UAL: &str = "/user.slice/user-10000.slice/user@10000.service/app.slice/lomiri-app-launch--application-legacy--org.example.Serial--.service";

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fp6-perm-test-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn broker(name: &str) -> (Broker, PathBuf) {
    let root = scratch(name);
    let cfg = Config {
        uid: UID,
        system_apps: vec!["lomiri-system-settings".into(), "lomiri-terminal-app".into()],
        trusted_units: vec!["fp6-nfcd.service".into()],
        agent_unit: "fp6-permission-agent.service".into(),
        apparmor: [("morph-browser".to_string(), Profile { path: "/nonexistent/morph-browser".into(), complain: false })].into(),
        apparmor_parser: "true".into(),
        state_dir: root.join("state"),
        run_dir: root.join("run"),
        dropin_root: root.join("systemd-user"),
        grace_secs: 30,
        prompt_timeout_secs: 60,
        devices: true,
        network: true,
        enforce: true,
        media: vec![Permission::Camera, Permission::Microphone, Permission::Location],
        defaults: [
            ("lomiri-camera-app".to_string(), [(Permission::Camera, Decision::Allow), (Permission::Microphone, Decision::Allow)].into()),
            ("morph-browser".to_string(), [(Permission::Camera, Decision::Deny)].into()),
        ]
        .into(),
    };
    let mut b = Broker::new(cfg).unwrap();
    b.proc_root = root.join("proc");
    (b, root)
}

fn fake_pid(root: &std::path::Path, pid: u32, cgroup: &str) {
    let d = root.join("proc").join(pid.to_string());
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("cgroup"), format!("0::{cgroup}\n")).unwrap();
}

#[test]
fn identities() {
    let (b, root) = broker("id");
    fake_pid(&root, 100, UAL);
    fake_pid(&root, 101, "/user.slice/user-10000.slice/session-2.scope");
    fake_pid(&root, 102, "/user.slice/user-10000.slice/user@10000.service/app.slice/fp6-permission-agent.service");
    fake_pid(&root, 103, "/user.slice/user-10000.slice/user@10000.service/app.slice/fp6-nfcd.service");
    let c = |pid, uid, label: Option<&[u8]>| Creds { uid: Some(uid), pid: Some(pid), label: label.map(|l| l.to_vec()) };
    assert_eq!(b.subject(&c(100, UID, None)), Subject::App { id: "org.example.Serial".into(), confined: false });
    assert_eq!(b.subject(&c(100, UID, Some(b"unconfined"))), Subject::App { id: "org.example.Serial".into(), confined: false });
    assert_eq!(b.subject(&c(100, UID, Some(b"morph-browser (enforce)"))), Subject::App { id: "morph-browser".into(), confined: true });
    assert_eq!(b.subject(&c(101, UID, None)), Subject::System { what: "session".into() });
    assert_eq!(b.subject(&c(1, 0, None)), Subject::System { what: "root".into() });
    // no /proc entry: an unknown app, never trusted
    assert_eq!(b.subject(&c(999, UID, None)), Subject::App { id: "unknown".into(), confined: false });
    assert!(b.is_agent_unit(&c(102, UID, None)));
    assert!(!b.is_agent_unit(&c(100, UID, None)));
    assert!(!b.is_agent_unit(&c(102, 10001, None)));
    assert!(!b.is_agent_unit(&c(102, UID, Some(b"morph-browser (enforce)"))));
    assert!(b.is_trusted_daemon(&c(1, 0, None)));
    assert!(b.is_trusted_daemon(&c(103, UID, None)));
    assert!(!b.is_trusted_daemon(&c(100, UID, None)));
    assert!(!b.is_trusted_daemon(&c(1, 0, Some(b"morph-browser (enforce)"))));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn prompt_rules_and_system_components() {
    let (mut b, root) = broker("rules");
    let app = Subject::App { id: "org.example.Serial".into(), confined: false };
    let settings = Subject::App { id: "lomiri-system-settings".into(), confined: false };
    let sys = Subject::System { what: "session".into() };
    // no focus information yet: prompts
    assert_eq!(b.evaluate(&app, Permission::Usb, 1000), Outcome::Prompt);
    assert_eq!(b.evaluate(&settings, Permission::Usb, 1000), Outcome::Granted);
    assert_eq!(b.evaluate(&sys, Permission::Contacts, 1000), Outcome::Granted);
    // background app: quietly refused
    b.set_focus(Some("lomiri-clock-app".into()), 1000);
    assert_eq!(b.evaluate(&app, Permission::Usb, 1100), Outcome::Denied);
    b.set_focus(Some("org.example.Serial".into()), 1100);
    assert_eq!(b.evaluate(&app, Permission::Usb, 1100), Outcome::Prompt);
    b.log_prompt("org.example.Serial", Permission::Usb, Answer::WhileUsing, 1100);
    assert_eq!(b.store.decision("org.example.Serial", Permission::Usb), Decision::WhileUsing);
    assert_eq!(b.evaluate(&app, Permission::Usb, 1101), Outcome::Granted);
    // dismissing does not remember
    b.log_prompt("org.example.Serial", Permission::Files, Answer::Dismissed, 1101);
    assert_eq!(b.store.decision("org.example.Serial", Permission::Files), Decision::Ask);
    // the grant is taken back once the app has been in the background > grace
    b.grants.push(Grant { app: "org.example.Serial".into(), perm: Permission::Usb, node: "/dev/ttyACM0".into(), usb_sysfs: "/x".into(), since: 1101 });
    b.set_focus(Some("lomiri-clock-app".into()), 1110);
    assert!(b.take_revocable(1120).is_empty(), "within grace");
    assert_eq!(b.take_revocable(1200).len(), 1);
    assert!(b.grants.is_empty());
    // Settings change to deny revokes at once
    b.set_focus(Some("org.example.Serial".into()), 1300);
    b.grants.push(Grant { app: "org.example.Serial".into(), perm: Permission::Usb, node: "/dev/ttyACM0".into(), usb_sysfs: "/x".into(), since: 1300 });
    assert_eq!(b.set_decision("org.example.Serial", Permission::Usb, Decision::Deny, 1301).len(), 1);
    b.record(&app, Permission::Usb, false, "Arduino Uno (2341:0043)", 1302);
    let st: serde_json::Value = serde_json::from_str(&b.state_json(1303)).unwrap();
    assert_eq!(st["apps"]["org.example.Serial"]["perms"]["usb"]["decision"], "deny");
    assert_eq!(st["apps"]["org.example.Serial"]["perms"]["usb"]["last_denied"], 1302);
    assert!(st["history"].as_array().unwrap().iter().any(|e| e["kind"] == "revoked"));
    // persisted and reloaded
    b.save().unwrap();
    let again = Broker::new(b.cfg.clone()).unwrap();
    assert_eq!(again.store.decision("org.example.Serial", Permission::Usb), Decision::Deny);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn enforcement_files() {
    let (mut b, root) = broker("enforce");
    b.cfg.defaults.clear();
    // nothing restricted: empty network file, empty include, no drop-ins
    let (profiles, dropins) = b.render_all().unwrap();
    assert!(profiles.is_empty() && !dropins);
    let net: serde_json::Value = serde_json::from_str(&fs::read_to_string(root.join("run/network.json")).unwrap()).unwrap();
    assert!(net["apps"].as_object().unwrap().is_empty());
    assert_eq!(fs::read_to_string(root.join("state/apparmor/morph-browser")).unwrap(), "");

    b.set_network("org.example.Serial", "mobile", false, 10).unwrap();
    b.set_decision("org.example.Serial", Permission::Bluetooth, Decision::Deny, 10);
    b.set_decision("morph-browser", Permission::Contacts, Decision::Deny, 10);
    b.set_decision("morph-browser", Permission::Usb, Decision::Allow, 10);
    // system components cannot lose their network
    assert!(b.set_network("lomiri-system-settings", "all", false, 10).is_err());
    let (profiles, dropins) = b.render_all().unwrap();
    assert_eq!(profiles, vec!["morph-browser".to_string()]);
    assert!(dropins);
    let inc = fs::read_to_string(root.join("state/apparmor/morph-browser")).unwrap();
    assert!(inc.contains("deny @{HOME}/.local/share/evolution/addressbook/** rwkl,"));
    assert!(inc.contains("/dev/hidraw[0-9]* rw,"));
    let drop = root.join("systemd-user/lomiri-app-launch--application-legacy--org.example.Serial--.service.d/60-fp6-permissions.conf");
    assert!(fs::read_to_string(&drop).unwrap().contains("RestrictAddressFamilies=~AF_BLUETOOTH"));
    let net: serde_json::Value = serde_json::from_str(&fs::read_to_string(root.join("run/network.json")).unwrap()).unwrap();
    assert_eq!(net["apps"]["org.example.Serial"]["mobile"], false);
    assert_eq!(net["apps"]["org.example.Serial"]["wifi"], true);
    // unchanged: nothing to reload
    let (profiles, dropins) = b.render_all().unwrap();
    assert!(profiles.is_empty() && !dropins);
    // reset removes the drop-in and the network rule
    b.reset_app("org.example.Serial", 20);
    let (_, dropins) = b.render_all().unwrap();
    assert!(dropins && !drop.exists());
    let net: serde_json::Value = serde_json::from_str(&fs::read_to_string(root.join("run/network.json")).unwrap()).unwrap();
    assert!(net["apps"].as_object().unwrap().is_empty());
    // reload uses the configured parser ("true" here)
    assert!(b.reload_profiles(&["morph-browser".into()]).is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn config_validation() {
    let (b, root) = broker("cfg");
    let mut c = b.cfg.clone();
    c.uid = 0;
    assert!(c.validate().is_err());
    let mut c = b.cfg.clone();
    c.system_apps.push("../evil".into());
    assert!(c.validate().is_err());
    let mut c = b.cfg.clone();
    c.trusted_units.push("x".into());
    assert!(c.validate().is_err());
    let j = r#"{"uid":10000}"#;
    let c: Config = serde_json::from_str(j).unwrap();
    assert!(c.validate().is_ok());
    assert_eq!(c.grace_secs, 30);
    assert!(serde_json::from_str::<Config>(r#"{"uid":10000,"bogus":1}"#).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn media_presets_and_only_this_time() {
    let (mut b, root) = broker("media");
    let cam = Subject::App { id: "lomiri-camera-app".into(), confined: false };
    let rec = Subject::App { id: "org.example.Recorder".into(), confined: false };
    // presets apply without a prompt; what they do not cover asks
    assert_eq!(b.evaluate(&cam, Permission::Camera, 10), Outcome::Granted);
    assert_eq!(b.evaluate(&cam, Permission::Microphone, 10), Outcome::Granted);
    assert_eq!(b.evaluate(&cam, Permission::Location, 10), Outcome::Prompt);
    let st: serde_json::Value = serde_json::from_str(&b.state_json(10)).unwrap();
    assert_eq!(st["apps"]["lomiri-camera-app"]["perms"]["camera"]["decision"], "allow");
    assert_eq!(st["apps"]["lomiri-camera-app"]["perms"]["camera"]["preset"], true);
    assert_eq!(st["media"], serde_json::json!(["camera", "microphone", "location"]));
    // the user's choice wins over the preset, even "ask"; reset restores it
    b.set_decision("lomiri-camera-app", Permission::Camera, Decision::Ask, 11);
    assert_eq!(b.evaluate(&cam, Permission::Camera, 11), Outcome::Prompt);
    let st: serde_json::Value = serde_json::from_str(&b.state_json(11)).unwrap();
    assert_eq!(st["apps"]["lomiri-camera-app"]["perms"]["camera"]["preset"], false);
    b.reset_app("lomiri-camera-app", 12);
    assert_eq!(b.evaluate(&cam, Permission::Camera, 12), Outcome::Granted);
    // a use does not turn a preset into a decision
    b.record(&cam, Permission::Camera, true, "", 13);
    assert_eq!(b.store.explicit_decision("lomiri-camera-app", Permission::Camera), None);
    assert_eq!(b.decision("lomiri-camera-app", Permission::Camera), Decision::Allow);

    // "Only this time": granted while in use, not remembered, ends in the background
    b.set_focus(Some("org.example.Recorder".into()), 20);
    assert_eq!(b.evaluate(&rec, Permission::Microphone, 20), Outcome::Prompt);
    b.log_prompt("org.example.Recorder", Permission::Microphone, Answer::Once, 20);
    assert_eq!(b.evaluate(&rec, Permission::Microphone, 21), Outcome::Granted);
    assert_eq!(b.store.explicit_decision("org.example.Recorder", Permission::Microphone), None);
    assert_eq!(b.evaluate(&rec, Permission::Camera, 21), Outcome::Prompt, "only the permission asked for");
    b.set_focus(Some("lomiri-clock-app".into()), 30);
    assert_eq!(b.evaluate(&rec, Permission::Microphone, 50), Outcome::Granted, "within grace");
    assert!(!b.expire_once(50));
    assert!(b.expire_once(100));
    assert_eq!(b.evaluate(&rec, Permission::Microphone, 100), Outcome::Denied, "background, ask: refused quietly");
    // a Settings change drops the grant at once
    b.set_focus(Some("org.example.Recorder".into()), 110);
    b.log_prompt("org.example.Recorder", Permission::Microphone, Answer::Once, 110);
    b.set_decision("org.example.Recorder", Permission::Microphone, Decision::Deny, 111);
    assert_eq!(b.evaluate(&rec, Permission::Microphone, 111), Outcome::Denied);
    assert!(b.once.is_empty());

    // daemons naming an app by id (GeoClue's agent)
    assert_eq!(b.app_subject("morph-browser"), Subject::App { id: "morph-browser".into(), confined: true });
    assert_eq!(b.app_subject("lomiri-weather-app"), Subject::App { id: "lomiri-weather-app".into(), confined: false });

    // confined app with the camera preset "deny": the device nodes are denied
    b.render_all().unwrap();
    let inc = fs::read_to_string(root.join("state/apparmor/morph-browser")).unwrap();
    assert!(inc.contains("deny /dev/video[0-9]* rw,"), "{inc}");
    // without the media option nothing camera-related is enforced
    b.cfg.media.clear();
    let (profiles, _) = b.render_all().unwrap();
    assert_eq!(profiles, vec!["morph-browser".to_string()]);
    assert!(!fs::read_to_string(root.join("state/apparmor/morph-browser")).unwrap().contains("/dev/video"));
    let st: serde_json::Value = serde_json::from_str(&b.state_json(120)).unwrap();
    assert!(st["apps"]["lomiri-camera-app"]["perms"].get("microphone").is_none(), "presets of unenforced permissions are hidden");
    // stores from before the media permissions still load
    let old: Store = serde_json::from_str(r#"{"version":1,"apps":{"a":{"perms":{"usb":{"decision":"allow","changed":5}}}}}"#).unwrap();
    assert_eq!(old.explicit_decision("a", Permission::Usb), Some(Decision::Allow));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn presets_are_validated() {
    let j = r#"{"uid":10000,"media":["camera"],"defaults":{"lomiri-camera-app":{"camera":"allow","microphone":"while_using"}}}"#;
    let c: Config = serde_json::from_str(j).unwrap();
    assert!(c.validate().is_ok());
    assert_eq!(c.preset("lomiri-camera-app", Permission::Microphone), Some(Decision::WhileUsing));
    assert!(c.enforced(Permission::Camera) && !c.enforced(Permission::Location) && c.enforced(Permission::Usb));
    assert!(serde_json::from_str::<Config>(r#"{"uid":10000,"defaults":{"a":{"camera":"maybe"}}}"#).is_err());
    let c: Config = serde_json::from_str(r#"{"uid":10000,"defaults":{"../x":{"camera":"allow"}}}"#).unwrap();
    assert!(c.validate().is_err());
}
