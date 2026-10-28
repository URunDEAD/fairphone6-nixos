use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use fp6_llm::{
    manager::{Manager, Options},
    service, BUS, OBJECT,
};
use std::{path::PathBuf, time::Duration};

#[derive(Parser)]
#[command(version, about)]
struct Args {
    #[command(subcommand)]
    command: Action,
}
#[derive(Subcommand)]
enum Action {
    Daemon {
        #[arg(long)]
        data_dir: Option<PathBuf>,
        #[arg(long, env = "FP6_LLAMA_SERVER", default_value = "llama-server")]
        server: PathBuf,
        #[arg(long, env = "FP6_LLAMA_VULKAN_SERVER")]
        vulkan_server: Option<PathBuf>,
        #[arg(long, default_value_t = 0)]
        port: u16,
    },
    Status,
    List,
    Endpoint,
    Unload,
    Download {
        id: String,
    },
    Delete {
        id: String,
    },
    SetActive {
        id: String,
    },
    /// Requested context in tokens; 0 = automatic.
    SetContext {
        tokens: u32,
    },
    Configure {
        #[arg(long)]
        idle_seconds: u64,
        #[arg(long)]
        use_gpu: bool,
        #[arg(long)]
        battery_saver: bool,
    },
}
#[tokio::main]
async fn main() -> Result<()> {
    match Args::parse().command {
        Action::Daemon {
            data_dir,
            server,
            vulkan_server,
            port,
        } => {
            let root = data_dir.unwrap_or_else(|| {
                std::env::var_os("XDG_DATA_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| {
                        PathBuf::from(std::env::var_os("HOME").expect("HOME must be set"))
                            .join(".local/share")
                    })
                    .join("fp6-llm")
            });
            let manager = Manager::new(
                Options::new(root, server, vulkan_server),
                fp6_llm::catalogue(),
            )
            .await?;
            let listener =
                tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
            *manager.endpoint.write().await =
                format!("http://127.0.0.1:{}/v1", listener.local_addr()?.port());
            manager.maintain().await;
            let _bus = service::bus(manager.clone())
                .await
                .context("own session D-Bus name")?;
            let monitor = manager.clone();
            let monitor_task = tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    monitor.maintain().await;
                }
            });
            let shutdown = async {
                let mut term =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .expect("SIGTERM handler");
                tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
            };
            axum::serve(listener, service::router(manager.clone()))
                .with_graceful_shutdown(shutdown)
                .await?;
            monitor_task.abort();
            manager.unload("manager stopping").await;
        }
        action => {
            let conn = zbus::Connection::session().await?;
            let proxy = zbus::Proxy::new(&conn, BUS, OBJECT, BUS).await?;
            match action {
                Action::Status => {
                    let value: String = proxy.call("Status", &()).await?;
                    println!("{value}");
                }
                Action::List => {
                    let value: String = proxy.call("ListModels", &()).await?;
                    println!("{value}");
                }
                Action::Endpoint => {
                    let value: (String, String) = proxy.call("Endpoint", &()).await?;
                    println!("{}", serde_json::json!({"url": value.0, "model": value.1}));
                }
                Action::Unload => {
                    proxy.call::<_, _, ()>("Unload", &()).await?;
                }
                Action::Download { id } => {
                    proxy.call::<_, _, ()>("Download", &(id,)).await?;
                }
                Action::Delete { id } => {
                    proxy.call::<_, _, ()>("Delete", &(id,)).await?;
                }
                Action::SetActive { id } => {
                    proxy.call::<_, _, ()>("SetActive", &(id,)).await?;
                }
                Action::SetContext { tokens } => {
                    proxy.call::<_, _, ()>("SetContextSize", &(tokens,)).await?;
                }
                Action::Configure {
                    idle_seconds,
                    use_gpu,
                    battery_saver,
                } => {
                    proxy
                        .call::<_, _, ()>("Configure", &(idle_seconds, use_gpu, battery_saver))
                        .await?;
                }
                _ => unreachable!(),
            }
        }
    }
    Ok(())
}
