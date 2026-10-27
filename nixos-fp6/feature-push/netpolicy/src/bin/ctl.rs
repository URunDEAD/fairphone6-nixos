fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    println!(
        "{}",
        fp6_netpolicy::daemon::call(
            args.get(1).map(String::as_str).unwrap_or("status"),
            args.get(2).map(String::as_str)
        )?
    );
    Ok(())
}
