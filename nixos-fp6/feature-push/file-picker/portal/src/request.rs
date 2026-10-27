//! Portal options (a{sv}) -> the picker's JSON request, and the picker's
//! JSON answer -> portal results.

use crate::mime::{glob_to_regex, MimeDb};
use crate::uri::{bytes_to_path, path_to_file_uri};
use serde_json::{json, Value as Json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use zbus::zvariant::{OwnedValue, Structure, Value};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Open,
    Save,
    SaveFiles,
}

/// (name, [(type, pattern)]): type 0 = glob, 1 = MIME type.
pub type Filter = (String, Vec<(u32, String)>);
/// (id, label, [(option id, option label)], initial selection)
pub type Choice = (String, String, Vec<(String, String)>, String);

#[derive(Debug, Default)]
pub struct Options {
    pub accept_label: Option<String>,
    pub multiple: bool,
    pub directory: bool,
    pub filters: Vec<Filter>,
    pub current_filter: Option<Filter>,
    pub choices: Vec<Choice>,
    pub current_folder: Option<PathBuf>,
    pub current_name: Option<String>,
    pub current_file: Option<PathBuf>,
    pub files: Vec<String>,
}

fn unwrap<'a>(v: &'a Value<'a>) -> &'a Value<'a> {
    match v {
        Value::Value(inner) => unwrap(inner),
        v => v,
    }
}

fn as_str(v: &Value) -> Option<String> {
    match unwrap(v) {
        Value::Str(s) => Some(s.to_string()),
        Value::ObjectPath(p) => Some(p.to_string()),
        _ => None,
    }
}

fn as_u32(v: &Value) -> Option<u32> {
    match unwrap(v) {
        Value::U32(n) => Some(*n),
        Value::I32(n) => u32::try_from(*n).ok(),
        Value::U8(n) => Some(*n as u32),
        _ => None,
    }
}

fn as_bool(v: &Value) -> Option<bool> {
    match unwrap(v) {
        Value::Bool(b) => Some(*b),
        _ => None,
    }
}

fn as_bytes(v: &Value) -> Option<Vec<u8>> {
    match unwrap(v) {
        Value::Array(a) => a
            .iter()
            .map(|x| match x {
                Value::U8(b) => Some(*b),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

fn fields<'a>(v: &'a Value<'a>) -> Option<&'a [Value<'a>]> {
    match unwrap(v) {
        Value::Structure(s) => Some(s.fields()),
        _ => None,
    }
}

fn items<'a>(v: &'a Value<'a>) -> Option<Vec<&'a Value<'a>>> {
    match unwrap(v) {
        Value::Array(a) => Some(a.iter().collect()),
        _ => None,
    }
}

fn parse_filter(v: &Value) -> Option<Filter> {
    let f = fields(v)?;
    let name = as_str(f.first()?)?;
    let mut pats = Vec::new();
    for p in items(f.get(1)?)? {
        let pf = fields(p)?;
        pats.push((as_u32(pf.first()?)?, as_str(pf.get(1)?)?));
    }
    Some((name, pats))
}

fn parse_choice(v: &Value) -> Option<Choice> {
    let f = fields(v)?;
    let id = as_str(f.first()?)?;
    let label = as_str(f.get(1)?)?;
    let mut opts = Vec::new();
    for o in items(f.get(2)?)? {
        let of = fields(o)?;
        opts.push((as_str(of.first()?)?, as_str(of.get(1)?)?));
    }
    let initial = as_str(f.get(3)?).unwrap_or_default();
    Some((id, label, opts, initial))
}

impl Options {
    pub fn parse(o: &HashMap<String, OwnedValue>) -> Options {
        let get = |k: &str| o.get(k).map(|v| &**v);
        let mut out = Options {
            accept_label: get("accept_label").and_then(as_str).map(|s| strip_mnemonic(&s)),
            multiple: get("multiple").and_then(as_bool).unwrap_or(false),
            directory: get("directory").and_then(as_bool).unwrap_or(false),
            current_name: get("current_name").and_then(as_str),
            current_folder: get("current_folder").and_then(as_bytes).and_then(|b| bytes_to_path(&b)),
            current_file: get("current_file").and_then(as_bytes).and_then(|b| bytes_to_path(&b)),
            current_filter: get("current_filter").and_then(parse_filter),
            ..Default::default()
        };
        if let Some(fs) = get("filters").and_then(items) {
            out.filters = fs.into_iter().filter_map(parse_filter).collect();
        }
        if let Some(cs) = get("choices").and_then(items) {
            out.choices = cs.into_iter().filter_map(parse_choice).collect();
        }
        if let Some(fs) = get("files").and_then(items) {
            out.files = fs
                .into_iter()
                .filter_map(|f| as_bytes(f))
                .filter_map(|b| bytes_to_path(&b))
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
        }
        out
    }
}

/// "_Open" -> "Open" (GTK mnemonics; "__" is a literal underscore).
pub fn strip_mnemonic(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '_' {
            if it.peek() == Some(&'_') {
                out.push('_');
                it.next();
            }
            continue;
        }
        out.push(c);
    }
    out
}

fn filter_json(db: &MimeDb, f: &Filter) -> Json {
    let mut globs: Vec<String> = Vec::new();
    for (t, p) in &f.1 {
        let gs = if *t == 1 { db.globs_for(p) } else { vec![p.clone()] };
        for g in gs {
            if !globs.contains(&g) {
                globs.push(g);
            }
        }
    }
    // A MIME type without known globs must not hide every file.
    if globs.is_empty() {
        globs.push("*".to_string());
    }
    let regex: Vec<String> = globs.iter().map(|g| glob_to_regex(g)).collect();
    json!({ "name": f.0, "globs": globs, "regex": regex.join("|") })
}

pub struct Caller {
    pub app_id: String,
    pub name: String,
}

pub fn build(mode: Mode, title: &str, caller: &Caller, o: &Options, db: &MimeDb) -> Json {
    let filters: Vec<Json> = o.filters.iter().map(|f| filter_json(db, f)).collect();
    let current_filter = o
        .current_filter
        .as_ref()
        .and_then(|cf| o.filters.iter().position(|f| f == cf || f.0 == cf.0))
        .map(|i| i as i64)
        .unwrap_or(if o.filters.is_empty() { -1 } else { 0 });
    // current_filter alone (no filters list) is a single filter.
    let (filters, current_filter) = match (&o.current_filter, filters.is_empty()) {
        (Some(cf), true) => (vec![filter_json(db, cf)], 0),
        _ => (filters, current_filter),
    };
    let mut folder = o.current_folder.clone();
    let mut name = o.current_name.clone();
    if let Some(cur) = &o.current_file {
        if folder.is_none() {
            folder = cur.parent().map(Path::to_path_buf);
        }
        if name.is_none() {
            name = cur.file_name().map(|n| n.to_string_lossy().into_owned());
        }
    }
    let mode_s = match mode {
        Mode::Open if o.directory => "folder",
        Mode::Open => "open",
        Mode::Save => "save",
        Mode::SaveFiles => "save-files",
    };
    json!({
        "mode": mode_s,
        "title": title,
        "acceptLabel": o.accept_label,
        "multiple": o.multiple && mode == Mode::Open,
        "filters": filters,
        "currentFilter": current_filter,
        "choices": o.choices.iter().map(|(id, label, opts, initial)| json!({
            "id": id, "label": label,
            "options": opts.iter().map(|(i, l)| json!({ "id": i, "label": l })).collect::<Vec<_>>(),
            "selected": initial,
        })).collect::<Vec<_>>(),
        "currentFolder": folder.map(|p| p.to_string_lossy().into_owned()),
        "currentName": name,
        "files": o.files,
        "appId": caller.app_id,
        "appName": caller.name,
    })
}

/// `name` in `dir`, or "name (2).ext", "name (3).ext", ... if taken.
pub fn unique_in(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    for n in 2..10_000 {
        let p = dir.join(format!("{stem} ({n}){ext}"));
        if !p.exists() {
            return p;
        }
    }
    first
}

fn ov(v: Value) -> OwnedValue {
    OwnedValue::try_from(v).expect("no fds in results")
}

/// The picker's answer `{"response":0,"paths":[..],"filter":i,"choices":[[id,v]..]}`.
pub fn results(mode: Mode, o: &Options, answer: &Json) -> (u32, HashMap<String, OwnedValue>) {
    let mut res = HashMap::new();
    let response = answer.get("response").and_then(Json::as_u64).unwrap_or(2) as u32;
    if response != 0 {
        return (response.min(2), res);
    }
    let paths: Vec<PathBuf> = answer
        .get("paths")
        .and_then(Json::as_array)
        .map(|a| a.iter().filter_map(Json::as_str).filter(|s| s.starts_with('/')).map(PathBuf::from).collect())
        .unwrap_or_default();
    let paths = if mode == Mode::SaveFiles {
        match paths.first() {
            Some(dir) => o.files.iter().map(|f| unique_in(dir, f)).collect(),
            None => Vec::new(),
        }
    } else {
        paths
    };
    if paths.is_empty() {
        return (2, res);
    }
    let uris: Vec<String> = paths.iter().map(|p| path_to_file_uri(p)).collect();
    res.insert("uris".to_string(), ov(Value::from(uris)));
    if let Some(i) = answer.get("filter").and_then(Json::as_i64) {
        let f = if o.filters.is_empty() { o.current_filter.clone() } else { o.filters.get(i as usize).cloned() };
        if let Some((name, pats)) = f {
            let s = Structure::from((name, pats));
            res.insert("current_filter".to_string(), ov(Value::from(s)));
        }
    }
    if !o.choices.is_empty() {
        let picked: HashMap<String, String> = answer
            .get("choices")
            .and_then(Json::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|p| Some((p.get(0)?.as_str()?.to_string(), p.get(1)?.as_str()?.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        let v: Vec<(String, String)> = o
            .choices
            .iter()
            .map(|(id, _, _, initial)| (id.clone(), picked.get(id).cloned().unwrap_or_else(|| initial.clone())))
            .collect();
        res.insert("choices".to_string(), ov(Value::from(v)));
    }
    if mode == Mode::Open {
        res.insert("writable".to_string(), ov(Value::from(false)));
    }
    (0, res)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::Array;

    fn opts() -> HashMap<String, OwnedValue> {
        let mut m = HashMap::new();
        let filters: Vec<Filter> = vec![
            ("Torrents".into(), vec![(1, "application/x-bittorrent".into())]),
            ("All".into(), vec![(0, "*".into())]),
        ];
        m.insert("filters".into(), ov(Value::from(filters)));
        m.insert("multiple".into(), ov(Value::from(true)));
        m.insert("accept_label".into(), ov(Value::from("_Open")));
        let folder: Vec<u8> = b"/home/u/Downloads\0".to_vec();
        m.insert("current_folder".into(), ov(Value::from(Array::from(folder))));
        let choices: Vec<Choice> = vec![("enc".into(), "Encoding".into(), vec![("utf8".into(), "UTF-8".into())], "utf8".into())];
        m.insert("choices".into(), ov(Value::from(choices)));
        m
    }

    #[test]
    fn parse_and_build() {
        let o = Options::parse(&opts());
        assert!(o.multiple);
        assert_eq!(o.accept_label.as_deref(), Some("Open"));
        assert_eq!(o.filters.len(), 2);
        assert_eq!(o.current_folder.as_deref(), Some(Path::new("/home/u/Downloads")));
        assert_eq!(o.choices[0].0, "enc");
        let mut db = MimeDb::default();
        db.add_globs2("50:application/x-bittorrent:*.torrent\n");
        let j = build(Mode::Open, "Open Torrent", &Caller { app_id: "x".into(), name: "X".into() }, &o, &db);
        assert_eq!(j["mode"], "open");
        assert_eq!(j["filters"][0]["globs"][0], "*.torrent");
        assert_eq!(j["currentFilter"], 0);
        assert_eq!(j["currentFolder"], "/home/u/Downloads");
    }

    #[test]
    fn answer_to_results() {
        let o = Options::parse(&opts());
        let a = json!({ "response": 0, "paths": ["/home/u/a b.torrent"], "filter": 1, "choices": [["enc", "utf8"]] });
        let (r, res) = results(Mode::Open, &o, &a);
        assert_eq!(r, 0);
        let uris: Vec<String> = res["uris"].try_clone().unwrap().try_into().unwrap();
        assert_eq!(uris, vec!["file:///home/u/a%20b.torrent"]);
        assert!(res.contains_key("current_filter") && res.contains_key("choices"));
        let (r, res) = results(Mode::Open, &o, &json!({ "response": 1 }));
        assert_eq!((r, res.len()), (1, 0));
        // relative paths are discarded -> "other" error
        let (r, _) = results(Mode::Open, &o, &json!({ "response": 0, "paths": ["rel"] }));
        assert_eq!(r, 2);
    }

    #[test]
    fn mnemonic_and_unique() {
        assert_eq!(strip_mnemonic("_Save"), "Save");
        assert_eq!(strip_mnemonic("a__b"), "a_b");
        let d = std::env::temp_dir().join(format!("fp6-picker-test-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("x.txt"), b"").unwrap();
        std::fs::write(d.join("x (2).txt"), b"").unwrap();
        assert_eq!(unique_in(&d, "x.txt"), d.join("x (3).txt"));
        assert_eq!(unique_in(&d, "y"), d.join("y"));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
