//! fp6-notify-reply: sends a freedesktop notification that offers an inline
//! reply (action "inline-reply", as KDE Plasma / Telegram Desktop use it) and
//! prints what the user typed (org.freedesktop.Notifications.NotificationReplied).
//!
//!   fp6-notify-reply                       # synthetic "Test Contact" message
//!   fp6-notify-reply --check               # does the server offer inline-reply?
//!   fp6-notify-reply --json --wait 300 --summary "Test Contact" --body "Hi"
//!
//! Exit status: 0 replied, 1 closed (or the server's "default" action) without
//! a reply, 2 the server lacks the "inline-reply" capability, 3 timed out,
//! 4 bus or usage error.

use std::collections::HashMap;
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Duration;

use zbus::blocking::{Connection, MessageIterator};
use zbus::zvariant::Value;
use zbus::MatchRule;

const DEST: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
const IFACE: &str = "org.freedesktop.Notifications";

#[derive(Debug, Clone, PartialEq)]
struct Options {
    app: String,
    icon: String,
    summary: String,
    body: String,
    label: String,
    placeholder: String,
    submit: Option<String>,
    wait_secs: u64,
    json: bool,
    check_only: bool,
    skip_capability_check: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            app: "fp6-notify-reply".into(),
            icon: "message".into(),
            summary: "Test Contact".into(),
            body: "Test message from +999 555 0100".into(),
            label: "Reply".into(),
            placeholder: "Reply to Test Contact".into(),
            submit: None,
            wait_secs: 120,
            json: false,
            check_only: false,
            skip_capability_check: false,
        }
    }
}

const USAGE: &str = "usage: fp6-notify-reply [--check] [--json] [--wait SECS (0: forever)]
       [--summary TEXT] [--body TEXT] [--app NAME] [--icon NAME]
       [--label TEXT] [--placeholder TEXT] [--submit TEXT] [--no-capability-check]";

fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<Options, String> {
    let mut o = Options::default();
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        let mut val = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        match a.as_str() {
            "--app" => o.app = val("--app")?,
            "--icon" => o.icon = val("--icon")?,
            "--summary" => o.summary = val("--summary")?,
            "--body" => o.body = val("--body")?,
            "--label" => o.label = val("--label")?,
            "--placeholder" => o.placeholder = val("--placeholder")?,
            "--submit" => o.submit = Some(val("--submit")?),
            "--wait" => {
                o.wait_secs = val("--wait")?
                    .parse()
                    .map_err(|_| "--wait takes whole seconds".to_string())?
            }
            "--json" => o.json = true,
            "--check" => o.check_only = true,
            "--no-capability-check" => o.skip_capability_check = true,
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(o)
}

/// What came back for our notification.
#[derive(Debug, Clone, PartialEq)]
enum Outcome {
    Replied(String),
    Action(String),
    Closed(u32),
    TimedOut,
}

/// Decodes one signal of the notification interface; None if it is not about `id`.
fn outcome_for(member: &str, msg: &zbus::Message, id: u32) -> Option<Outcome> {
    let body = msg.body();
    match member {
        "NotificationReplied" => {
            let (nid, text): (u32, String) = body.deserialize().ok()?;
            (nid == id).then_some(Outcome::Replied(text))
        }
        "ActionInvoked" => {
            let (nid, action): (u32, String) = body.deserialize().ok()?;
            (nid == id).then_some(Outcome::Action(action))
        }
        "NotificationClosed" => {
            let (nid, reason): (u32, u32) = body.deserialize().ok()?;
            (nid == id).then_some(Outcome::Closed(reason))
        }
        _ => None,
    }
}

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn report(o: &Outcome, id: u32, json: bool) -> String {
    if json {
        match o {
            Outcome::Replied(t) => format!(
                "{{\"id\":{id},\"event\":\"replied\",\"text\":{}}}",
                json_str(t)
            ),
            Outcome::Action(a) => format!(
                "{{\"id\":{id},\"event\":\"action\",\"action\":{}}}",
                json_str(a)
            ),
            Outcome::Closed(r) => format!("{{\"id\":{id},\"event\":\"closed\",\"reason\":{r}}}"),
            Outcome::TimedOut => format!("{{\"id\":{id},\"event\":\"timeout\"}}"),
        }
    } else {
        match o {
            Outcome::Replied(t) => format!("replied ({id}): {t}"),
            Outcome::Action(a) => format!("action ({id}): {a}"),
            Outcome::Closed(r) => {
                let why = match r {
                    1 => "expired",
                    2 => "dismissed",
                    3 => "closed by CloseNotification",
                    _ => "undefined",
                };
                format!("closed ({id}) without a reply: reason {r} ({why})")
            }
            Outcome::TimedOut => format!("no reply ({id}): timed out"),
        }
    }
}

fn exit_code(o: &Outcome) -> u8 {
    match o {
        Outcome::Replied(_) => 0,
        Outcome::Action(_) | Outcome::Closed(_) => 1,
        Outcome::TimedOut => 3,
    }
}

fn capabilities(conn: &Connection) -> zbus::Result<Vec<String>> {
    conn.call_method(Some(DEST), PATH, Some(IFACE), "GetCapabilities", &())?
        .body()
        .deserialize()
}

fn run(o: &Options) -> Result<u8, String> {
    let conn = Connection::session().map_err(|e| format!("session bus: {e}"))?;
    let caps = capabilities(&conn).map_err(|e| format!("GetCapabilities: {e}"))?;
    let has_reply = caps.iter().any(|c| c == "inline-reply");
    if o.check_only {
        if o.json {
            println!("{{\"inline-reply\":{has_reply}}}");
        } else {
            println!(
                "inline-reply: {}",
                if has_reply {
                    "supported"
                } else {
                    "not offered"
                }
            );
        }
        return Ok(if has_reply { 0 } else { 2 });
    }
    if !has_reply && !o.skip_capability_check {
        eprintln!("the notification server does not offer \"inline-reply\" (--no-capability-check to send anyway)");
        return Ok(2);
    }

    // Subscribe before Notify so a quick reply cannot be missed.
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(IFACE)
        .and_then(|b| b.path(PATH))
        .map_err(|e| format!("match rule: {e}"))?
        .build();
    let signals = MessageIterator::for_match_rule(rule, &conn, Some(64))
        .map_err(|e| format!("subscribe: {e}"))?;
    let (tx, rx) = mpsc::channel::<zbus::Message>();
    std::thread::spawn(move || {
        for m in signals.flatten() {
            if tx.send(m).is_err() {
                break;
            }
        }
    });

    let mut hints: HashMap<&str, Value> = HashMap::new();
    hints.insert(
        "x-kde-reply-placeholder-text",
        Value::from(o.placeholder.as_str()),
    );
    if let Some(s) = &o.submit {
        hints.insert("x-kde-reply-submit-button-text", Value::from(s.as_str()));
    }
    hints.insert("urgency", Value::from(1u8));
    let actions = vec!["default", "Open", "inline-reply", o.label.as_str()];
    let id: u32 = conn
        .call_method(
            Some(DEST),
            PATH,
            Some(IFACE),
            "Notify",
            &(
                o.app.as_str(),
                0u32,
                o.icon.as_str(),
                o.summary.as_str(),
                o.body.as_str(),
                actions,
                hints,
                -1i32,
            ),
        )
        .and_then(|r| r.body().deserialize())
        .map_err(|e| format!("Notify: {e}"))?;
    if !o.json {
        eprintln!("sent notification {id}; waiting for a reply...");
    }

    let deadline =
        (o.wait_secs > 0).then(|| std::time::Instant::now() + Duration::from_secs(o.wait_secs));
    let outcome = loop {
        let msg = match deadline {
            Some(d) => {
                let left = d.saturating_duration_since(std::time::Instant::now());
                match rx.recv_timeout(left) {
                    Ok(m) => m,
                    Err(_) => break Outcome::TimedOut,
                }
            }
            None => match rx.recv() {
                Ok(m) => m,
                Err(_) => return Err("signal stream ended".into()),
            },
        };
        let header = msg.header();
        let Some(member) = header.member().map(|m| m.to_string()) else {
            continue;
        };
        match outcome_for(&member, &msg, id) {
            // "default" is a tap on the bubble on some servers; keep waiting
            // for the reply only if the server says nothing else.
            Some(Outcome::Action(a)) if a == "inline-reply" => continue,
            Some(o) => break o,
            None => continue,
        }
    };
    if outcome == Outcome::TimedOut {
        // Tidy up: nobody answered.
        let _ = conn.call_method(Some(DEST), PATH, Some(IFACE), "CloseNotification", &(id,));
    }
    println!("{}", report(&outcome, id, o.json));
    Ok(exit_code(&outcome))
}

fn main() -> ExitCode {
    let opts = match parse_args(std::env::args().skip(1)) {
        Ok(o) => o,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("{e}");
            }
            eprintln!("{USAGE}");
            return ExitCode::from(if e.is_empty() { 0 } else { 4 });
        }
    };
    match run(&opts) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("fp6-notify-reply: {e}");
            ExitCode::from(4)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn defaults_are_synthetic() {
        let o = parse_args(args(&[])).unwrap();
        assert_eq!(o.summary, "Test Contact");
        assert!(o.body.contains("+999"));
        assert_eq!(o.wait_secs, 120);
        assert!(!o.json && !o.check_only);
    }

    #[test]
    fn parses_options() {
        let o = parse_args(args(&[
            "--summary",
            "Test Contact",
            "--body",
            "hi",
            "--wait",
            "0",
            "--json",
            "--submit",
            "Send",
            "--label",
            "Answer",
            "--no-capability-check",
        ]))
        .unwrap();
        assert_eq!(o.body, "hi");
        assert_eq!(o.wait_secs, 0);
        assert_eq!(o.submit.as_deref(), Some("Send"));
        assert_eq!(o.label, "Answer");
        assert!(o.json && o.skip_capability_check);
    }

    #[test]
    fn rejects_bad_arguments() {
        assert!(parse_args(args(&["--wait", "soon"])).is_err());
        assert!(parse_args(args(&["--body"])).is_err());
        assert!(parse_args(args(&["--frobnicate"])).is_err());
        assert_eq!(parse_args(args(&["--help"])), Err(String::new()));
    }

    #[test]
    fn reports_and_exit_codes() {
        let r = Outcome::Replied("On my way \"now\"\n".into());
        assert_eq!(
            report(&r, 7, true),
            r#"{"id":7,"event":"replied","text":"On my way \"now\"\n"}"#
        );
        assert_eq!(report(&r, 7, false), "replied (7): On my way \"now\"\n");
        assert_eq!(exit_code(&r), 0);
        assert_eq!(
            report(&Outcome::Closed(2), 7, false),
            "closed (7) without a reply: reason 2 (dismissed)"
        );
        assert_eq!(exit_code(&Outcome::Closed(1)), 1);
        assert_eq!(
            report(&Outcome::TimedOut, 3, true),
            r#"{"id":3,"event":"timeout"}"#
        );
        assert_eq!(exit_code(&Outcome::TimedOut), 3);
        assert_eq!(json_str("\u{1}"), "\"\\u0001\"");
    }
}
