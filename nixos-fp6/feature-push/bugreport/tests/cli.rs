use std::process::Command;
fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_fp6-bugreport"))
        .args(args)
        .output()
        .unwrap()
}
#[test]
fn preview_validates_without_reading_host_data_or_writing_reports() {
    let out = cli(&["--preview", "--since", "2h", "--sections", "system,power"]);
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("2h"));
    assert!(text.contains("system"));
    assert!(text.contains("power"));
    assert!(text.contains("\"screenshot\": false"));
}
#[test]
fn malformed_options_and_unconsented_capture_are_rejected() {
    for args in [
        vec!["--preview", "--since", "1🦀"],
        vec!["--preview", "--since", "8d"],
        vec!["--preview", "--sections", "system,system"],
        vec!["--preview", "--sections", "../private"],
        vec!["--preview", "--sections", ""],
        vec!["--preview", "--screenshot"],
    ] {
        assert!(!cli(&args).status.success(), "accepted {args:?}");
    }
    assert!(cli(&["--preview", "--capture-now", "--screenshot"])
        .status
        .success());
}
