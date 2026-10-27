use fp6_selftest::{
    base,
    config::Config,
    files,
    process::{self, Failure},
    report::{self, Check, Report, Status},
    runner,
};
use std::{
    collections::HashSet,
    fs,
    os::unix::fs::{symlink, PermissionsExt},
    process::Command,
    time::{Duration, Instant},
};

#[test]
fn shipped_configuration_is_valid_and_rejects_writes() {
    let cfg = Config::parse(runner::DEFAULT_CONFIG).unwrap();
    assert_eq!(cfg.probe.len(), 17);
    assert!(Config::parse(
        &runner::DEFAULT_CONFIG.replace("method = \"Status\"", "method = \"Start\"")
    )
    .is_err());
    assert!(Config::parse(
        &runner::DEFAULT_CONFIG.replace("id = \"webshare\"", "id = \"quickshare\"")
    )
    .is_err());
}
#[test]
fn report_severity_and_redaction() {
    for (status, exit) in [
        (Status::Skip, 0),
        (Status::Pass, 0),
        (Status::Warn, 1),
        (Status::Fail, 2),
    ] {
        assert_eq!(status.exit_code(), exit);
    }
    let check = Check::new("fixture", Status::Warn, "synthetic@example.invalid +999123456789 123456789012345 192.0.2.10 aa:bb:cc:dd:ee:ff /home/synthetic/file 2001:db8::1");
    let report = Report::new("test", vec![Check::new("ok", Status::Pass, "Okay"), check]);
    assert_eq!(report.status, Status::Warn);
    let out = report.json();
    for sensitive in [
        "synthetic",
        "999123",
        "123456789012345",
        "192.0.2",
        "aa:bb",
        "2001:db8",
    ] {
        assert!(!out.contains(sensitive));
    }
}
#[test]
fn private_reports_never_overwrite_or_follow_symlinks() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("reports");
    let report = Report::new(
        "fixture",
        vec![Check::new("fixture", Status::Pass, "Synthetic result")],
    );
    report::save(&report, &dir).unwrap();
    assert_eq!(
        fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let entries: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(entries.len(), 2);
    for p in entries {
        assert_eq!(fs::metadata(p).unwrap().permissions().mode() & 0o777, 0o600);
    }
    assert!(report::save(&report, &dir).is_err());
    symlink(&dir, temp.path().join("link")).unwrap();
    assert!(report::save(&report, &temp.path().join("link")).is_err());
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(report::save(&report, &dir).is_err());
}
#[test]
fn schema_fixtures_and_dry_run_do_not_write() {
    let good = std::path::PathBuf::from("tests/fixtures/schemas/good");
    let bad = std::path::PathBuf::from("tests/fixtures/schemas/bad");
    assert_eq!(
        files::inspect_schema(include_str!(
            "fixtures/schemas/good/com.fairphone6.fixture.gschema.xml"
        )),
        Ok(1)
    );
    assert!(files::inspect_schema(include_str!(
        "fixtures/schemas/bad/com.fairphone6.fixture.gschema.xml"
    ))
    .is_err());
    assert!(files::inspect_schema("<bad").is_err());
    assert_eq!(
        files::schemas(std::slice::from_ref(&good)).status,
        Status::Pass
    );
    assert!(!good.join("gschemas.compiled").exists());
    assert_eq!(files::schemas(&[bad]).status, Status::Fail);
    let malformed = tempfile::tempdir().unwrap();
    fs::write(malformed.path().join("test.gschema.xml"), "<schemalist><schema id='com.fairphone6.fixture'><key name='test' type='b'><default>bogus</default></key></schema></schemalist>").unwrap();
    assert_eq!(
        files::schemas(&[malformed.path().into()]).status,
        Status::Fail
    );
}
#[test]
fn touch_desktops_validate_exec_env_and_icons() {
    let temp = tempfile::tempdir().unwrap();
    for name in ["fixture-app", "env"] {
        let path = temp.path().join(name);
        fs::write(&path, "synthetic executable fixture").unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let search = vec![temp.path().into()];
    let icons = HashSet::from(["fixture-icon".into()]);
    let good = include_str!("fixtures/desktops/good.desktop");
    assert_eq!(files::inspect_desktop(good, &search, &icons), Ok(true));
    assert!(
        files::inspect_desktop(&good.replace("fixture-app", "missing-app"), &search, &icons)
            .is_err()
    );
    assert!(files::inspect_desktop(
        &good.replace("fixture-icon", "missing-icon"),
        &search,
        &icons
    )
    .is_err());
    assert_eq!(
        files::inspect_desktop(
            &good.replace("Exec=fixture-app", "Exec=env A=B fixture-app"),
            &search,
            &icons
        ),
        Ok(true)
    );
    assert!(files::inspect_desktop(
        &good.replace("Exec=fixture-app", "Exec=env A=B missing"),
        &search,
        &icons
    )
    .is_err());
    assert_eq!(
        files::inspect_desktop(
            &good.replace("X-Lomiri-Touch=true", "X-Lomiri-Touch=false"),
            &search,
            &icons
        ),
        Ok(false)
    );
}
#[test]
fn journal_and_memory_reduce_to_safe_counts() {
    let (status, text) = base::journal_summary("{\"_SYSTEMD_UNIT\":\"synthetic@example.invalid.service\",\"MESSAGE\":\"+999123456789\"}\n{\"_SYSTEMD_USER_UNIT\":\"lomiri.service\"}", false);
    assert_eq!(status, Status::Warn);
    assert!(text.contains("other=1") && text.contains("shell=1"));
    assert!(!text.contains("synthetic") && !text.contains("999"));
    assert_eq!(
        base::memory_summary("MemTotal: 1000 kB\nMemAvailable: 40 kB").0,
        Status::Fail
    );
    assert_eq!(
        base::memory_summary("MemTotal: 1000 kB\nMemAvailable: 500 kB").0,
        Status::Pass
    );
}
#[test]
fn deadline_kills_descendants_and_output_is_bounded() {
    let temp = tempfile::tempdir().unwrap();
    let marker = temp.path().join("unexpected");
    let mut cmd = Command::new("sh");
    cmd.args(["-c", "(sleep 1; touch \"$1\") & wait", "fixture"])
        .arg(&marker);
    let start = Instant::now();
    assert!(matches!(
        process::run(cmd, Duration::from_millis(80)),
        Err(Failure::Timeout)
    ));
    assert!(start.elapsed() < Duration::from_secs(1));
    std::thread::sleep(Duration::from_millis(1100));
    assert!(!marker.exists());
    let mut cmd = Command::new("sh");
    cmd.args(["-c", "head -c 3000000 /dev/zero"]);
    assert!(matches!(
        process::run(cmd, Duration::from_secs(3)),
        Err(Failure::Output)
    ));
}

#[test]
fn nonlocal_dbus_addresses_are_rejected() {
    assert!(fp6_selftest::dbus::local_address(
        "unix:path=/tmp/synthetic"
    ));
    assert!(fp6_selftest::dbus::local_address("unix:abstract=synthetic"));
    for address in [
        "",
        "tcp:host=example.invalid,port=1234",
        "autolaunch:",
        "unix:path=/tmp/synthetic;tcp:host=example.invalid,port=1234",
    ] {
        assert!(!fp6_selftest::dbus::local_address(address));
    }
}

#[test]
fn parent_runner_reports_full_quick_and_system_scopes_with_synthetic_workers() {
    let temp = tempfile::tempdir().unwrap();
    let script = temp.path().join("worker");
    let shell = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|p| p.join("sh"))
        .find(|p| p.is_file())
        .unwrap();
    fs::write(&script, format!("#!{}\nprintf '{{\"id\":\"%s\",\"status\":\"PASS\",\"detail\":\"Synthetic result\"}}' \"$2\"\n", shell.display())).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    let cfg = temp.path().join("empty.toml");
    fs::write(&cfg, "probe = []").unwrap();
    let full = runner::run(&script, Some(&cfg), false, false, Duration::from_secs(1));
    assert_eq!(full.status, Status::Pass);
    assert_eq!(full.checks.len(), 22);
    let quick = runner::run(&script, Some(&cfg), true, false, Duration::from_secs(1));
    assert_eq!(
        quick
            .checks
            .iter()
            .filter(|c| c.status == Status::Skip)
            .count(),
        2
    );
    let system = runner::run(&script, Some(&cfg), true, true, Duration::from_secs(1));
    assert_eq!(system.mode, "system");
    assert!(!system
        .checks
        .iter()
        .any(|c| c.id == "lomiri" || c.id == "failed-user"));
    fs::write(&script, format!("#!{}\nprintf '{{\"id\":\"wrong\",\"status\":\"PASS\",\"detail\":\"Synthetic result\"}}'\n", shell.display())).unwrap();
    assert_eq!(
        runner::run(&script, Some(&cfg), true, true, Duration::from_secs(1)).status,
        Status::Fail
    );
}

#[test]
fn inherited_array_keys_are_rejected_even_from_non_fairphone_parent() {
    let xml = "<schemalist><schema id='org.example.parent'><key name='bad' type='ax'><default>[]</default></key></schema><schema id='com.fairphone6.child' extends='org.example.parent'/></schemalist>";
    assert!(files::inspect_schema(xml).is_err());
    assert_eq!(
        files::inspect_schema(&xml.replace("type='ax'", "type='as'")),
        Ok(1)
    );
}
