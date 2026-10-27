#![forbid(unsafe_code)]
use fp6_callkit::{
    client::{self, CallsProxy},
    model::State,
};
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let connection = zbus::Connection::session().await?;
    let proxy = CallsProxy::new(&connection).await?;
    let id = proxy
        .report_incoming("org.example.SyntheticVoip", "Synthetic caller")
        .await?;
    println!("Synthetic incoming call {id}. Waiting for shell controls (45 second ring timeout).");
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => { proxy.report_ended(id).await?; break; },
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {},
        }
        if let Some(command) = client::pending(&proxy, id).await? {
            // A real provider must execute media/signalling changes BEFORE acknowledging.
            println!("Simulating {:?}", command.action);
            proxy.complete(id, command.serial, true).await?;
        }
        if client::own_calls(&proxy)
            .await?
            .iter()
            .any(|c| c.id == id && c.state == State::Ended)
        {
            break;
        }
    }
    Ok(())
}
