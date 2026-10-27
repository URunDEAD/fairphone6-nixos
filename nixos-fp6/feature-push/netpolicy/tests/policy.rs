use fp6_netpolicy::{
    config::Settings,
    policy::{app_allowed, metered, Metrics, Route, Switcher},
    usage::Usage,
};
use std::collections::BTreeMap;
fn settings() -> Settings {
    Settings {
        smart_switching: true,
        capped_plan: false,
        ..Settings::default()
    }
}
fn bad() -> Metrics {
    Metrics {
        public_ok: Some(false),
        ..Metrics::default()
    }
}
fn good() -> Metrics {
    Metrics {
        signal: 80,
        gateway_ms: Some(5.0),
        public_ms: Some(30.0),
        public_ok: Some(true),
        ..Metrics::default()
    }
}
#[test]
fn foreground_beats_saver_and_limit() {
    let s = Settings { data_saver: true, ..Settings::default() };
    assert!(app_allowed(&s, true, "browser", Some("browser"), u64::MAX));
    assert!(!app_allowed(&s, true, "browser", None, 0));
    assert!(app_allowed(&s, false, "browser", None, u64::MAX));
}
#[test]
fn allow_list_does_not_override_background_quota() {
    let mut s = Settings::default();
    s.allow_list.insert("chat".into());
    assert!(app_allowed(&s, true, "chat", None, 0));
    assert!(!app_allowed(&s, true, "chat", None, s.limit_bytes));
    s.limit_bytes = 0;
    assert!(app_allowed(&s, true, "chat", None, u64::MAX));
}
#[test]
fn saver_off_still_respects_explicit_quota() {
    let s = Settings {
        data_saver: false,
        ..Settings::default()
    };
    assert!(app_allowed(&s, true, "x", None, 0));
    assert!(!app_allowed(&s, true, "x", None, s.limit_bytes));
}
#[test]
fn nm_unknown_is_conservative() {
    for n in [0, 1, 3, 99] {
        assert!(metered(n));
    }
    for n in [2, 4] {
        assert!(!metered(n));
    }
}
#[test]
fn switching_requires_consecutive_bad_and_confirmed_action() {
    let s = settings();
    let mut h = Switcher::default();
    assert_eq!(h.observe(0, &s, Some(bad()), true), None);
    assert_eq!(h.observe(5, &s, Some(good()), true), None);
    assert_eq!(h.observe(10, &s, Some(bad()), true), None);
    assert_eq!(h.observe(15, &s, Some(bad()), true), None);
    assert_eq!(h.observe(20, &s, Some(bad()), true), Some(Route::Mobile));
    assert!(!h.mobile_owned); // Failed adapter action must not update route state.
    assert_eq!(h.observe(25, &s, Some(bad()), true), Some(Route::Mobile));
    h.commit(25, Route::Mobile);
    assert!(h.mobile_owned);
}
#[test]
fn recovery_obeys_cooldown_and_good_threshold() {
    let s = settings();
    let mut h = Switcher::default();
    h.commit(0, Route::Mobile);
    for n in 0..25 {
        assert_eq!(h.observe(n, &s, Some(good()), true), None);
    }
    assert_eq!(h.observe(120, &s, Some(good()), true), Some(Route::Wifi));
    h.commit(120, Route::Wifi);
    for n in 121..125 {
        assert_eq!(h.observe(n, &s, Some(bad()), true), None);
    }
}
#[test]
fn capped_and_disabled_never_activate_mobile_and_release_owned_route() {
    for s in [
        Settings {
            smart_switching: true,
            ..Settings::default()
        },
        Settings::default(),
    ] {
        let mut h = Switcher::default();
        for n in 0..20 {
            assert_eq!(h.observe(n * 100, &s, Some(bad()), true), None);
        }
        h.commit(100, Route::Mobile);
        assert_eq!(h.observe(101, &s, Some(bad()), true), Some(Route::Wifi));
    }
}
#[test]
fn consent_and_available_adapter_required() {
    let s = Settings {
        allow_capped: true,
        smart_switching: true,
        ..Settings::default()
    };
    let mut h = Switcher::default();
    for n in 0..10 {
        assert_eq!(h.observe(n, &s, Some(bad()), false), None);
    }
    for n in 10..12 {
        assert_eq!(h.observe(n, &s, Some(bad()), true), None);
    }
    assert_eq!(h.observe(12, &s, Some(bad()), true), Some(Route::Mobile));
}
#[test]
fn missing_metrics_reset_streak_and_icmp_block_alone_is_not_failure() {
    let s = settings();
    let mut h = Switcher::default();
    h.observe(0, &s, Some(bad()), true);
    h.observe(5, &s, Some(bad()), true);
    h.observe(10, &s, None, true);
    assert_eq!(h.observe(15, &s, Some(bad()), true), None);
    let mut m = good();
    m.loss_percent = 100.0;
    m.gateway_ms = None;
    m.signal = 0;
    assert!(!m.bad(&s));
    assert!(!m.recovered(&s));
    m.public_ok = None;
    assert!(!m.bad(&s));
}
#[test]
fn captive_portal_and_latency_trigger_failure() {
    let s = settings();
    let mut m = good();
    m.captive = true;
    assert!(m.bad(&s));
    m.captive = false;
    m.public_ms = Some(1000.0);
    assert!(m.bad(&s));
    m.public_ms = Some(500.0);
    assert!(!m.bad(&s));
    assert!(!m.recovered(&s));
}
#[test]
fn usage_deltas_restarts_and_month_boundary() {
    let mut u = Usage::default();
    let raw = |n| BTreeMap::from([("a".into(), n)]);
    u.sample("2026-01", &raw(100));
    u.sample("2026-01", &raw(150));
    assert_eq!(u.totals["a"], 150);
    let json = serde_json::to_string(&u).unwrap();
    u = serde_json::from_str(&json).unwrap();
    u.sample("2026-01", &raw(150));
    assert_eq!(u.totals["a"], 150);
    u.sample("2026-01", &raw(10));
    assert_eq!(u.totals["a"], 160);
    assert_eq!(u.warnings(100), vec!["a"]);
    assert!(u.warnings(100).is_empty());
    u.sample("2026-02", &raw(30));
    assert_eq!(u.totals["a"], 20);
    assert!(u.warned.is_empty());
}
#[test]
fn settings_reject_invalid_thresholds_urls_and_unknown_fields() {
    let mut s = Settings::default();
    s.thresholds.bad_samples = 0;
    assert!(s.validate().is_err());
    s.thresholds.bad_samples = 1;
    s.endpoint = "file:///etc/passwd".into();
    assert!(s.validate().is_err());
    s.endpoint = "https://user:pass@localhost/".into();
    assert!(s.validate().is_err());
    assert!(serde_json::from_str::<Settings>(r#"{"unrecognized":true}"#).is_err());
}
#[test]
fn probe_parsers_distinguish_portal_from_connectivity() {
    use fp6_netpolicy::nm::{parse_http, parse_ping};
    assert_eq!(parse_ping("3 packets transmitted, 2 received, 33.3333% packet loss, time 2ms\nrtt min/avg/max/mdev = 1.0/4.5/8.0/1.0 ms"), Some((33.3333, Some(4.5))));
    assert_eq!(
        parse_http("HTTP/1.1 204 No Content\r\n\r\n\nNETPOLICY_RESULT 204 0.5"),
        (true, Some(500.0))
    );
    assert!(!parse_http("HTTP/1.1 200 OK\r\n\r\nSign in\nNETPOLICY_RESULT 200 0.1").0);
    assert!(
        parse_http("HTTP/1.1 200 OK\r\n\r\nNetworkManager is online\nNETPOLICY_RESULT 200 0.1").0
    );
    assert!(!parse_http("HTTP/1.1 302 Found\r\n\r\n\nNETPOLICY_RESULT 302 0.1").0);
}
