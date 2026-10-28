use fp6_netpolicy::nft::{counter_name, read_counters, Rules, Scope};
use std::collections::BTreeSet;
fn rules(allowed: bool) -> Rules {
    Rules {
        uid: 1000,
        interfaces: vec!["wwan0".into()],
        scopes: vec![Scope {
            app: "chat".into(),
            path: "user.slice/user-1000.slice/user@1000.service/app.slice/app-chat.scope".into(),
            inode: 42,
            allowed,
        }],
        system_allow: vec!["system.slice/systemd-resolved.service".into()],
        restrict_unknown: true,
        ..Default::default()
    }
}
#[test]
fn background_golden() {
    assert_eq!(
        rules(false).render(&BTreeSet::new(), false).unwrap(),
        include_str!("golden/background.nft")
    );
}
#[test]
fn foreground_golden_preserves_counters() {
    let counters = BTreeSet::from([counter_name("chat", "tx"), counter_name("chat", "rx")]);
    assert_eq!(
        rules(true).render(&counters, true).unwrap(),
        include_str!("golden/foreground.nft")
    );
}
#[test]
fn unmetered_golden() {
    let mut r = rules(true);
    r.interfaces.clear();
    assert_eq!(
        r.render(&BTreeSet::new(), false).unwrap(),
        include_str!("golden/unmetered.nft")
    );
}
#[test]
fn reject_rule_injection_and_traversal() {
    let mut r = rules(false);
    r.interfaces[0] = "x\"; flush ruleset".into();
    assert!(r.render(&BTreeSet::new(), false).is_err());
    r = rules(false);
    r.scopes[0].path = "user.slice/../system.slice".into();
    assert!(r.render(&BTreeSet::new(), false).is_err());
    r = rules(false);
    r.scopes[0].path = "user.slice/x\"".into();
    assert!(r.render(&BTreeSet::new(), false).is_err());
}
#[test]
fn names_are_injective_and_escaped_systemd_names_work() {
    assert_ne!(counter_name("a.b", "tx"), counter_name("a_b", "tx"));
    let mut r = rules(true);
    r.scopes[0].path =
        r"user.slice/user-1000.slice/user@1000.service/app.slice/app-a\x2db.scope".into();
    assert!(r
        .render(&BTreeSet::new(), false)
        .unwrap()
        .contains(r"a\\x2db.scope"));
}
#[test]
fn counters_sum_rx_tx_but_ignore_other_tables() {
    let json = r#"{"nftables":[{"counter":{"family":"inet","table":"fp6_netpolicy","name":"a63686174_tx","bytes":100}},{"counter":{"family":"inet","table":"fp6_netpolicy","name":"a63686174_rx","bytes":70}},{"counter":{"family":"inet","table":"unrelated","name":"a63686174_tx","bytes":999}}]}"#;
    let (names, totals) = read_counters(json, &["chat".into()]).unwrap();
    assert_eq!(names.len(), 2);
    assert_eq!(totals["chat"], 170);
}
#[test]
fn no_established_shortcut_or_global_ruleset_flush() {
    let rendered = rules(false).render(&BTreeSet::new(), false).unwrap();
    assert!(!rendered.contains("ct state"));
    assert!(!rendered.contains("flush ruleset"));
    assert!(rendered.contains("hook input"));
    assert!(rendered.contains("hook output"));
}
