#![forbid(unsafe_code)]
use anyhow::{ensure, Result};
use lomiri_kdeconnect::{
    app::{App, BUS_NAME, BUS_PATH},
    dbus::Api,
    network, plugins,
    store::{state_directory, Store},
    tls::Credentials,
};

#[tokio::main]
async fn main() -> Result<()> {
    let mut interfaces = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--interface" => {
                let name = args
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--interface needs a name"))?;
                ensure!(
                    !name.is_empty()
                        && name.len() <= 15
                        && name
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.')),
                    "invalid interface name"
                );
                interfaces.push(name);
            }
            "--help" => {
                println!("lomiri-kdeconnectd --interface wlan0 [--interface wlp1s0]\nOnly configured interfaces with a Linux wireless sysfs entry are used.");
                return Ok(());
            }
            _ => anyhow::bail!("unknown argument: {arg}"),
        }
    }
    ensure!(
        !interfaces.is_empty(),
        "configure at least one Wi-Fi interface with --interface"
    );
    let store = Store::load(state_directory())?;
    let credentials = Credentials::load(&store.directory, &store.id)?;
    let bus = zbus::Connection::session().await?;
    let app = App::new(
        store,
        credentials,
        bus.clone(),
        interfaces,
        lomiri_kdeconnect::store::home().join("Downloads/KDE Connect"),
    );
    bus.object_server().at(BUS_PATH, Api(app.clone())).await?;
    bus.request_name(BUS_NAME).await?;
    let monitor_app = app.clone();
    tokio::spawn(async move {
        loop {
            if let Err(e) = plugins::notification_monitor(monitor_app.clone()).await {
                monitor_app
                    .event(format!("Notification forwarding unavailable: {e}"))
                    .await;
            }
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        }
    });
    tokio::spawn(plugins::periodic(app.clone()));
    tokio::select! {
        result = network::run(app.clone()) => result?,
        _ = tokio::signal::ctrl_c() => {},
        _ = async {if let Ok(mut sig) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {sig.recv().await;}} => {},
    }
    app.stop_ring().await?;
    Ok(())
}
