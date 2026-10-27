//! fp6-accessibility-audio: mirrors the mono-audio / audio-balance keys of
//! GSettings com.fairphone6.accessibility into WirePlumber's settings
//! (fp6.a11y.mono-audio, fp6.a11y.audio-balance), where the fp6/a11y-audio.lua
//! script applies them to every stereo output as an audioconvert filter graph.
//!
//! The values go to both the live `sm-settings` metadata and
//! `persistent-sm-settings` (WirePlumber stores that one, so a WirePlumber
//! restart keeps them). pw-metadata is used rather than `wpctl settings`
//! because wpctl cannot take a negative value (its option parser reads
//! "-0.5" as an option).
//!
//! Event-driven: GSettings `changed` signals only; a failed write (PipeWire not
//! up yet) is retried every 5 s until it succeeds.
use gio::prelude::*;
use std::cell::RefCell;
use std::process::Command;
use std::rc::Rc;

const SCHEMA: &str = "com.fairphone6.accessibility";
const METADATA: [&str; 2] = ["sm-settings", "persistent-sm-settings"];

/// The WirePlumber settings for the GSettings values, as (key, JSON value).
pub fn wp_values(mono: bool, balance: f64) -> Vec<(&'static str, String)> {
    let b = if balance.is_finite() { balance.clamp(-1.0, 1.0) } else { 0.0 };
    // Snap tiny values to 0 so "centre" removes the graph.
    let b = if b.abs() < 0.005 { 0.0 } else { b };
    vec![
        ("fp6.a11y.mono-audio", if mono { "true".into() } else { "false".into() }),
        ("fp6.a11y.audio-balance", format!("{b:.3}")),
    ]
}

/// The pw-metadata argument lists that set `values` in both metadata objects.
pub fn commands(values: &[(&'static str, String)]) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    for md in METADATA {
        for (k, v) in values {
            out.push(vec![
                "-n".into(), md.into(), "0".into(), (*k).into(), "--".into(), v.clone(),
                "Spa:String:JSON".into(),
            ]);
        }
    }
    out
}

fn apply(settings: &gio::Settings) -> bool {
    let values = wp_values(settings.boolean("mono-audio"), settings.double("audio-balance"));
    let mut ok = true;
    for args in commands(&values) {
        match Command::new("pw-metadata").args(&args).output() {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                ok = false;
                eprintln!("pw-metadata {:?}: {}", args, String::from_utf8_lossy(&o.stderr).trim());
            }
            Err(e) => {
                ok = false;
                eprintln!("pw-metadata: {e}");
            }
        }
    }
    if ok {
        eprintln!("applied {values:?}");
    }
    ok
}

fn run() {
    let settings = gio::Settings::new(SCHEMA);
    let retry: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    let schedule = {
        let settings = settings.clone();
        let retry = retry.clone();
        move || {
            if let Some(id) = retry.borrow_mut().take() {
                id.remove();
            }
            if !apply(&settings) {
                let s = settings.clone();
                let r = retry.clone();
                let id = glib::timeout_add_seconds_local(5, move || {
                    if apply(&s) {
                        r.borrow_mut().take();
                        glib::ControlFlow::Break
                    } else {
                        glib::ControlFlow::Continue
                    }
                });
                *retry.borrow_mut() = Some(id);
            }
        }
    };
    schedule();
    settings.connect_changed(None, move |_, key| {
        if key == "mono-audio" || key == "audio-balance" {
            schedule();
        }
    });
    glib::MainLoop::new(None, false).run();
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("run") | None => run(),
        Some("print") => {
            let s = gio::Settings::new(SCHEMA);
            for args in commands(&wp_values(s.boolean("mono-audio"), s.double("audio-balance"))) {
                println!("pw-metadata {}", args.join(" "));
            }
        }
        _ => {
            eprintln!("usage: fp6-accessibility-audio [run|print]");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values() {
        assert_eq!(
            wp_values(true, -0.5),
            vec![("fp6.a11y.mono-audio", "true".to_string()), ("fp6.a11y.audio-balance", "-0.500".to_string())]
        );
        assert_eq!(wp_values(false, 0.001)[1].1, "0.000");
        assert_eq!(wp_values(false, 7.0)[1].1, "1.000");
        assert_eq!(wp_values(false, f64::NAN)[1].1, "0.000");
    }

    #[test]
    fn negative_values_follow_the_separator() {
        let c = commands(&wp_values(false, -0.25));
        assert_eq!(c.len(), 4);
        assert_eq!(c[1], vec!["-n", "sm-settings", "0", "fp6.a11y.audio-balance", "--", "-0.250", "Spa:String:JSON"]);
        assert_eq!(c[3][1], "persistent-sm-settings");
    }
}
