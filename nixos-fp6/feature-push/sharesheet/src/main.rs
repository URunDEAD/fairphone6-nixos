#![forbid(unsafe_code)]
use anyhow::{Context, Result};
use fp6_share::{model::Request, BUS, PATH};
#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "daemon") {
        return fp6_share::service::run().await;
    }
    if args.first().is_some_and(|a| a == "--help") {
        println!("fp6-share FILE...\nfp6-share --text TEXT\nfp6-share --url URL\nfp6-share 'share:?text=Hello'\nfp6-share --clear-history\nfp6-share daemon");
        return Ok(());
    }
    let bus = zbus::Connection::session().await?;
    let proxy = zbus::Proxy::new(&bus, BUS, PATH, BUS).await?;
    if args == ["--clear-history"] {
        proxy.call::<_, _, ()>("ClearHistory", &()).await?;
        return Ok(());
    }
    let r = Request::from_cli(&args)?;
    proxy
        .call::<_, _, ()>("Share", &(r.items, r.mime, r.source_app))
        .await
        .context("Cannot open share sheet; enable/start the fp6-share user service")?;
    Ok(())
}
