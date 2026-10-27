#![forbid(unsafe_code)]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let demo = match args.as_slice() {
        [] => false,
        [arg] if arg == "--demo" => true,
        _ => {
            return Err(
                "usage: fp6-callsd [--demo] (demo assumes cellular idle; use a private bus)".into(),
            )
        }
    };
    if demo {
        eprintln!("Demo mode: simulated cellular idle; no hardware integration implied.");
    }
    fp6_callkit::service::run(demo).await
}
