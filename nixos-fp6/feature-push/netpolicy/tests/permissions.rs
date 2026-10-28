// App network permission (app-permissions): nftables rendering, the
// fp6-permd file contract and cgroup discovery.
use fp6_netpolicy::helper::{discover_perm, read_permissions};
use fp6_netpolicy::nft::{counter_name, AppNet, PermScope, Rules, Scope};
use std::collections::{BTreeMap, BTreeSet};

const UAL: &str = "user.slice/user-1000.slice/user@1000.service/app.slice/lomiri-app-launch--application-legacy--chat--.service";

fn net(wifi: bool, mobile: bool, vpn: bool, local: bool) -> AppNet {
    AppNet {
        wifi,
        mobile,
        vpn,
        local,
    }
}

fn rules(access: AppNet) -> Rules {
    Rules {
        uid: 1000,
        interfaces: vec!["wwan0".into()],
        scopes: vec![],
        system_allow: vec!["system.slice/systemd-resolved.service".into()],
        restrict_unknown: false,
        perm: vec![PermScope {
            app: "chat".into(),
            path: UAL.into(),
            inode: 7,
            access,
        }],
        mobile_interfaces: vec!["wwan0".into()],
        vpn_interfaces: vec!["nb0".into()],
    }
}

#[test]
fn mobile_only_denial_golden() {
    assert_eq!(
        rules(net(true, false, true, true))
            .render(&BTreeSet::new(), false)
            .unwrap(),
        include_str!("golden/perm-mobile.nft")
    );
}

#[test]
fn no_network_at_all_is_one_drop() {
    let r = rules(net(false, false, false, false))
        .render(&BTreeSet::new(), true)
        .unwrap();
    let chain: Vec<&str> = r
        .lines()
        .filter(|l| l.starts_with("add rule inet fp6_netpolicy perm_0_out"))
        .collect();
    assert_eq!(chain, vec!["add rule inet fp6_netpolicy perm_0_out drop"]);
    // the jump comes right after loopback, before system/app/saver rules
    let out: Vec<&str> = r
        .lines()
        .filter(|l| l.starts_with("add rule inet fp6_netpolicy out "))
        .collect();
    assert!(out[0].ends_with("\"lo\" return"));
    assert!(out[1].ends_with("jump perm_0_out"), "{}", out[1]);
    assert!(r.contains("jump perm_0_in"));
}

#[test]
fn verdict_order_mobile_and_vpn_before_lan() {
    // Wi-Fi denied, local allowed: LAN over Wi-Fi passes, internet over Wi-Fi
    // drops; a carrier 10.x address over rmnet follows the mobile toggle.
    let r = rules(net(false, true, true, true))
        .render(&BTreeSet::new(), false)
        .unwrap();
    let chain: Vec<&str> = r
        .lines()
        .filter(|l| l.starts_with("add rule inet fp6_netpolicy perm_0_out"))
        .collect();
    let pos = |needle: &str| chain.iter().position(|l| l.contains(needle)).unwrap();
    assert!(pos("oifname \"rmnet*\" return") < pos("ip daddr"));
    assert!(pos("oifname \"nb0\" return") < pos("ip daddr"));
    assert!(chain[pos("ip daddr")].ends_with("return"));
    assert_eq!(
        chain.last().unwrap(),
        &"add rule inet fp6_netpolicy perm_0_out drop"
    );
    // local denied only
    let r = rules(net(true, true, true, false))
        .render(&BTreeSet::new(), false)
        .unwrap();
    assert!(r.contains("perm_0_out ip daddr { 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, 169.254.0.0/16, 224.0.0.0/4, 255.255.255.255 } drop"));
    assert!(r.contains("perm_0_in ip6 saddr { fc00::/7, fe80::/10, ff00::/8 } drop"));
    assert!(!r
        .lines()
        .any(|l| l == "add rule inet fp6_netpolicy perm_0_out drop"));
}

#[test]
fn data_saver_output_unchanged_without_permissions() {
    // The existing goldens still hold with the new (empty) fields.
    let r = Rules {
        uid: 1000,
        interfaces: vec!["wwan0".into()],
        scopes: vec![Scope {
            app: "chat".into(),
            path: "user.slice/user-1000.slice/user@1000.service/app.slice/app-chat.scope".into(),
            inode: 42,
            allowed: false,
        }],
        system_allow: vec!["system.slice/systemd-resolved.service".into()],
        restrict_unknown: true,
        ..Default::default()
    };
    assert_eq!(
        r.render(&BTreeSet::new(), false).unwrap(),
        include_str!("golden/background.nft")
    );
    // with a restricted app, saver rules still follow and keep counters
    let mut r2 = r.clone();
    r2.perm = rules(net(true, false, true, true)).perm;
    let text = r2
        .render(
            &BTreeSet::from([counter_name("chat", "tx"), counter_name("chat", "rx")]),
            true,
        )
        .unwrap();
    assert!(text.contains("counter name a63686174_rx drop"));
    assert!(text.contains("meta skuid 1000 drop"));
}

#[test]
fn stale_chains_are_deleted_and_names_validated() {
    let mut r = rules(net(true, false, true, true));
    let two = {
        let mut x = r.clone();
        x.perm.push(PermScope {
            app: "zz".into(),
            path: format!("{UAL}x"),
            inode: 8,
            access: net(false, false, false, false),
        });
        x
    };
    assert_eq!(two.perm_apps().len(), 2);
    let text = r.render_with(&BTreeSet::new(), true, 2).unwrap();
    assert!(text.ends_with(
        "delete chain inet fp6_netpolicy perm_1_out\ndelete chain inet fp6_netpolicy perm_1_in\n"
    ));
    // two instances of one app share one chain
    r.perm.push(PermScope {
        app: "chat".into(),
        path: format!("{UAL}/sub"),
        inode: 9,
        access: net(true, false, true, true),
    });
    assert_eq!(r.perm_apps().len(), 1);
    let text = r.render(&BTreeSet::new(), false).unwrap();
    assert_eq!(text.matches("jump perm_0_out").count(), 2);
    // deepest first
    let jumps: Vec<&str> = text
        .lines()
        .filter(|l| l.contains("jump perm_0_out"))
        .collect();
    assert!(jumps[0].contains("/sub\""));
    let mut bad = rules(net(false, true, true, true));
    bad.vpn_interfaces = vec!["x\"; flush ruleset".into()];
    assert!(bad.render(&BTreeSet::new(), false).is_err());
    let mut bad = rules(net(false, true, true, true));
    bad.perm[0].path = "user.slice/../system.slice".into();
    assert!(bad.render(&BTreeSet::new(), false).is_err());
}

#[test]
fn permissions_file_contract() {
    let dir = std::env::temp_dir().join(format!("netpolicy-perm-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("network.json");
    assert!(
        read_permissions(&f, 1000).unwrap().is_empty(),
        "missing file = no restrictions"
    );
    std::fs::write(&f, r#"{"version":1,"uid":1000,"apps":{"chat":{"wifi":true,"mobile":false,"vpn":true,"local":true},"open":{"wifi":true,"mobile":true,"vpn":true,"local":true}}}"#).unwrap();
    let m = read_permissions(&f, 1000).unwrap();
    assert_eq!(m.len(), 1);
    assert!(!m["chat"].mobile && m["chat"].wifi);
    assert!(read_permissions(&f, 1001).is_err(), "wrong user");
    std::fs::write(
        &f,
        r#"{"version":1,"uid":1000,"apps":{"../x":{"wifi":false}}}"#,
    )
    .unwrap();
    assert!(read_permissions(&f, 1000).is_err());
    std::fs::write(&f, r#"{"version":2,"uid":1000,"apps":{}}"#).unwrap();
    assert!(read_permissions(&f, 1000).is_err());
    std::fs::write(&f, "{").unwrap();
    assert!(read_permissions(&f, 1000).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn discovery_finds_ual_services_and_app_scopes() {
    let root = std::env::temp_dir().join(format!("netpolicy-permdisc-{}", std::process::id()));
    let base = root.join("user.slice/user-1000.slice/user@1000.service");
    for d in [
        "app.slice/lomiri-app-launch--application-legacy--chat--.service/inner",
        "app.slice/lomiri-app-launch--application-legacy--chat--42.service",
        "app.slice/app-lomiri-chat-777.scope",
        "app.slice/lomiri-app-launch--application-legacy--other--.service",
        "session.slice/lomiri.service",
    ] {
        std::fs::create_dir_all(base.join(d)).unwrap();
    }
    let apps = BTreeMap::from([("chat".to_string(), net(false, false, false, false))]);
    let found = discover_perm(1000, &apps, &root).unwrap();
    let paths: Vec<&str> = found
        .iter()
        .map(|s| s.path.rsplit('/').next().unwrap())
        .collect();
    assert_eq!(
        paths,
        vec![
            "app-lomiri-chat-777.scope",
            "lomiri-app-launch--application-legacy--chat--.service",
            "lomiri-app-launch--application-legacy--chat--42.service",
        ]
    );
    assert!(found.iter().all(|s| s.app == "chat" && s.inode > 0));
    assert!(discover_perm(1000, &BTreeMap::new(), &root)
        .unwrap()
        .is_empty());
    std::fs::remove_dir_all(root).unwrap();
}
