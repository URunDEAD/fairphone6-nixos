use fp6_netpolicy::helper::{unavailable_reason, Backoff, Snapshot, NO_SOCKET_MATCH};
use std::time::Duration;

#[test]
fn backoff_doubles_to_ceiling_and_logs_only_escalations() {
    let mut b = Backoff::new(Duration::from_secs(5), Duration::from_secs(600));
    let mut delays = vec![];
    let mut logs = 0;
    for _ in 0..20 {
        let (d, log) = b.failure("nft: boom");
        delays.push(d.as_secs());
        logs += log as usize;
    }
    assert_eq!(&delays[..9], &[5, 10, 20, 40, 80, 160, 320, 600, 600]);
    assert!(delays[7..].iter().all(|&d| d == 600));
    // 5..320 and the first 600: eight lines for twenty failures.
    assert_eq!(logs, 8);
    // A different error is logged even at the ceiling, and keeps the delay.
    assert_eq!(b.failure("nft: other"), (Duration::from_secs(600), true));
    assert_eq!(b.failure("nft: other"), (Duration::from_secs(600), false));
}

#[test]
fn backoff_resets_on_success() {
    let mut b = Backoff::new(Duration::from_secs(5), Duration::from_secs(600));
    assert!(!b.success());
    b.failure("x");
    b.failure("x");
    assert!(b.failing());
    assert!(b.success());
    assert!(!b.failing());
    assert_eq!(b.failure("x"), (Duration::from_secs(5), true));
}

#[test]
fn missing_nft_socket_is_classified() {
    let kernel = "nft: /dev/stdin:12:54-76: Error: Could not process rule: No such file or directory\n\
        add rule inet fp6_netpolicy in iifname \"rmnet_data3\" socket cgroupv2 level 2 \"system.slice/fp6-netpolicy-helper.service\" return\n\
        ^^^^^^^^^^^^^^^^^^^^^^^";
    assert_eq!(unavailable_reason(kernel), Some(NO_SOCKET_MATCH));
    assert_eq!(
        unavailable_reason("nft: Error: syntax error, unexpected newline"),
        None
    );
    assert_eq!(
        unavailable_reason("read /sys/fs/cgroup/x: No such file or directory"),
        None
    );
}

#[test]
fn snapshot_without_unavailable_field_still_decodes() {
    let mut v = serde_json::to_value(Snapshot::default()).unwrap();
    v.as_object_mut().unwrap().remove("unavailable");
    let s: Snapshot = serde_json::from_value(v).unwrap();
    assert!(s.unavailable.is_empty());
}
