use fp6_selftest::{
    base,
    config::Config,
    dbus, files,
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
fn hidden_desktops_need_no_icon_and_failures_are_named() {
    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("bin");
    let apps = temp.path().join("share/applications");
    let icons = temp.path().join("share/icons/hicolor/scalable/apps");
    for d in [&bin, &apps, &icons] {
        fs::create_dir_all(d).unwrap();
    }
    let exe = bin.join("fixture-app");
    fs::write(&exe, "synthetic executable fixture").unwrap();
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(icons.join("fixture-icon.svg"), "<svg/>").unwrap();
    let search = vec![bin.clone()];
    let none = HashSet::new();
    let good = include_str!("fixtures/desktops/good.desktop");
    let no_icon = good.replace("Icon=fixture-icon", "Icon=");
    assert!(files::inspect_desktop(&no_icon, &search, &none).is_err());
    for hidden in ["NoDisplay=true", "OnlyShowIn=Old;", "NotShowIn=Lomiri;"] {
        let text = format!("{no_icon}{hidden}\n");
        assert_eq!(
            files::inspect_desktop(&text, &search, &none),
            Ok(true),
            "{hidden}"
        );
    }
    let shown = format!("{no_icon}OnlyShowIn=Lomiri;\n");
    assert!(files::inspect_desktop(&shown, &search, &none).is_err());
    let deleted = format!("{no_icon}Hidden=true\n");
    assert_eq!(files::inspect_desktop(&deleted, &search, &none), Ok(false));
    // Hidden entries still need a working Exec.
    let broken = format!(
        "{}NoDisplay=true\n",
        good.replace("fixture-app", "missing-app")
    );
    assert!(files::inspect_desktop(&broken, &search, &none).is_err());

    fs::write(apps.join("good-app.desktop"), good).unwrap();
    fs::write(apps.join("blank-icon.desktop"), &no_icon).unwrap();
    fs::write(
        apps.join("helper.desktop"),
        format!("{no_icon}NoDisplay=true\n"),
    )
    .unwrap();
    let check = files::desktops(&[temp.path().join("share")], &search);
    assert_eq!(check.status, Status::Fail);
    assert!(
        check.detail.starts_with(
            "3 touch desktop entries checked; 1 have invalid Exec or Icon: blank-icon ("
        ),
        "{}",
        check.detail
    );
    assert!(!check.detail.contains("good-app") && !check.detail.contains("helper"));
}
#[test]
fn stopped_services_on_demand_pass_wanted_or_restarting_warn() {
    let st = |active: &str, sub: &str, wanted: &str| {
        format!("LoadState=loaded\nActiveState={active}\nSubState={sub}\nWantedBy={wanted}\nRequiredBy=\nUpheldBy=\n")
    };
    use dbus::stopped_unit_status as s;
    assert_eq!(s(&st("inactive", "dead", ""), false).0, Status::Pass);
    assert_eq!(s(&st("inactive", "dead", ""), true).0, Status::Pass);
    assert_eq!(
        s(&st("inactive", "dead", "graphical-session.target"), true).0,
        Status::Pass
    );
    assert_eq!(
        s(&st("inactive", "dead", "graphical-session.target"), false).0,
        Status::Warn
    );
    assert_eq!(
        s(
            &st("activating", "auto-restart", "graphical-session.target"),
            false
        )
        .0,
        Status::Warn
    );
    assert_eq!(
        s(&st("activating", "auto-restart", ""), true).0,
        Status::Warn
    );
    assert_eq!(s(&st("failed", "failed", ""), true).0, Status::Fail);
    assert_eq!(s(&st("active", "running", ""), false).0, Status::Fail);
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
#[test]
fn keyboard_names_ignore_nix_wrappers() {
    for argv0 in [
        "/nix/store/x-maliit-keyboard-2.3.1/bin/maliit-keyboard",
        "/nix/store/x-maliit-keyboard-2.3.1/bin/.maliit-keyboard-wrapped",
        "..maliit-keyboard-wrapped-wrapped",
        "maliit-server",
    ] {
        assert!(base::is_keyboard(argv0), "{argv0}");
    }
    for argv0 in ["", "maliit-keyboard-settings", "/bin/sh", "keyboard"] {
        assert!(!base::is_keyboard(argv0), "{argv0}");
    }
}
#[test]
fn exec_search_covers_session_profiles() {
    let dirs = files::exec_search_dirs();
    for d in ["/run/wrappers/bin", "/run/current-system/sw/bin"] {
        assert!(dirs.iter().any(|p| p == std::path::Path::new(d)), "{d}");
    }
}
#[test]
fn schema_data_dir_list_keeps_absolute_paths() {
    assert_eq!(
        files::schema_data_dirs_from("/nix/store/a-x/share\n\nrelative/share\n /nix/store/b-y/share \n"),
        vec![
            std::path::PathBuf::from("/nix/store/a-x/share"),
            std::path::PathBuf::from("/nix/store/b-y/share")
        ]
    );
}
