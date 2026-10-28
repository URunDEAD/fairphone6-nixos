//! fp6-assistantd: the assistant's session service (systemd user unit).

use fp6_assistant::agent::Agent;
use fp6_assistant::keyring::Secrets;
use fp6_assistant::prompts::PromptHub;
use fp6_assistant::service::{self, Service};
use fp6_assistant::settings::Paths;
use fp6_assistant::tools::Registry;
use futures_util::StreamExt;
use std::sync::Arc;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    let paths = Paths::from_env();
    let session = zbus::Connection::session().await?;
    let system = zbus::Connection::system().await.ok();
    let mut dirs = vec![std::path::PathBuf::from("/etc/fp6-assistant/tools.d"), paths.tools_d()];
    if let Ok(extra) = std::env::var("FP6_ASSISTANT_TOOLS_D") {
        dirs.push(extra.into());
    }
    let registry = Registry::standard(&dirs);
    let hub = PromptHub::new();
    let secrets = Arc::new(Secrets::new(Some(session.clone()), paths.credentials_file()));
    let agent = Agent::new(paths, registry, hub.clone(), Some(session.clone()), system, secrets);
    let days = agent.settings().retention_days;
    let expired = agent.store.expire(days, fp6_assistant::store::now());
    if expired > 0 {
        eprintln!("fp6-assistantd: removed {expired} chats older than {days} days");
    }
    let trust_any = std::env::var("FP6_ASSISTANT_TRUST_ANY").as_deref() == Ok("1");
    let bin_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf()));
    session.object_server().at(service::PATH, Service { agent: agent.clone(), bin_dir, trust_any }).await?;
    session.request_name(service::BUS).await?;

    // No UI on screen when a dialog is needed: a notification opens it.
    {
        let conn = session.clone();
        hub.set_summon(Box::new(move |prompt| {
            let conn = conn.clone();
            let chat = prompt["chat"].as_str().unwrap_or("").to_string();
            let title = match prompt["kind"].as_str() {
                Some("permission") => "The assistant asks for permission",
                Some("confirm") => "The assistant needs your confirmation",
                _ => "The assistant needs the app",
            };
            tokio::spawn(async move { notify(&conn, title, &chat).await });
        }));
    }

    // Forget the UI when its connection goes away.
    {
        let conn = session.clone();
        let hub = hub.clone();
        tokio::spawn(async move {
            let Ok(dbus) = zbus::fdo::DBusProxy::new(&conn).await else { return };
            let Ok(mut s) = dbus.receive_name_owner_changed().await else { return };
            while let Some(sig) = s.next().await {
                if let Ok(a) = sig.args() {
                    if a.new_owner().is_none() {
                        hub.ui_gone(a.name().as_str());
                    }
                }
            }
        });
    }
    tokio::spawn(service::pump(session.clone(), agent.clone()));
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
    agent.stop_all();
    Ok(())
}

async fn notify(conn: &zbus::Connection, title: &str, chat: &str) {
    use std::collections::HashMap;
    use zbus::zvariant::Value;
    let mut hints: HashMap<&str, Value> = HashMap::new();
    hints.insert("urgency", Value::U8(2));
    hints.insert("x-lomiri-snap-decisions-timeout", Value::I32(-1));
    let actions = vec!["default", "Open"];
    let r = conn
        .call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &("Assistant", 0u32, "com.fairphone6.Assistant", title, "Open the Assistant to answer.", actions, hints, 60_000i32),
        )
        .await;
    let Ok(r) = r else { return };
    let Ok(id) = r.body().deserialize::<u32>() else { return };
    let Ok(proxy) = zbus::Proxy::new(conn, "org.freedesktop.Notifications", "/org/freedesktop/Notifications", "org.freedesktop.Notifications").await else { return };
    let Ok(mut s) = proxy.receive_signal("ActionInvoked").await else { return };
    let wait = async {
        while let Some(m) = s.next().await {
            if let Ok((nid, _action)) = m.body().deserialize::<(u32, String)>() {
                if nid == id {
                    let url = format!("assistant:///chat/{chat}");
                    let _ = fp6_assistant::tools::apps::dispatch(conn, &url).await;
                    return;
                }
            }
        }
    };
    let _ = tokio::time::timeout(std::time::Duration::from_secs(300), wait).await;
}
