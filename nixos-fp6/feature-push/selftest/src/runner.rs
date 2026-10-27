use crate::{
    base,
    config::Config,
    files,
    process::{self, Failure},
    report::{Check, Report, Status},
};
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
pub const DEFAULT_CONFIG: &str = include_str!("../data/probes.toml");
pub fn config(path: Option<&Path>) -> Result<Config, &'static str> {
    match path {
        Some(path) => {
            let text =
                std::fs::read_to_string(path).map_err(|_| "Probe configuration cannot be read")?;
            if text.len() > 131072 {
                return Err("Probe configuration is too large");
            }
            Config::parse(&text)
        }
        None => Config::parse(DEFAULT_CONFIG),
    }
}
pub fn worker(id: &str, cfg: &Config) -> Check {
    if let Some(index) = id
        .strip_prefix("feature:")
        .and_then(|s| s.parse::<usize>().ok())
    {
        return cfg
            .probe
            .get(index)
            .map(crate::dbus::check)
            .unwrap_or_else(|| Check::new("worker", Status::Fail, "Invalid probe index"));
    }
    match id {
        "schemas" => files::schemas(&files::schema_dirs()),
        "desktops" => files::desktops(
            &files::data_dirs(),
            &std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .collect::<Vec<_>>(),
        ),
        _ if base::CHECKS.contains(&id) => base::check(id),
        _ => Check::new("worker", Status::Fail, "Unknown check identifier"),
    }
}
pub fn run(
    executable: &Path,
    cfg_path: Option<&Path>,
    quick: bool,
    system_only: bool,
    deadline: Duration,
) -> Report {
    let cfg = match config(cfg_path) {
        Ok(c) => c,
        Err(e) => {
            return Report::new(
                "configuration",
                vec![Check::new("configuration", Status::Fail, e)],
            )
        }
    };
    let mut tasks: Vec<(String, String)> = base::CHECKS
        .iter()
        .map(|s| (s.to_string(), s.to_string()))
        .collect();
    if system_only {
        tasks.retain(|(_, id)| {
            !matches!(
                id.as_str(),
                "failed-user"
                    | "journal-user"
                    | "lomiri"
                    | "maliit"
                    | "audio-sink"
                    | "audio-source"
            )
        });
    }
    for (i, probe) in cfg.probe.iter().enumerate() {
        if !system_only || probe.bus == "system" {
            tasks.push((format!("feature:{i}"), format!("feature-{}", probe.id)));
        }
    }
    let mut checks = vec![];
    if quick {
        for id in ["schemas", "desktops"] {
            checks.push(Check::new(
                id,
                Status::Skip,
                "Omitted in quick mode; run full self-test for coverage",
            ));
        }
    } else {
        tasks.extend([
            ("schemas".into(), "schemas".into()),
            ("desktops".into(), "desktops".into()),
        ]);
    }
    for (task, id) in tasks {
        let mut cmd = Command::new(executable);
        cmd.args(["--internal-check", &task]);
        if let Some(path) = cfg_path {
            cmd.arg("--config").arg(path);
        }
        let check = match process::run(cmd, deadline) {
            Ok((true, bytes)) => match serde_json::from_slice::<Check>(&bytes) {
                Ok(c) if c.id == id => Check::new(&id, c.status, &c.detail),
                _ => Check::new(&id, Status::Fail, "Worker returned a malformed result"),
            },
            Err(Failure::Timeout) => Check::new(
                &id,
                Status::Fail,
                "Check exceeded its deadline and was stopped",
            ),
            _ => Check::new(
                &id,
                Status::Fail,
                "Check worker failed; raw error suppressed",
            ),
        };
        checks.push(check);
    }
    Report::new(
        if system_only {
            "system"
        } else if quick {
            "quick"
        } else {
            "full"
        },
        checks,
    )
}
pub fn default_directory() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Documents/Self-tests"))
}
