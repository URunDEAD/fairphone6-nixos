#![forbid(unsafe_code)]
use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use fp6_webshare::{
    control::{self, Api, BUS, Endpoint, OBJECT},
    session::{Manager, StartOptions, private_dir},
};
use std::{io::Write, path::PathBuf};

#[derive(Parser)]
#[command(
    version,
    about = "Temporary HTTP sharing on trusted Wi-Fi or a phone hotspot"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Run the session-bus daemon (normally started by systemd --user).
    Daemon,
    /// List eligible Wi-Fi and hotspot addresses.
    Interfaces,
    /// Start one share. HTTP is unencrypted: use a trusted LAN.
    Share {
        #[arg(short, long)]
        interface: String,
        #[arg(long, default_value_t = 10)]
        minutes: u64,
        #[arg(long)]
        uploads: bool,
        #[arg(long)]
        text: bool,
        files: Vec<PathBuf>,
    },
    Status,
    Stop,
    /// Approve one staged upload by its ID, saving under the private received directory.
    Approve {
        id: String,
    },
    Reject {
        id: String,
    },
    /// Consume received text and print it. The GUI offers a clipboard Copy action.
    TakeText,
    /// Open the Lomiri app, optionally with selected files.
    Gui {
        files: Vec<PathBuf>,
    },
}
fn runtime() -> Result<PathBuf> {
    let base =
        PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR is required")?);
    if !base.is_absolute() {
        bail!("XDG_RUNTIME_DIR must be absolute");
    }
    let path = base.join("fp6-webshare");
    private_dir(&path)?;
    Ok(path)
}
async fn daemon() -> Result<()> {
    let runtime = runtime()?;
    use std::os::unix::fs::OpenOptionsExt;
    let lock = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(runtime.join("daemon.lock"))?;
    lock.try_lock()
        .context("WebShare daemon is already running")?;
    let connection = zbus::Connection::session().await?;
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .context("No data directory")?;
    let manager = Manager::new(runtime.join("staging"), data.join("fp6-webshare/received"))?;
    connection
        .object_server()
        .at(OBJECT, Api(manager.clone()))
        .await?;
    let (endpoint, task) = control::listen(manager.clone()).await?;
    let mut file = tempfile::NamedTempFile::new_in(&runtime)?;
    file.write_all(&serde_json::to_vec(&endpoint)?)?;
    file.persist(runtime.join("control.json"))?;
    let reply = connection
        .request_name_with_flags(BUS, zbus::fdo::RequestNameFlags::DoNotQueue.into())
        .await?;
    if reply != zbus::fdo::RequestNameReply::PrimaryOwner {
        bail!("WebShare bus name already owned");
    }

    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
    manager.stop().await;
    task.abort();
    let _ = std::fs::remove_file(runtime.join("control.json"));
    Ok(())
}
async fn gui(files: Vec<PathBuf>) -> Result<()> {
    let runtime = runtime()?;
    // GUI launch starts the configured user service. No shell interpolation.
    let status = std::process::Command::new("systemctl")
        .args(["--user", "start", "fp6-webshare.service"])
        .status()?;
    if !status.success() {
        bail!("Could not start fp6-webshare.service");
    }
    let mut endpoint = None;
    for _ in 0..50 {
        if let Ok(bytes) = std::fs::read(runtime.join("control.json"))
            && let Ok(value) = serde_json::from_slice::<Endpoint>(&bytes)
        {
            endpoint = Some(value);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let endpoint = endpoint.context("Daemon control endpoint is not ready")?;
    let qml_dir = std::env::var_os("FP6_WEBSHARE_QML_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("qml"));
    let qml_dir = qml_dir.canonicalize()?;
    let files: Vec<String> = files
        .into_iter()
        .map(|p| std::path::absolute(p).map(|p| p.to_string_lossy().to_string()))
        .collect::<std::io::Result<_>>()?;
    let mut launcher = tempfile::Builder::new()
        .suffix(".qml")
        .tempfile_in(runtime)?;
    const QML_URL_ENCODE: &percent_encoding::AsciiSet = &percent_encoding::CONTROLS
        .add(b'"')
        .add(b'%')
        .add(b'#')
        .add(b'?')
        .add(b' ');
    let import = format!(
        "file://{}",
        percent_encoding::utf8_percent_encode(
            qml_dir.to_str().context("Non-UTF8 QML directory")?,
            QML_URL_ENCODE
        )
    );
    writeln!(
        launcher,
        "import QtQuick 2.12\nimport {} as App\nApp.Main {{ controlUrl: {}; controlKey: {}; selectedFiles: {} }}",
        serde_json::to_string(&import)?,
        serde_json::to_string(&endpoint.url)?,
        serde_json::to_string(&endpoint.key)?,
        serde_json::to_string(&files)?
    )?;
    let exit = std::process::Command::new("qmlscene")
        .env("APP_ID", "fp6-webshare")
        .arg(launcher.path())
        .status()
        .context("Launch qmlscene")?;
    if !exit.success() {
        bail!("QML app exited with {exit}");
    }
    Ok(())
}
#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Daemon => return daemon().await,
        Command::Gui { files } => return gui(files).await,
        _ => {}
    }
    let connection = zbus::Connection::session().await?;
    let proxy = zbus::Proxy::new(&connection, BUS, OBJECT, BUS).await?;
    match cli.command {
        Command::Interfaces => {
            let value: String = proxy.call("Interfaces", &()).await?;
            println!("{value}");
        }
        Command::Share {
            interface,
            minutes,
            uploads,
            text,
            files,
        } => {
            let files = files
                .into_iter()
                .map(std::path::absolute)
                .collect::<std::io::Result<_>>()?;
            let options = serde_json::to_string(&StartOptions {
                files,
                interface,
                minutes,
                uploads,
                text,
            })?;
            let value: String = proxy.call("Start", &(options,)).await?;
            let state: serde_json::Value = serde_json::from_str(&value)?;
            let url = state["url"].as_str().context("Missing URL")?;
            eprintln!("HTTP is unencrypted. Use trusted Wi-Fi or a private phone hotspot.");
            println!("{url}");
            let qr = qrcode::QrCode::new(url.as_bytes())?;
            println!(
                "{}",
                qr.render::<qrcode::render::unicode::Dense1x2>()
                    .quiet_zone(true)
                    .build()
            );
        }
        Command::Status => {
            let value: String = proxy.call("Status", &()).await?;
            println!("{value}");
        }
        Command::Stop => {
            proxy.call::<_, _, ()>("Stop", &()).await?;
        }
        Command::Approve { id } => {
            let value: String = proxy.call("Approve", &(id,)).await?;
            println!("{value}");
        }
        Command::Reject { id } => {
            proxy.call::<_, _, ()>("Reject", &(id,)).await?;
        }
        Command::TakeText => {
            let value: String = proxy.call("TakeText", &()).await?;
            println!("{value}");
        }
        _ => unreachable!(),
    }
    Ok(())
}
