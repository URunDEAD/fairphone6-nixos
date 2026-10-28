//! zvariant <-> JSON helpers and small call wrappers used by the tools.

use serde_json::{json, Map, Value as J};
use std::collections::HashMap;
use std::time::Duration;
use zbus::zvariant::{OwnedValue, Value};

pub fn to_json(v: &Value<'_>) -> J {
    match v {
        Value::U8(x) => json!(x),
        Value::Bool(x) => json!(x),
        Value::I16(x) => json!(x),
        Value::U16(x) => json!(x),
        Value::I32(x) => json!(x),
        Value::U32(x) => json!(x),
        Value::I64(x) => json!(x),
        Value::U64(x) => json!(x),
        Value::F64(x) => json!(x),
        Value::Str(s) => json!(s.as_str()),
        Value::Signature(s) => json!(s.to_string()),
        Value::ObjectPath(p) => json!(p.as_str()),
        Value::Value(inner) => to_json(inner),
        Value::Array(a) => J::Array(a.iter().map(to_json).collect()),
        Value::Dict(d) => {
            let mut m = Map::new();
            for (k, val) in d.iter() {
                let key = match k {
                    Value::Str(s) => s.to_string(),
                    other => to_json(other).to_string().trim_matches('"').to_string(),
                };
                m.insert(key, to_json(val));
            }
            J::Object(m)
        }
        Value::Structure(s) => J::Array(s.fields().iter().map(to_json).collect()),
        #[allow(unreachable_patterns)]
        _ => J::Null,
    }
}

pub fn map_to_json(m: &HashMap<String, OwnedValue>) -> J {
    let mut out = Map::new();
    for (k, v) in m {
        out.insert(k.clone(), to_json(v));
    }
    J::Object(out)
}

/// Deserialize a reply body (any shape) to JSON.
pub fn body_json(msg: &zbus::Message) -> J {
    let body = msg.body();
    match body.deserialize::<zbus::zvariant::Structure>() {
        Ok(s) => {
            let mut f: Vec<J> = s.fields().iter().map(to_json).collect();
            if f.len() == 1 {
                f.pop().unwrap()
            } else {
                J::Array(f)
            }
        }
        Err(_) => J::Null,
    }
}

/// A list of property maps; accepts aa{sv} and a(a{sv}).
pub fn list_of_maps(msg: &zbus::Message) -> Vec<J> {
    let body = msg.body();
    if let Ok(v) = body.deserialize::<Vec<HashMap<String, OwnedValue>>>() {
        return v.iter().map(map_to_json).collect();
    }
    if let Ok(v) = body.deserialize::<Vec<(HashMap<String, OwnedValue>,)>>() {
        return v.iter().map(|(m,)| map_to_json(m)).collect();
    }
    match body_json(msg) {
        J::Array(a) => a
            .into_iter()
            .map(|x| match x {
                J::Array(mut inner) if inner.len() == 1 => inner.pop().unwrap(),
                o => o,
            })
            .collect(),
        _ => vec![],
    }
}

pub async fn call<B>(conn: &zbus::Connection, dest: &str, path: &str, iface: &str, method: &str, body: &B) -> Result<zbus::Message, String>
where
    B: serde::Serialize + zbus::zvariant::DynamicType,
{
    tokio::time::timeout(Duration::from_secs(25), conn.call_method(Some(dest), path, Some(iface), method, body))
        .await
        .map_err(|_| format!("{dest} did not answer"))?
        .map_err(|e| friendly(dest, e))
}

fn friendly(dest: &str, e: zbus::Error) -> String {
    let s = e.to_string();
    if s.contains("ServiceUnknown") || s.contains("was not provided") || s.contains("NameHasNoOwner") {
        format!("the phone service {dest} is not running")
    } else {
        format!("{dest}: {s}")
    }
}

/// org.gtk.Actions helpers (the indicators' switches).
pub async fn gaction_state(conn: &zbus::Connection, dest: &str, path: &str, action: &str) -> Result<J, String> {
    let r = call(conn, dest, path, "org.gtk.Actions", "Describe", &(action,)).await?;
    // (bsav): enabled, parameter type, state (0 or 1 element)
    let (enabled, _ptype, state): (bool, zbus::zvariant::Signature, Vec<OwnedValue>) = r.body().deserialize().map_err(|e| e.to_string())?;
    Ok(json!({"enabled": enabled, "state": state.first().map(|v| to_json(v)).unwrap_or(J::Null)}))
}

pub async fn gaction_set_state(conn: &zbus::Connection, dest: &str, path: &str, action: &str, value: Value<'_>) -> Result<(), String> {
    let platform: HashMap<&str, Value> = HashMap::new();
    call(conn, dest, path, "org.gtk.Actions", "SetState", &(action, Value::Value(Box::new(value)), platform)).await.map(|_| ())
}

pub async fn gaction_activate(conn: &zbus::Connection, dest: &str, path: &str, action: &str, param: Option<Value<'_>>) -> Result<(), String> {
    let platform: HashMap<&str, Value> = HashMap::new();
    let params: Vec<Value> = param.into_iter().collect();
    call(conn, dest, path, "org.gtk.Actions", "Activate", &(action, params, platform)).await.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn converts_nested() {
        let mut d: HashMap<&str, Value> = HashMap::new();
        d.insert("n", Value::from(3i32));
        d.insert("s", Value::from("x"));
        d.insert("a", Value::from(vec!["p", "q"]));
        let v = Value::from(d);
        let j = to_json(&v);
        assert_eq!(j["n"], 3);
        assert_eq!(j["s"], "x");
        assert_eq!(j["a"], json!(["p", "q"]));
    }
}
