//! fp6-assistant-ctl: command line for the assistant service (developer
//! and test use). It cannot answer permission or confirmation dialogs:
//! those are answered on the phone, in the Assistant app.

use fp6_assistant::service::{BUS, PATH};
use futures_util::StreamExt;
use serde_json::Value;

const USAGE: &str = "usage: fp6-assistant-ctl <command>
  chats                       list chats
  show ID                     show a chat (messages, activity)
  new [BACKEND]               new chat, prints its id
  send ID TEXT [FILE...]      send a message (files are attached)
  ask TEXT                    new chat on the default model, send, follow the reply
  follow ID                   print streamed replies / dialogs of a chat
  stop [ID]                   stop one chat (or all)
  delete ID | clear           delete a chat / every chat
  settings                    print settings (JSON)
  set-settings FILE           replace settings with a JSON file
  set-key NAME                store an API key/password read from stdin (backend:<id>, email:<id>)
  models BACKEND              models offered by a backend
  tools | grants              tool catalogue / 'Always allow' grants
  revoke KEY                  remove an 'Always allow' grant
  suggest TEXT                Springboard suggestions
  local                       on-device model service status";

async fn call(conn: &zbus::Connection, method: &str, body: &(impl serde::Serialize + zbus::zvariant::DynamicType)) -> anyhow::Result<zbus::Message> {
    Ok(conn.call_method(Some(BUS), PATH, Some(BUS), method, body).await?)
}

fn s(m: &zbus::Message) -> String {
    m.body().deserialize::<String>().unwrap_or_default()
}

fn pretty(t: &str) -> String {
    serde_json::from_str::<Value>(t).map(|v| serde_json::to_string_pretty(&v).unwrap_or_default()).unwrap_or_else(|_| t.to_string())
}

async fn follow(conn: &zbus::Connection, chat: &str) -> anyhow::Result<()> {
    use std::io::Write;
    let proxy = zbus::Proxy::new(conn, BUS, PATH, BUS).await?;
    let mut sigs = proxy.receive_all_signals().await?;
    while let Some(m) = sigs.next().await {
        let h = m.header();
        let name = h.member().map(|x| x.to_string()).unwrap_or_default();
        match name.as_str() {
            "Delta" => {
                let (c, t): (String, String) = m.body().deserialize()?;
                if c == chat {
                    print!("{t}");
                    std::io::stdout().flush().ok();
                }
            }
            "Prompt" => {
                let p: String = m.body().deserialize()?;
                let v: Value = serde_json::from_str(&p).unwrap_or_default();
                if v["chat"] == chat {
                    eprintln!("\n[{}] {} - answer it in the Assistant app", v["kind"].as_str().unwrap_or(""), v["title"].as_str().unwrap_or(v["op"].as_str().unwrap_or("")));
                }
            }
            "Activity" => {
                let (c, j): (String, String) = m.body().deserialize()?;
                if c == chat {
                    let v: Value = serde_json::from_str(&j).unwrap_or_default();
                    eprintln!("\n[tool {}: {} -> {}]", v["tool"].as_str().unwrap_or(""), v["decision"].as_str().unwrap_or(""), if v["ok"] == true { "ok" } else { "failed" });
                }
            }
            "TurnState" => {
                let (c, st): (String, String) = m.body().deserialize()?;
                if c == chat && (st == "idle" || st.starts_with("error:")) {
                    if let Some(e) = st.strip_prefix("error:") {
                        eprintln!("\nerror: {e}");
                    }
                    println!();
                    return Ok(());
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a = |i: usize| args.get(i).cloned().unwrap_or_default();
    let conn = zbus::Connection::session().await?;
    match a(0).as_str() {
        "chats" => println!("{}", pretty(&s(&call(&conn, "ListChats", &()).await?))),
        "show" => println!("{}", pretty(&s(&call(&conn, "GetChat", &(a(1),)).await?))),
        "new" => println!("{}", s(&call(&conn, "NewChat", &(a(1),)).await?)),
        "send" => {
            let files = serde_json::to_string(&args.get(3..).unwrap_or(&[]).to_vec())?;
            call(&conn, "Send", &(a(1), a(2), files)).await?;
            follow(&conn, &a(1)).await?;
        }
        "ask" => {
            let text = args[1..].join(" ");
            let proxy_conn = conn.clone();
            let id = s(&call(&conn, "Quick", &(text,)).await?);
            eprintln!("chat {id}");
            follow(&proxy_conn, &id).await?;
        }
        "follow" => follow(&conn, &a(1)).await?,
        "stop" if args.len() > 1 => {
            call(&conn, "Stop", &(a(1),)).await?;
        }
        "stop" => {
            call(&conn, "StopAll", &()).await?;
        }
        "delete" => println!("{}", call(&conn, "DeleteChat", &(a(1),)).await?.body().deserialize::<bool>()?),
        "clear" => println!("{} chats deleted", call(&conn, "ClearHistory", &()).await?.body().deserialize::<u32>()?),
        "settings" => println!("{}", pretty(&s(&call(&conn, "GetSettings", &()).await?))),
        "set-settings" => {
            let t = std::fs::read_to_string(a(1))?;
            call(&conn, "SetSettings", &(t,)).await?;
        }
        "set-key" => {
            let mut v = String::new();
            std::io::stdin().read_line(&mut v)?;
            call(&conn, "SetKey", &(a(1), v.trim().to_string())).await?;
            eprintln!("stored");
        }
        "models" => println!("{}", pretty(&s(&call(&conn, "ListModels", &(a(1),)).await?))),
        "tools" => println!("{}", pretty(&s(&call(&conn, "Tools", &()).await?))),
        "grants" => println!("{}", pretty(&s(&call(&conn, "Grants", &()).await?))),
        "revoke" => println!("{}", call(&conn, "RevokeGrant", &(a(1),)).await?.body().deserialize::<bool>()?),
        "suggest" => println!("{}", pretty(&s(&call(&conn, "Suggest", &(args[1..].join(" "),)).await?))),
        "local" => println!("{}", pretty(&s(&call(&conn, "LocalStatus", &()).await?))),
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    }
    Ok(())
}
