#![forbid(unsafe_code)]
use clap::Parser;
use fp6_selftest::{
    report::{self, Check, Report, Status},
    runner,
};
use std::{path::PathBuf, time::Duration};
#[derive(Parser)]
#[command(
    version,
    about = "Read-only FP6 health checks. Exit: 0 PASS/SKIP, 1 WARN, 2 FAIL. Reports contain no raw device data."
)]
struct Args {
    #[arg(long)]
    quick: bool,
    #[arg(long)]
    json: bool,
    /// Run system-bus/base checks only (for the boot service).
    #[arg(long)]
    system_only: bool,
    /// Override the audited TOML probe list.
    #[arg(long)]
    config: Option<PathBuf>,
    /// Report directory. Default: ~/Documents/Self-tests (private, mode 0700).
    #[arg(long)]
    output_dir: Option<PathBuf>,
    /// Do not save reports (stdout only).
    #[arg(long)]
    no_save: bool,
    /// Deadline in seconds for each independent check.
    #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u64).range(1..=30))]
    timeout: u64,
    #[arg(long, hide = true)]
    internal_check: Option<String>,
}
fn main() {
    // Panics must not echo external values or paths, even in a worker.
    std::panic::set_hook(Box::new(|_| {
        eprintln!("Self-test stopped unexpectedly; raw details suppressed")
    }));
    let args = match Args::try_parse() {
        Ok(args) => args,
        Err(e)
            if matches!(
                e.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            let _ = e.print();
            return;
        }
        Err(_) => {
            eprintln!("Invalid command line; use fp6-selftest --help");
            std::process::exit(2);
        }
    };
    let config_path = args.config.or_else(|| {
        let p = PathBuf::from("/etc/fp6-selftest/probes.toml");
        p.exists().then_some(p)
    });
    if let Some(id) = args.internal_check {
        let check = match runner::config(config_path.as_deref()) {
            Ok(cfg) => runner::worker(&id, &cfg),
            Err(e) => Check::new("configuration", Status::Fail, e),
        };
        println!("{}", serde_json::to_string(&check).unwrap());
        return;
    }
    let mut result = match std::env::current_exe() {
        Ok(exe) => runner::run(
            &exe,
            config_path.as_deref(),
            args.quick,
            args.system_only,
            Duration::from_secs(args.timeout),
        ),
        Err(_) => Report::new(
            "startup",
            vec![Check::new(
                "startup",
                Status::Fail,
                "Could not locate check worker",
            )],
        ),
    };
    if !args.no_save {
        let dir = args.output_dir.or_else(runner::default_directory);
        if dir
            .as_ref()
            .is_none_or(|dir| report::save(&result, dir).is_err())
        {
            result.checks.push(Check::new(
                "report-save",
                Status::Fail,
                "Could not save private reports; use an owned directory with mode 0700",
            ));
            result.status = Status::Fail;
        }
    }
    if args.json {
        println!("{}", result.json());
    } else {
        print!("{}", result.text());
    }
    std::process::exit(result.status.exit_code());
}
