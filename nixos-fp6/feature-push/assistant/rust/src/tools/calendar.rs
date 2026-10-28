//! Calendar and alarms. Both live in mkcal (the calendar app's store; the
//! clock app's alarms are organizer items too), reached from the
//! Assistant UI through QtOrganizer (manager "mkcal") and Lomiri UITK's
//! Alarm API — the same APIs the calendar and clock apps use. The daemon
//! validates and confirms; the UI performs the operation.

use super::{arg_str, opt_str, opt_u64, schema, spec, Confirm, Ctx, Output, Tool, ToolSpec};
use crate::permissions::Resource;
use crate::settings::Settings;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

pub fn tools() -> Vec<Arc<dyn Tool>> {
    vec![Arc::new(List), Arc::new(Add), Arc::new(AlarmSet), Arc::new(TimerSet), Arc::new(AlarmsList)]
}

/// "YYYY-MM-DDTHH:MM" (local time; seconds optional).
pub fn valid_local_datetime(s: &str) -> bool {
    let b = s.as_bytes();
    if !(s.len() == 16 || s.len() == 19) {
        return false;
    }
    let digits = |r: std::ops::Range<usize>| b[r].iter().all(u8::is_ascii_digit);
    if !(digits(0..4) && b[4] == b'-' && digits(5..7) && b[7] == b'-' && digits(8..10) && b[10] == b'T' && digits(11..13) && b[13] == b':' && digits(14..16)) {
        return false;
    }
    if s.len() == 19 && !(b[16] == b':' && digits(17..19)) {
        return false;
    }
    let num = |r: std::ops::Range<usize>| s[r].parse::<u32>().unwrap_or(99);
    (1..=12).contains(&num(5..7)) && (1..=31).contains(&num(8..10)) && num(11..13) < 24 && num(14..16) < 60
}

pub fn valid_hhmm(s: &str) -> bool {
    let Some((h, m)) = s.split_once(':') else { return false };
    h.len() <= 2 && m.len() == 2 && h.parse::<u32>().map(|h| h < 24).unwrap_or(false) && m.parse::<u32>().map(|m| m < 60).unwrap_or(false)
}

struct List;
#[async_trait]
impl Tool for List {
    fn spec(&self) -> ToolSpec {
        spec("calendar_list", "Calendar", "Calendar events", "List calendar events from today for a number of days.", schema(&[("days", "integer", "How many days ahead (default 7, max 60)", false)]), false, true)
    }
    fn resources(&self, _a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        Ok(vec![Resource::new("calendar.read")])
    }
    async fn run(&self, a: Value, ctx: &Ctx) -> Result<Output, String> {
        let r = ctx.ui_op("calendar_list", json!({"days": opt_u64(&a, "days", 7, 60)}), 60).await?;
        Ok(Output::json(r.get("events").unwrap_or(&json!([]))))
    }
}

struct Add;
#[async_trait]
impl Tool for Add {
    fn spec(&self) -> ToolSpec {
        spec(
            "calendar_add",
            "Calendar",
            "Add a calendar event",
            "Add an event to the calendar (local times as YYYY-MM-DDTHH:MM). The user confirms first.",
            schema(&[
                ("title", "string", "Title", true),
                ("start", "string", "Start, e.g. 2026-10-30T09:30", true),
                ("end", "string", "End (default: one hour after the start)", false),
                ("location", "string", "Place", false),
                ("notes", "string", "Description", false),
            ]),
            true,
            false,
        )
    }
    fn resources(&self, a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        arg_str(a, "title")?;
        if !valid_local_datetime(arg_str(a, "start")?) {
            return Err("start must look like 2026-10-30T09:30".into());
        }
        if let Some(e) = opt_str(a, "end") {
            if !valid_local_datetime(e) || e < arg_str(a, "start")? {
                return Err("end must look like 2026-10-30T10:30 and be after the start".into());
            }
        }
        Ok(vec![Resource::new("calendar.write")])
    }
    fn confirm(&self, a: &Value) -> Option<Confirm> {
        let mut f = vec![("Event".to_string(), opt_str(a, "title")?.to_string()), ("Starts".into(), opt_str(a, "start")?.replace('T', " "))];
        if let Some(e) = opt_str(a, "end") {
            f.push(("Ends".into(), e.replace('T', " ")));
        }
        if let Some(l) = opt_str(a, "location") {
            f.push(("Where".into(), l.into()));
        }
        if let Some(n) = opt_str(a, "notes") {
            f.push(("Notes".into(), n.into()));
        }
        Some(Confirm { title: "Add this event?".into(), fields: f, action: "Add".into(), destructive: false })
    }
    async fn run(&self, a: Value, ctx: &Ctx) -> Result<Output, String> {
        ctx.ui_op("calendar_add", a.clone(), 60).await?;
        Ok(Output::text(format!("Added \"{}\" on {}.", arg_str(&a, "title")?, arg_str(&a, "start")?.replace('T', " "))))
    }
}

struct AlarmSet;
#[async_trait]
impl Tool for AlarmSet {
    fn spec(&self) -> ToolSpec {
        spec(
            "alarm_set",
            "Alarms",
            "Set an alarm",
            "Set a one-time alarm at a local time (HH:MM; the next occurrence, or on a date YYYY-MM-DD).",
            schema(&[("time", "string", "HH:MM", true), ("date", "string", "YYYY-MM-DD (optional)", false), ("label", "string", "Label", false)]),
            true,
            false,
        )
    }
    fn resources(&self, a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        if !valid_hhmm(arg_str(a, "time")?) {
            return Err("time must be HH:MM".into());
        }
        if let Some(d) = opt_str(a, "date") {
            if !valid_local_datetime(&format!("{d}T00:00")) {
                return Err("date must be YYYY-MM-DD".into());
            }
        }
        Ok(vec![Resource::new("alarms")])
    }
    fn confirm(&self, a: &Value) -> Option<Confirm> {
        let when = match opt_str(a, "date") {
            Some(d) => format!("{d} {}", opt_str(a, "time")?),
            None => opt_str(a, "time")?.to_string(),
        };
        Some(Confirm { title: "Set this alarm?".into(), fields: vec![("Alarm".into(), when), ("Label".into(), opt_str(a, "label").unwrap_or("Alarm").into())], action: "Set".into(), destructive: false })
    }
    async fn run(&self, a: Value, ctx: &Ctx) -> Result<Output, String> {
        let r = ctx.ui_op("alarm_set", a.clone(), 60).await?;
        Ok(Output::text(format!("Alarm set for {}.", r.get("when").and_then(Value::as_str).unwrap_or(arg_str(&a, "time")?))))
    }
}

struct TimerSet;
#[async_trait]
impl Tool for TimerSet {
    fn spec(&self) -> ToolSpec {
        spec("timer_set", "Alarms", "Set a timer", "Start a countdown timer (rings as an alarm when it ends).", schema(&[("minutes", "number", "Duration in minutes (0.5–1440)", true), ("label", "string", "Label", false)]), true, false)
    }
    fn resources(&self, a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        let m = minutes(a)?;
        if !(0.5..=1440.0).contains(&m) {
            return Err("minutes must be between 0.5 and 1440".into());
        }
        Ok(vec![Resource::new("alarms")])
    }
    fn confirm(&self, a: &Value) -> Option<Confirm> {
        Some(Confirm { title: "Start this timer?".into(), fields: vec![("Timer".into(), format!("{} min", minutes(a).ok()?)), ("Label".into(), opt_str(a, "label").unwrap_or("Timer").into())], action: "Start".into(), destructive: false })
    }
    async fn run(&self, a: Value, ctx: &Ctx) -> Result<Output, String> {
        let m = minutes(&a)?;
        let r = ctx.ui_op("timer_set", json!({"seconds": (m * 60.0).round() as u64, "label": opt_str(&a, "label").unwrap_or("Timer")}), 60).await?;
        Ok(Output::text(format!("Timer set; it rings at {}.", r.get("when").and_then(Value::as_str).unwrap_or("the end"))))
    }
}

fn minutes(a: &Value) -> Result<f64, String> {
    match a.get("minutes") {
        Some(Value::Number(n)) => n.as_f64().ok_or("bad number".into()),
        Some(Value::String(s)) => s.parse::<f64>().map_err(|_| "minutes must be a number".into()),
        _ => Err("missing required argument \"minutes\"".into()),
    }
}

struct AlarmsList;
#[async_trait]
impl Tool for AlarmsList {
    fn spec(&self) -> ToolSpec {
        spec("alarms_list", "Alarms", "List alarms", "List the alarms set on the phone.", schema(&[]), false, false)
    }
    fn resources(&self, _a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        Ok(vec![Resource::new("alarms")])
    }
    async fn run(&self, _a: Value, ctx: &Ctx) -> Result<Output, String> {
        let r = ctx.ui_op("alarms_list", json!({}), 30).await?;
        Ok(Output::json(r.get("alarms").unwrap_or(&json!([]))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn datetimes() {
        assert!(valid_local_datetime("2026-10-30T09:30"));
        assert!(valid_local_datetime("2026-10-30T09:30:15"));
        assert!(!valid_local_datetime("2026-13-30T09:30"));
        assert!(!valid_local_datetime("tomorrow 9"));
        assert!(valid_hhmm("7:05"));
        assert!(!valid_hhmm("24:00"));
        assert!(Add.resources(&json!({"title": "x", "start": "2026-10-30T09:30", "end": "2026-10-30T08:00"}), &Settings::default()).is_err());
        assert!(TimerSet.resources(&json!({"minutes": "5"}), &Settings::default()).is_ok());
        assert!(TimerSet.resources(&json!({"minutes": 0.1}), &Settings::default()).is_err());
    }
}
