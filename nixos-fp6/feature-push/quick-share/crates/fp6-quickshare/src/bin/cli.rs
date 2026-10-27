#![forbid(unsafe_code)]
use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use fp6_quickshare::{QuickShareProxy, Transfer};
use futures_util::StreamExt;
use quickshare_core::Device;
use std::{
    io::{self, Write},
    path::PathBuf,
    time::Duration,
};

#[derive(Parser)]
#[command(about = "Quick Share LAN client for the FP6 session daemon")]
struct Args {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Devices {
        #[arg(long, default_value_t = 5)]
        wait: u64,
    },
    Send {
        files: Vec<PathBuf>,
        #[arg(long)]
        to: Option<String>,
        #[arg(long, conflicts_with_all = ["files", "url"])]
        text: Option<String>,
        #[arg(long, conflicts_with_all = ["files", "text"])]
        url: Option<String>,
        #[arg(long, default_value_t = 5)]
        wait: u64,
    },
    Receive {
        #[arg(long, default_value = "10m", value_parser = ["10m", "always"])]
        visible: String,
        /// Explicit unattended consent bypass, for synthetic local tests only.
        #[arg(long)]
        accept_for_testing: bool,
    },
    Status,
    Accept {
        transfer_id: String,
    },
    Reject {
        transfer_id: String,
    },
    Cancel {
        transfer_id: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let connection = zbus::Connection::session().await?;
    let proxy = QuickShareProxy::new(&connection)
        .await
        .context("connect to the session Quick Share daemon")?;
    match args.command {
        Command::Status => {
            println!(
                "{}: {} (visibility: {})",
                proxy.device_name().await?,
                proxy.state().await?,
                proxy.visibility().await?
            );
            println!("{}", proxy.list_transfers().await?);
        }
        Command::Devices { wait } => {
            discover(&proxy, wait).await?;
            for d in devices(&proxy).await? {
                println!("{}\t{}\t{}", d.id, d.name, d.address);
            }
        }
        Command::Send {
            files,
            to,
            text,
            url,
            wait,
        } => {
            let items = if let Some(text) = text {
                vec![format!("text:{text}")]
            } else if let Some(url) = url {
                vec![format!("url:{url}")]
            } else {
                files
                    .into_iter()
                    .map(|path| {
                        let path = std::fs::canonicalize(path)?;
                        url::Url::from_file_path(path)
                            .map(|url| url.to_string())
                            .map_err(|_| anyhow::anyhow!("invalid local path"))
                    })
                    .collect::<Result<Vec<_>>>()?
            };
            quickshare_core::parse_payload(&items)?;
            discover(&proxy, wait).await?;
            let devices = devices(&proxy).await?;
            let matched: Vec<_> = devices
                .iter()
                .filter(|d| {
                    to.as_ref()
                        .is_none_or(|name| name == &d.name || name == &d.id)
                })
                .collect();
            if matched.len() != 1 {
                bail!("expected one target, found {}; use devices and --to NAME or ID (open Android's Quick Share receive screen)", matched.len());
            }
            let mut done = proxy.receive_finished().await?;
            let mut progress = proxy.receive_progress().await?;
            let id = proxy.send(&matched[0].id, &items).await?;
            println!("Transfer {id}. Compare the PIN shown by both devices; confirm on Android.");
            let mut poll = tokio::time::interval(Duration::from_millis(500));
            let mut pin_shown = false;
            loop {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => { proxy.cancel(&id).await?; bail!("cancelled"); }
                    Some(signal) = done.next() => { let a = signal.args()?; if a.transfer_id == id { println!("{}", a.status); if a.status != "finished" { bail!("transfer {}", a.status); } break; } }
                    Some(signal) = progress.next() => { let a = signal.args()?; if a.transfer_id == id { eprint!("\r{} / {} bytes", a.bytes, a.total); io::stderr().flush()?; } }
                    _ = poll.tick() => {
                        let transfers: Vec<Transfer> = serde_json::from_str(&proxy.list_transfers().await?)?;
                        if let Some(t) = transfers.iter().find(|t| t.id == id) {
                            if !pin_shown && !t.pin.is_empty() { println!("PIN: {}", t.pin); pin_shown = true; }
                            if matches!(t.status.as_str(), "finished" | "failed" | "cancelled" | "rejected") { println!("\n{}", t.status); if t.status != "finished" { bail!("transfer {}", t.status); } break; }
                        }
                    }
                }
            }
        }
        Command::Receive {
            visible,
            accept_for_testing,
        } => {
            let mut incoming = proxy.receive_incoming_request().await?;
            let mut finished = proxy.receive_finished().await?;
            proxy
                .set_visibility(if visible == "10m" {
                    "everyone-for-10-min"
                } else {
                    "everyone"
                })
                .await?;
            println!("Visible. Compare PINs before accepting. Ctrl-C turns visibility off.");
            let receive: Result<()> = async {
                loop {
                    tokio::select! {
                        _ = tokio::signal::ctrl_c() => break,
                        Some(signal) = incoming.next() => {
                            let a = signal.args()?;
                            let id = a.transfer_id.to_owned();
                            println!("\n{} wants to share {:?}; PIN {} (request {})", a.sender, a.files, a.pin, id);
                            let accept = if accept_for_testing { true } else {
                                println!("Verify PIN on sender, then type yes to accept (anything else declines):");
                                let (answer_sender, answer) = tokio::sync::oneshot::channel();
                                std::thread::spawn(move || { let mut line = String::new(); let result = io::stdin().read_line(&mut line).map(|_| line); let _ = answer_sender.send(result); });
                                tokio::select! { _ = tokio::signal::ctrl_c() => break, answer = answer => answer??.trim() == "yes" }
                            };
                            if accept { proxy.accept(&id).await?; } else { proxy.reject(&id).await?; }
                        }
                        Some(signal) = finished.next() => {
                            let a = signal.args()?; println!("{}: {}", a.transfer_id, a.status);
                            let transfers: Vec<Transfer> = serde_json::from_str(&proxy.list_transfers().await?)?;
                            if let Some(t) = transfers.iter().find(|t| t.id == a.transfer_id && !t.text.is_empty()) { println!("Received text: {}", t.text); }
                        }
                    }
                }
                Ok(())
            }.await;
            proxy.set_visibility("off").await?;
            receive?;
        }
        Command::Accept { transfer_id } => proxy.accept(&transfer_id).await?,
        Command::Reject { transfer_id } => proxy.reject(&transfer_id).await?,
        Command::Cancel { transfer_id } => proxy.cancel(&transfer_id).await?,
    }
    Ok(())
}
async fn devices(proxy: &QuickShareProxy<'_>) -> Result<Vec<Device>> {
    Ok(serde_json::from_str(&proxy.list_devices().await?)?)
}
async fn discover(proxy: &QuickShareProxy<'_>, wait: u64) -> Result<()> {
    if wait > 60 {
        bail!("discovery wait must be at most 60 seconds");
    }
    proxy.start_discovery().await?;
    tokio::time::sleep(Duration::from_secs(wait)).await;
    // Keep discovery alive so IDs remain valid for Send and other session clients.
    Ok(())
}
