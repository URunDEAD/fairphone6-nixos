use fp6_share::{
    history::History,
    model::{Request, Target},
    providers::write_destination,
    registry::decode_peer,
};
use std::os::unix::fs::PermissionsExt;
use url::Url;
fn text(value: &str) -> Request {
    Request::new(
        vec![value.into()],
        "text/plain".into(),
        "Synthetic app".into(),
    )
    .unwrap()
}
#[test]
fn validates_empty_oversize_and_mixed_items() {
    assert!(Request::new(vec![], "text/plain".into(), "test".into()).is_err());
    assert!(Request::new(vec!["x".repeat(131073)], "text/plain".into(), "test".into()).is_err());
    assert!(Request::new(vec!["x".into(); 33], "text/plain".into(), "test".into()).is_err());
    assert!(Request::new(vec!["hello".into()], "invalid".into(), "test".into()).is_err());
    assert!(Request::new(vec!["a\0b".into()], "text/plain".into(), "test".into()).is_err());
}
#[test]
fn canonical_file_uris_and_cli_unicode() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Synthetic ș garden.png");
    std::fs::write(&path, "synthetic").unwrap();
    let r = Request::from_cli(&[path.to_string_lossy().into()]).unwrap();
    assert_eq!(r.mime, "image/png");
    assert_eq!(r.content_type(), "pictures");
    assert_eq!(r.files().unwrap(), vec![path.clone()]);
    assert!(r.items[0].contains("%20"));
    assert!(Request::new(
        vec![r.items[0].clone(), "text".into()],
        "text/plain".into(),
        "test".into()
    )
    .is_err());
    assert!(Request::from_cli(&[dir.path().to_string_lossy().into()]).is_err());
}
#[test]
fn rejects_remote_and_missing_files() {
    for uri in [
        "file://evil.invalid/tmp/example",
        "https://example.invalid/a.png",
        "file:///definitely-missing-fp6-synthetic",
    ] {
        assert!(Request::new(vec![uri.into()], "image/png".into(), "test".into()).is_err());
    }
}
#[test]
fn share_uri_is_encoded_text_or_http_only() {
    let r = Request::from_share_uri("share:?text=Hello%20%26%20%3Cgarden%3E").unwrap();
    assert_eq!(r.items, ["Hello & <garden>"]);
    assert!(
        Request::from_share_uri("share:?url=https%3A%2F%2Fexample.invalid%2Fdemo")
            .unwrap()
            .is_link()
    );
    for uri in [
        "share:?file=/etc/passwd",
        "share:?text=file%3A%2F%2F%2Fetc%2Fpasswd",
        "share:?url=file%3A%2F%2F%2Fetc%2Fpasswd",
        "share://host?text=x",
        "share:?text=x&text=y",
        "share:?text=x#ignored",
        "share:?url=javascript:alert(1)",
    ] {
        assert!(Request::from_share_uri(uri).is_err(), "{uri}");
    }
}
#[test]
fn cli_does_not_execute_shell_metacharacters() {
    let r =
        Request::from_cli(&["--text".into(), "$(touch /tmp/never); `echo nope`".into()]).unwrap();
    assert_eq!(r.items[0], "$(touch /tmp/never); `echo nope`");
}
#[test]
fn history_is_recent_unique_bounded_and_available_only() {
    let mut history = History::default();
    let targets: Vec<_> = (0..10)
        .map(|i| {
            Target::new(
                "quick",
                &i.to_string(),
                "Synthetic person",
                "Synthetic",
                true,
            )
        })
        .collect();
    for t in &targets {
        history.record(t);
    }
    assert_eq!(history.ids.len(), 8);
    assert_eq!(history.ids[0], "quick:9");
    history.record(&targets[4]);
    assert_eq!(history.ids[0], "quick:4");
    history.record(&Target::new("action", "copy", "Copy", "", false));
    assert_eq!(history.ids.len(), 8);
    assert_eq!(
        history.recent(&targets[..5]),
        vec![targets[4].clone(), targets[3].clone(), targets[2].clone()]
    );
}
#[test]
fn history_persists_identifiers_only_privately() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state/recent.json");
    let mut history = History::default();
    history.record(&Target::new(
        "kde",
        "synthetic",
        "Secret name",
        "private payload",
        true,
    ));
    history.save(&path).unwrap();
    assert_eq!(History::load(&path).ids, ["kde:synthetic"]);
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(!content.contains("Secret"));
    assert!(!content.contains("payload"));
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    std::fs::write(&path, "corrupt").unwrap();
    assert!(History::load(&path).ids.is_empty());
}
#[test]
fn decodes_actual_content_hub_peer_struct_and_excludes_self() {
    use zbus::zvariant::{OwnedValue, Value};
    let peer = |id: &str| {
        OwnedValue::try_from(Value::from((
            id,
            "Synthetic Notes",
            Vec::<u8>::new(),
            "notes",
            false,
        )))
        .unwrap()
    };
    let target = decode_peer(peer("synthetic.notes")).unwrap().unwrap();
    assert_eq!(target.id, "content:synthetic.notes");
    assert_eq!(target.label, "Synthetic Notes");
    assert!(decode_peer(peer("com.fairphone6.Share")).unwrap().is_none());
    assert!(decode_peer(OwnedValue::from(7u32)).is_err());
}
#[tokio::test]
async fn save_is_private_and_never_overwrites() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("saved.txt");
    write_destination(&text("Synthetic shared text"), &path)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "Synthetic shared text"
    );
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(write_destination(&text("replacement"), &path)
        .await
        .is_err());
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "Synthetic shared text"
    );
    let link = temp.path().join("link.txt");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    assert!(write_destination(&text("replacement"), &link)
        .await
        .is_err());
}
#[tokio::test]
async fn save_streams_files() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.txt");
    let dest = temp.path().join("dest.txt");
    std::fs::write(&source, vec![42u8; 1024 * 1024]).unwrap();
    let r = Request::new(
        vec![Url::from_file_path(&source).unwrap().to_string()],
        "application/octet-stream".into(),
        "test".into(),
    )
    .unwrap();
    write_destination(&r, &dest).await.unwrap();
    assert_eq!(std::fs::read(source).unwrap(), std::fs::read(dest).unwrap());
}
#[test]
fn webshare_staging_is_private_and_removed_on_drop() {
    let dir = tempfile::tempdir().unwrap();
    let file =
        fp6_share::providers::stage_text_in(&text("Synthetic WebShare text"), dir.path()).unwrap();
    let path = file.path().to_path_buf();
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "Synthetic WebShare text"
    );
    drop(file);
    assert!(!path.exists());
}
#[test]
fn content_hub_category_mapping_preserves_native_types() {
    for (mime, expected) in [
        ("text/vcard", "contacts"),
        ("text/calendar", "events"),
        ("application/epub+zip", "ebooks"),
        ("audio/mpeg", "music"),
        ("video/mp4", "videos"),
    ] {
        let r = Request {
            items: vec!["file:///synthetic/item".into()],
            mime: mime.into(),
            source_app: "synthetic".into(),
        };
        assert_eq!(r.content_type(), expected);
    }
}
#[test]
fn history_recovers_from_stale_temporary_file_and_duplicate_ids() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recent.json");
    std::fs::write(path.with_extension("new"), "Interrupted previous write").unwrap();
    let mut history = History::default();
    history.record(&Target::new("quick", "synthetic", "Synthetic", "", true));
    history.save(&path).unwrap();
    history.save(&path).unwrap();
    std::fs::write(&path, r#"{"ids":["quick:x","quick:x","invalid","kde:y"]}"#).unwrap();
    assert_eq!(History::load(&path).ids, ["quick:x", "kde:y"]);
}
