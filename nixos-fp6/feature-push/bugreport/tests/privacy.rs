use fp6_bugreport::{
    bundle::{capture_screenshot, write_bundle},
    collect::{collect, privileged_collect, Entry, Sources},
    config::Options,
    runner::{Output, Runner},
};
use std::{
    cell::RefCell,
    io::Read,
    os::unix::fs::{symlink, PermissionsExt},
};

#[derive(Default)]
struct Synthetic {
    calls: RefCell<Vec<String>>,
}
impl Runner for Synthetic {
    fn run(&self, program: &str, args: &[&str]) -> Output {
        self.calls
            .borrow_mut()
            .push(format!("{program} {}", args.join(" ")));
        Output { status: "ok".into(), text: "GPU fault -5\nphone=+999 123 456 789\nSSID=Synthetic Cafe\npeer=2001:db8::9\npassword=test-secret\n".into() }
    }
}
impl Sources for Synthetic {
    fn read(&self, path: &str) -> Output {
        self.calls.borrow_mut().push(path.into());
        Output {
            status: "ok".into(),
            text: "synthetic\n".into(),
        }
    }
    fn children(&self, _: &str) -> Vec<String> {
        vec!["synthetic0".into()]
    }
}
#[test]
fn disabled_sections_are_never_collected() {
    let source = Synthetic::default();
    let options = Options {
        sections: vec!["system".into()],
        ..Options::default()
    };
    let entries = collect(&options, &source).unwrap();
    assert_eq!(entries.len(), 3);
    assert!(source
        .calls
        .borrow()
        .iter()
        .all(|c| c.starts_with("/etc/") || c.starts_with("/proc/")));
}
#[test]
fn windows_are_passed_without_a_shell_and_core_contents_never_requested() {
    let source = Synthetic::default();
    let options = Options {
        since: "2h".into(),
        ..Options::default()
    };
    let entries = collect(&options, &source).unwrap();
    assert_eq!(entries.len(), 28);
    assert!(source
        .calls
        .borrow()
        .iter()
        .filter(|s| s.starts_with("journalctl"))
        .all(|s| s.contains("7200 seconds ago")));
    let calls = source.calls.borrow();
    let core = calls.iter().find(|s| s.starts_with("coredumpctl")).unwrap();
    assert!(core.ends_with(" list"));
    assert!(!core.contains("dump "));
    for entry in entries {
        assert!(!entry.text.contains("test-secret"));
        assert!(!entry.text.contains("Synthetic Cafe"));
    }
}
#[test]
fn helper_rejects_unbounded_or_arbitrary_requests_before_running() {
    let source = Synthetic::default();
    for section in ["/etc/shadow", "--help", "storage", "dump"] {
        assert!(privileged_collect(section, "15m", &source).is_err());
    }
    assert!(privileged_collect("journal", "--all", &source).is_err());
    assert!(source.calls.borrow().is_empty());
    let entry = privileged_collect("journal", "15m", &source).unwrap();
    assert!(!entry.text.contains("test-secret"));
}
#[test]
fn bundle_has_private_permissions_safe_members_and_only_redacted_text() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("reports");
    let entries = vec![Entry {
        name: "journal/user.txt".into(),
        status: "ok".into(),
        text: "contact=+999 123 456 789\npassword=synthetic-secret\nGPU fault -5\n".into(),
    }];
    let path = write_bundle(&root, &Options::default(), &entries, None).unwrap();
    for p in [&path, &path.parent().unwrap().join("summary.txt")] {
        assert_eq!(p.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    }
    assert_eq!(root.metadata().unwrap().permissions().mode() & 0o777, 0o700);
    assert_eq!(
        path.parent()
            .unwrap()
            .metadata()
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    let reader = zstd::Decoder::new(std::fs::File::open(&path).unwrap()).unwrap();
    let mut archive = tar::Archive::new(reader);
    let mut names = Vec::new();
    for entry in archive.entries().unwrap() {
        let mut e = entry.unwrap();
        names.push(e.path().unwrap().to_string_lossy().into_owned());
        assert_eq!(e.header().mode().unwrap(), 0o600);
        let mut text = String::new();
        e.read_to_string(&mut text).unwrap();
        assert!(!text.contains("synthetic-secret"));
        assert!(!text.contains("+999"));
    }
    assert_eq!(names, ["summary.txt", "journal/user.txt"]);
    let next = write_bundle(&root, &Options::default(), &entries, None).unwrap();
    assert_ne!(next, path);
}
#[test]
fn unsafe_outputs_and_screenshot_without_consent_are_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("linked");
    symlink(tmp.path(), &root).unwrap();
    assert!(write_bundle(&root, &Options::default(), &[], None).is_err());
    let root = tmp.path().join("private");
    assert!(write_bundle(&root, &Options::default(), &[], Some(b"not consented")).is_err());
    let bad = Entry {
        name: "../escape.txt".into(),
        status: "ok".into(),
        text: "test".into(),
    };
    assert!(write_bundle(&root, &Options::default(), &[bad], None).is_err());
    assert_eq!(
        std::fs::read_dir(&root).unwrap().count(),
        0,
        "failed writes must clean up"
    );
}
#[test]
fn absent_screenshot_api_is_explicit_and_does_not_invent_an_image() {
    let source = Synthetic::default();
    let (entry, image) = capture_screenshot(&source);
    assert!(entry.status.starts_with("unavailable:"));
    assert!(image.is_none());
    let calls = source.calls.borrow();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].contains("--session"));
}

#[test]
fn successful_screenshot_is_only_accepted_from_the_requested_private_path() {
    struct Camera;
    impl Runner for Camera {
        fn run(&self, _: &str, args: &[&str]) -> Output {
            let path = args.last().unwrap();
            std::fs::write(path, b"\x89PNG\r\n\x1a\nsynthetic-fixture").unwrap();
            Output {
                status: "ok".into(),
                text: "(true, 'ignored-return-path')".into(),
            }
        }
    }
    let (entry, bytes) = capture_screenshot(&Camera);
    assert!(entry.status.starts_with("ok:"));
    assert!(bytes.unwrap().starts_with(b"\x89PNG"));
}

#[test]
fn subprocess_limits_and_failures_are_visible_without_stderr() {
    use fp6_bugreport::runner::run_bounded;
    use std::time::{Duration, Instant};
    let shell = option_env!("FP6_TEST_SH").unwrap_or("/bin/sh");
    let out = run_bounded(
        shell,
        &[
            "-c",
            "printf 'synthetic stdout'; printf 'secret stderr' >&2; exit 7",
        ],
        Duration::from_secs(2),
    );
    assert_eq!(out.text, "synthetic stdout");
    assert_eq!(out.status, "unavailable: exit 7");
    let start = Instant::now();
    let out = run_bounded(
        shell,
        &["-c", "while :; do :; done"],
        Duration::from_millis(80),
    );
    assert!(out.status.contains("timed out"));
    assert!(start.elapsed() < Duration::from_secs(5));
    let out = run_bounded(
        shell,
        &[
            "-c",
            "while :; do printf 'synthetic bounded output line\\n'; done",
        ],
        Duration::from_millis(500),
    );
    assert!(out.text.len() <= fp6_bugreport::runner::LIMIT);
    assert!(out.status.contains("timed out"));
}
