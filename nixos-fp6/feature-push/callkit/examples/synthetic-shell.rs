//! A scripted shell + DND + platform stand-in for private-bus demos only.
//! It prints desired hardware state; it never manipulates real hardware.
#![forbid(unsafe_code)]
use fp6_callkit::{
    client::CallsProxy,
    model::{Action, Route, Snapshot, State},
    service::{NOTIFY, PLATFORM, SHELL},
};
use std::time::Duration;
struct Platform;
#[zbus::interface(name = "com.fairphone6.CallsPlatform1")]
impl Platform {
    fn apply(&self, effects: &str, _lease_ms: u32) {
        println!("Synthetic platform: {effects}");
    }
}
struct Notify;
#[zbus::interface(name = "com.fairphone6.Notifications1")]
impl Notify {
    fn should_alert(&self, _category: &str) -> (bool, bool) {
        (true, true)
    }
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let connection = zbus::connection::Builder::session()?
        .name(SHELL)?
        .name(PLATFORM)?
        .name(NOTIFY)?
        .serve_at("/com/fairphone6/CallsPlatform1", Platform)?
        .serve_at("/com/fairphone6/Notifications1", Notify)?
        .build()
        .await?;
    let proxy = CallsProxy::new(&connection).await?;
    let id = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if let Ok(raw) = proxy.snapshot().await {
                let snapshot: Snapshot = serde_json::from_str(&raw).unwrap();
                if let Some(c) = snapshot.calls.iter().find(|c| c.state == State::Ringing) {
                    break c.id;
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await?;
    for action in [
        Action::Answer,
        Action::Mute(true),
        Action::Route(Route::Speaker),
        Action::Hold,
        Action::Resume,
        Action::Route(Route::Bluetooth),
        Action::End,
    ] {
        tokio::time::sleep(Duration::from_millis(700)).await;
        println!("Synthetic user: {action:?}");
        proxy.control(id, &serde_json::to_string(&action)?).await?;
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let snapshot: Snapshot = serde_json::from_str(&proxy.snapshot().await?).unwrap();
                if snapshot
                    .calls
                    .iter()
                    .find(|c| c.id == id)
                    .is_some_and(|c| c.pending.is_none())
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Ok::<(), zbus::Error>(())
        })
        .await??;
    }
    println!("Synthetic history: {}", proxy.history().await?);
    Ok(())
}
