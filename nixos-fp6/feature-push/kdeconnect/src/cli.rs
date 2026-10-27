#![forbid(unsafe_code)]
use anyhow::{ensure, Result};
use lomiri_kdeconnect::app::{BUS_NAME, BUS_PATH};
#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "--help" {
        println!("kdeconnectctl devices|discover|pair ID FINGERPRINT|accept ID FINGERPRINT|unpair ID|plugin ID NAME on/off|ping ID|ring ID|stop-ringing|share ID URL_OR_PATH|media ID JSON");
        return Ok(());
    }
    let bus = zbus::Connection::session().await?;
    let p = zbus::Proxy::new(&bus, BUS_NAME, BUS_PATH, BUS_NAME).await?;
    let arg = |n: usize| {
        args.get(n)
            .map(String::as_str)
            .ok_or_else(|| anyhow::anyhow!("missing argument {n}"))
    };
    match args[0].as_str() {
        "devices" => {
            let s: String = p.call("Devices", &()).await?;
            println!("{s}");
        }
        "discover" => {
            let _: () = p.call("Discover", &()).await?;
        }
        "pair" | "accept" | "unpair" => {
            let method = match args[0].as_str() {
                "pair" => "RequestPair",
                "accept" => "AcceptPair",
                _ => "Unpair",
            };
            if method == "Unpair" {
                let _: () = p.call(method, &(arg(1)?,)).await?;
            } else {
                let _: () = p.call(method, &(arg(1)?, arg(2)?)).await?;
            }
        }
        "plugin" => {
            let enabled = arg(3)?;
            ensure!(["on", "off"].contains(&enabled), "use on/off");
            let _: () = p
                .call("SetPlugin", &(arg(1)?, arg(2)?, enabled == "on"))
                .await?;
        }
        "stop-ringing" => {
            let _: () = p.call("StopRinging", &()).await?;
        }
        "ping" | "ring" | "share" | "media" => {
            let mut data = args.get(2).cloned().unwrap_or_default();
            if args[0] == "share" && std::path::Path::new(&data).exists() {
                data = lomiri_kdeconnect::transfer::local_file_url(std::path::Path::new(&data))?;
            }
            let _: () = p.call("Action", &(arg(1)?, args[0].as_str(), data)).await?;
        }
        _ => anyhow::bail!("unknown command"),
    }
    Ok(())
}
