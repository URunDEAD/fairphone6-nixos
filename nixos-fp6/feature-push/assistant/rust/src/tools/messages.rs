//! Messages, calls, contacts: Lomiri's history-service (read),
//! telephony-service (send SMS, start calls; both over oFono) and the
//! qtcontacts-sqlite store (read-only lookups).

use super::{arg_str, opt_str, opt_u64, schema, spec, Confirm, Ctx, Output, Tool, ToolSpec};
use crate::dbusutil::{call, list_of_maps};
use crate::permissions::Resource;
use crate::settings::Settings;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use zbus::zvariant::Value as ZV;

pub const HISTORY_BUS: &str = "com.lomiri.HistoryService";
pub const HISTORY_PATH: &str = "/com/lomiri/HistoryService";
pub const HISTORY_IFACE: &str = "com.lomiri.HistoryService";
pub const TEL_BUS: &str = "com.lomiri.TelephonyServiceHandler";
pub const TEL_PATH: &str = "/com/lomiri/TelephonyServiceHandler";
pub const TEL_IFACE: &str = "com.lomiri.TelephonyServiceHandler";
pub const DEFAULT_ACCOUNT: &str = "ofono/ofono/account0";

const EVENT_TEXT: i32 = 0;
const EVENT_VOICE: i32 = 1;
// history-service Filter: FilterTypeStandard, Qt::DescendingOrder, MatchCaseSensitive.
const FILTER_STANDARD: i32 = 0;
const SORT_DESC: i32 = 1;
const MATCH_CASE_SENSITIVE: i32 = 1;

pub fn tools() -> Vec<Arc<dyn Tool>> {
    vec![Arc::new(Threads), Arc::new(ReadThread), Arc::new(Search), Arc::new(Send), Arc::new(CallsRecent), Arc::new(PlaceCall), Arc::new(Contacts)]
}

fn sort() -> HashMap<&'static str, ZV<'static>> {
    HashMap::from([("sortField", ZV::from("timestamp")), ("sortOrder", ZV::from(SORT_DESC))])
}

fn filter(prop: Option<(&str, &str)>) -> HashMap<String, ZV<'static>> {
    match prop {
        None => HashMap::new(),
        Some((k, v)) => HashMap::from([
            ("filterType".to_string(), ZV::from(FILTER_STANDARD)),
            ("filterProperty".to_string(), ZV::from(k.to_string())),
            ("filterValue".to_string(), ZV::Value(Box::new(ZV::from(v.to_string())))),
            ("matchFlags".to_string(), ZV::from(MATCH_CASE_SENSITIVE)),
        ]),
    }
}

/// Fetch up to `limit` items from a history view (then destroy it).
async fn view_items(conn: &zbus::Connection, view: &str, iface: &str, limit: usize) -> Result<Vec<Value>, String> {
    let mut out = Vec::new();
    for _ in 0..20 {
        let r = call(conn, HISTORY_BUS, view, iface, "NextPage", &()).await?;
        let page = list_of_maps(&r);
        if page.is_empty() {
            break;
        }
        out.extend(page);
        if out.len() >= limit {
            break;
        }
    }
    let _ = call(conn, HISTORY_BUS, view, iface, "Destroy", &()).await;
    out.truncate(limit);
    Ok(out)
}

pub async fn query_events(conn: &zbus::Connection, kind: i32, by: Option<(&str, &str)>, limit: usize) -> Result<Vec<Value>, String> {
    let r = call(conn, HISTORY_BUS, HISTORY_PATH, HISTORY_IFACE, "QueryEvents", &(kind, sort(), filter(by))).await?;
    let view: String = r.body().deserialize().map_err(|e| e.to_string())?;
    view_items(conn, &view, "com.lomiri.HistoryService.EventView", limit).await
}

pub async fn query_threads(conn: &zbus::Connection, limit: usize) -> Result<Vec<Value>, String> {
    let props: HashMap<String, ZV> = HashMap::new();
    let r = call(conn, HISTORY_BUS, HISTORY_PATH, HISTORY_IFACE, "QueryThreads", &(EVENT_TEXT, sort(), filter(None), props)).await?;
    let view: String = r.body().deserialize().map_err(|e| e.to_string())?;
    view_items(conn, &view, "com.lomiri.HistoryService.ThreadView", limit).await
}

fn participants(v: &Value) -> Vec<String> {
    v.get("participants")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    let id = p.get("identifier").and_then(Value::as_str).or_else(|| p.as_str())?;
                    let alias = p.get("alias").and_then(Value::as_str).unwrap_or("");
                    Some(if alias.is_empty() { id.to_string() } else { format!("{alias} ({id})") })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn s(v: &Value, k: &str) -> String {
    match v.get(k) {
        Some(Value::String(x)) => x.clone(),
        Some(Value::Null) | None => String::new(),
        Some(o) => o.to_string(),
    }
}

fn event_line(e: &Value) -> Value {
    let sender = s(e, "senderId");
    json!({
        "time": s(e, "timestamp"),
        "from": if sender == "self" || sender.is_empty() { "me".to_string() } else { sender },
        "thread": s(e, "threadId"),
        "text": s(e, "message"),
        "unread": e.get("newEvent").and_then(Value::as_bool).unwrap_or(false),
    })
}

struct Threads;
#[async_trait]
impl Tool for Threads {
    fn spec(&self) -> ToolSpec {
        spec("messages_threads", "Messages", "List conversations", "List the most recent text-message conversations (participants, last message, unread count).", schema(&[("limit", "integer", "How many (default 15, max 50)", false)]), false, true)
    }
    fn resources(&self, _a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        Ok(vec![Resource::new("messages.read")])
    }
    async fn run(&self, a: Value, ctx: &Ctx) -> Result<Output, String> {
        let t = query_threads(ctx.session()?, opt_u64(&a, "limit", 15, 50) as usize).await?;
        let rows: Vec<Value> = t
            .iter()
            .map(|x| {
                json!({
                    "thread": s(x, "threadId"), "with": participants(x),
                    "last": s(x, "lastEventTimestamp"), "unread": x.get("unreadCount").cloned().unwrap_or(json!(0)),
                    "preview": crate::llm::truncate(&s(x, "message"), 120),
                })
            })
            .collect();
        Ok(Output::json(&json!(rows)))
    }
}

struct ReadThread;
#[async_trait]
impl Tool for ReadThread {
    fn spec(&self) -> ToolSpec {
        spec(
            "messages_read",
            "Messages",
            "Read messages",
            "Read the latest text messages of one conversation (by thread id from messages_threads) or with one phone number.",
            schema(&[("thread", "string", "Thread id", false), ("number", "string", "Phone number instead of a thread id", false), ("limit", "integer", "How many (default 20, max 100)", false)]),
            false,
            true,
        )
    }
    fn resources(&self, a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        if opt_str(a, "thread").is_none() && opt_str(a, "number").is_none() {
            return Err("give a thread id or a number".into());
        }
        Ok(vec![Resource::new("messages.read")])
    }
    async fn run(&self, a: Value, ctx: &Ctx) -> Result<Output, String> {
        let conn = ctx.session()?;
        let limit = opt_u64(&a, "limit", 20, 100) as usize;
        let thread = match (opt_str(&a, "thread"), opt_str(&a, "number")) {
            (Some(t), _) => t.to_string(),
            (None, Some(n)) => {
                let props: HashMap<&str, ZV> = HashMap::new();
                let _ = props;
                let r = call(conn, HISTORY_BUS, HISTORY_PATH, HISTORY_IFACE, "ThreadForParticipants", &(DEFAULT_ACCOUNT, EVENT_TEXT, vec![n.to_string()], 8i32 /* MatchPhoneNumber */, false)).await?;
                let m = crate::dbusutil::body_json(&r);
                let t = s(&m, "threadId");
                if t.is_empty() {
                    return Ok(Output::text(format!("No messages with {n}.")));
                }
                t
            }
            _ => unreachable!(),
        };
        let ev = query_events(conn, EVENT_TEXT, Some(("threadId", &thread)), limit).await?;
        let mut rows: Vec<Value> = ev.iter().map(event_line).collect();
        rows.reverse();
        Ok(Output::json(&json!(rows)))
    }
}

struct Search;
#[async_trait]
impl Tool for Search {
    fn spec(&self) -> ToolSpec {
        spec("messages_search", "Messages", "Search messages", "Search recent text messages (last 500) for words.", schema(&[("query", "string", "Words to find", true), ("limit", "integer", "Max results (default 20)", false)]), false, true)
    }
    fn resources(&self, _a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        Ok(vec![Resource::new("messages.read")])
    }
    async fn run(&self, a: Value, ctx: &Ctx) -> Result<Output, String> {
        let q = arg_str(&a, "query")?.to_lowercase();
        let words: Vec<&str> = q.split_whitespace().collect();
        let ev = query_events(ctx.session()?, EVENT_TEXT, None, 500).await?;
        let hits: Vec<Value> = ev
            .iter()
            .filter(|e| {
                let t = s(e, "message").to_lowercase();
                words.iter().all(|w| t.contains(w))
            })
            .take(opt_u64(&a, "limit", 20, 50) as usize)
            .map(event_line)
            .collect();
        Ok(Output::json(&json!(hits)))
    }
}

pub fn valid_number(n: &str) -> bool {
    let digits = n.chars().filter(|c| c.is_ascii_digit()).count();
    digits >= 3 && digits <= 20 && n.chars().all(|c| c.is_ascii_digit() || " +-()./".contains(c)) && n.chars().filter(|c| *c == '+').count() <= 1
}

pub async fn account(conn: &zbus::Connection) -> String {
    if let Ok(r) = call(conn, TEL_BUS, TEL_PATH, TEL_IFACE, "AccountIds", &()).await {
        if let Ok(ids) = r.body().deserialize::<Vec<String>>() {
            if let Some(id) = ids.iter().find(|i| i.starts_with("ofono/")) {
                return id.clone();
            }
        }
    }
    DEFAULT_ACCOUNT.into()
}

struct Send;
#[async_trait]
impl Tool for Send {
    fn spec(&self) -> ToolSpec {
        spec(
            "messages_send",
            "Messages",
            "Send a text message",
            "Send an SMS to a phone number. Look the number up with contacts_search first; never guess. The user sees and confirms the exact text.",
            schema(&[("to", "string", "Phone number, e.g. +999 555 0100", true), ("text", "string", "The message", true)]),
            true,
            false,
        )
    }
    fn resources(&self, a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        let to = arg_str(a, "to")?;
        if !valid_number(to) {
            return Err(format!("\"{to}\" is not a phone number; look it up with contacts_search"));
        }
        if arg_str(a, "text")?.chars().count() > 1600 {
            return Err("the message is too long (max 1600 characters)".into());
        }
        Ok(vec![Resource::new("messages.send")])
    }
    fn confirm(&self, a: &Value) -> Option<Confirm> {
        Some(Confirm {
            title: "Send this text message?".into(),
            fields: vec![("To".into(), opt_str(a, "to")?.into()), ("Message".into(), opt_str(a, "text")?.into())],
            action: "Send".into(),
            destructive: false,
        })
    }
    async fn run(&self, a: Value, ctx: &Ctx) -> Result<Output, String> {
        let conn = ctx.session()?;
        let to = arg_str(&a, "to")?;
        let text = arg_str(&a, "text")?;
        let acct = account(conn).await;
        let attachments: Vec<(String, String, String)> = vec![];
        let props: HashMap<&str, ZV> = HashMap::from([("participantIds", ZV::from(vec![to.to_string()]))]);
        let r = call(conn, TEL_BUS, TEL_PATH, TEL_IFACE, "SendMessage", &(acct.as_str(), text, attachments, props)).await?;
        let job: String = r.body().deserialize().unwrap_or_default();
        Ok(Output::text(format!("Message to {to} handed to the phone for sending{}.", if job.is_empty() { String::new() } else { format!(" (job {job})") })))
    }
}

struct CallsRecent;
#[async_trait]
impl Tool for CallsRecent {
    fn spec(&self) -> ToolSpec {
        spec("calls_recent", "Calls", "Call history", "Recent phone calls (who, when, missed, duration).", schema(&[("limit", "integer", "How many (default 20, max 100)", false)]), false, true)
    }
    fn resources(&self, _a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        Ok(vec![Resource::new("calls.read")])
    }
    async fn run(&self, a: Value, ctx: &Ctx) -> Result<Output, String> {
        let ev = query_events(ctx.session()?, EVENT_VOICE, None, opt_u64(&a, "limit", 20, 100) as usize).await?;
        let rows: Vec<Value> = ev
            .iter()
            .map(|e| {
                let sender = s(e, "senderId");
                json!({
                    "time": s(e, "timestamp"),
                    "direction": if sender == "self" { "outgoing" } else { "incoming" },
                    "number": if sender == "self" { s(e, "remoteParticipant") } else { sender },
                    "missed": e.get("missed").and_then(Value::as_bool).unwrap_or(false),
                    "duration": s(e, "duration"),
                })
            })
            .collect();
        Ok(Output::json(&json!(rows)))
    }
}

struct PlaceCall;
#[async_trait]
impl Tool for PlaceCall {
    fn spec(&self) -> ToolSpec {
        spec("call_place", "Calls", "Make a phone call", "Start a phone call to a number (the user confirms first).", schema(&[("number", "string", "Phone number", true)]), true, false)
    }
    fn resources(&self, a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        let n = arg_str(a, "number")?;
        if !valid_number(n) {
            return Err(format!("\"{n}\" is not a phone number"));
        }
        Ok(vec![Resource::new("calls.place")])
    }
    fn confirm(&self, a: &Value) -> Option<Confirm> {
        Some(Confirm { title: "Start this call?".into(), fields: vec![("Call".into(), opt_str(a, "number")?.into())], action: "Call".into(), destructive: true })
    }
    async fn run(&self, a: Value, ctx: &Ctx) -> Result<Output, String> {
        let conn = ctx.session()?;
        let n = arg_str(&a, "number")?;
        let acct = account(conn).await;
        call(conn, TEL_BUS, TEL_PATH, TEL_IFACE, "StartCall", &(n, acct.as_str())).await?;
        Ok(Output::text(format!("Calling {n}.")))
    }
}

// ---- contacts (qtcontacts-sqlite, read-only) ----

pub fn contacts_db() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("FP6_ASSISTANT_CONTACTS_DB") {
        return p.into();
    }
    let home = std::env::var("HOME").unwrap_or_default();
    std::path::PathBuf::from(home).join(".local/share/system/privileged/Contacts/qtcontacts-sqlite/contacts.db")
}

pub fn search_contacts(db: &std::path::Path, query: &str, limit: usize) -> Result<Vec<Value>, String> {
    use rusqlite::{Connection, OpenFlags};
    let c = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(|e| format!("contacts: {e}"))?;
    let _ = c.busy_timeout(std::time::Duration::from_secs(2));
    let like = format!("%{}%", query.to_lowercase().replace('%', "").replace('_', ""));
    let digits: String = query.chars().filter(|c| c.is_ascii_digit()).collect();
    let dlike = if digits.len() >= 3 { format!("%{digits}%") } else { "\u{1}".into() };
    // Names: displayLabel when the column exists, else first + last name.
    let label_sql = ["COALESCE(NULLIF(c.displayLabel,''), TRIM(COALESCE(n.firstName,'') || ' ' || COALESCE(n.lastName,'')))", "TRIM(COALESCE(n.firstName,'') || ' ' || COALESCE(n.lastName,''))"];
    let mut last_err = String::new();
    for label in label_sql {
        let sql = format!(
            "SELECT c.contactId, {label} AS label FROM Contacts c LEFT JOIN Names n ON n.contactId = c.contactId \
             WHERE lower({label}) LIKE ?1 \
                OR c.contactId IN (SELECT contactId FROM PhoneNumbers WHERE replace(replace(replace(phoneNumber,' ',''),'-',''),'+','') LIKE ?2) \
                OR c.contactId IN (SELECT contactId FROM EmailAddresses WHERE lower(emailAddress) LIKE ?1) \
             GROUP BY c.contactId LIMIT ?3"
        );
        let mut st = match c.prepare(&sql) {
            Ok(s) => s,
            Err(e) => {
                last_err = e.to_string();
                continue;
            }
        };
        let rows: Vec<(i64, String)> = st
            .query_map(rusqlite::params![like, dlike, limit as i64], |r| Ok((r.get(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default())))
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .collect();
        let mut out = Vec::new();
        for (id, label) in rows {
            let phones: Vec<String> = c
                .prepare("SELECT phoneNumber FROM PhoneNumbers WHERE contactId = ?1")
                .and_then(|mut s| s.query_map([id], |r| r.get::<_, String>(0)).map(|it| it.filter_map(Result::ok).collect()))
                .unwrap_or_default();
            let emails: Vec<String> = c
                .prepare("SELECT emailAddress FROM EmailAddresses WHERE contactId = ?1")
                .and_then(|mut s| s.query_map([id], |r| r.get::<_, String>(0)).map(|it| it.filter_map(Result::ok).collect()))
                .unwrap_or_default();
            out.push(json!({"name": label, "phones": phones, "emails": emails}));
        }
        return Ok(out);
    }
    Err(format!("contacts database: {last_err}"))
}

struct Contacts;
#[async_trait]
impl Tool for Contacts {
    fn spec(&self) -> ToolSpec {
        spec("contacts_search", "Contacts", "Find contacts", "Find contacts by name, number or e-mail; returns names, phone numbers and e-mail addresses.", schema(&[("query", "string", "Name, part of a number or e-mail", true)]), false, false)
    }
    fn resources(&self, _a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        Ok(vec![Resource::new("contacts.read")])
    }
    async fn run(&self, a: Value, _ctx: &Ctx) -> Result<Output, String> {
        let q = arg_str(&a, "query")?.to_string();
        let db = contacts_db();
        let hits = tokio::task::spawn_blocking(move || search_contacts(&db, &q, 10)).await.map_err(|e| e.to_string())??;
        if hits.is_empty() {
            return Ok(Output::text("No matching contacts."));
        }
        Ok(Output::json(&json!(hits)))
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A synthetic qtcontacts-sqlite database (only the tables used).
    pub fn fake_contacts_db(path: &std::path::Path) {
        let c = rusqlite::Connection::open(path).unwrap();
        c.execute_batch(
            "CREATE TABLE Contacts (contactId INTEGER PRIMARY KEY, displayLabel TEXT);
             CREATE TABLE Names (detailId INTEGER PRIMARY KEY, contactId INTEGER, firstName TEXT, lastName TEXT);
             CREATE TABLE PhoneNumbers (detailId INTEGER PRIMARY KEY, contactId INTEGER, phoneNumber TEXT);
             CREATE TABLE EmailAddresses (detailId INTEGER PRIMARY KEY, contactId INTEGER, emailAddress TEXT);
             INSERT INTO Contacts VALUES (1, 'Test Contact'), (2, '');
             INSERT INTO Names VALUES (1, 1, 'Test', 'Contact'), (2, 2, 'Second', 'Person');
             INSERT INTO PhoneNumbers VALUES (1, 1, '+999 555 0100'), (2, 2, '+999 555 0199');
             INSERT INTO EmailAddresses VALUES (1, 1, 'test@example.com');",
        )
        .unwrap();
    }

    #[test]
    fn contacts_by_name_number_email() {
        let d = tempfile::tempdir().unwrap();
        let db = d.path().join("contacts.db");
        fake_contacts_db(&db);
        let r = search_contacts(&db, "test", 10).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0]["phones"][0], "+999 555 0100");
        assert_eq!(search_contacts(&db, "5550199", 10).unwrap()[0]["name"], "Second Person");
        assert_eq!(search_contacts(&db, "example.com", 10).unwrap()[0]["name"], "Test Contact");
        assert!(search_contacts(&db, "nobody", 10).unwrap().is_empty());
    }

    #[test]
    fn numbers() {
        assert!(valid_number("+999 555 0100"));
        assert!(valid_number("(999) 555-0100"));
        assert!(!valid_number("Test Contact"));
        assert!(!valid_number("12"));
        assert!(!valid_number("+1+2+3"));
    }
}
