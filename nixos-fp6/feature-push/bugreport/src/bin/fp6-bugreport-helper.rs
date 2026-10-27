#![forbid(unsafe_code)]
use fp6_bugreport::{collect::privileged_collect, config::since_seconds, runner::RealRunner};
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), String> {
    // pkexec authenticates before exec; this is not a setuid executable.
    let status = std::fs::read_to_string("/proc/self/status")
        .map_err(|_| "Cannot verify helper identity")?;
    if !status.lines().any(|line| {
        line.starts_with("Uid:") && line.split_whitespace().skip(1).take(4).all(|id| id == "0")
    }) {
        return Err("Run the helper through polkit".into());
    }
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 4 || args[0] != "--since" || args[2] != "--sections" {
        return Err("Expected --since DURATION --sections kernel,journal,display,memory".into());
    }
    since_seconds(&args[1])?;
    let sections: Vec<_> = args[3].split(',').collect();
    if sections.is_empty()
        || sections.len() > 4
        || sections
            .iter()
            .any(|s| !["kernel", "journal", "display", "memory"].contains(s))
    {
        return Err("Invalid helper sections".into());
    }
    let entries: Result<Vec<_>, _> = sections
        .iter()
        .map(|s| privileged_collect(s, &args[1], &RealRunner))
        .collect();
    println!(
        "{}",
        serde_json::to_string(&entries?).map_err(|_| "Cannot encode helper result")?
    );
    Ok(())
}
