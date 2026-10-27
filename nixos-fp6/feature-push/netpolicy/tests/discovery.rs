use fp6_netpolicy::{
    config::{App, SystemConfig},
    helper::discover,
};
#[test]
fn configurable_scope_matching_and_ambiguous_mapping_rejection() {
    let root = std::env::temp_dir().join(format!("netpolicy-discovery-{}", std::process::id()));
    let unit = root.join(
        "user.slice/user-1000.slice/user@1000.service/app.slice/lomiri-app-launch-chat-123.scope",
    );
    std::fs::create_dir_all(&unit).unwrap();
    let mut config = SystemConfig {
        uid: 1000,
        apps: vec![App {
            id: "chat".into(),
            unit_pattern: "lomiri-app-launch-chat-*.scope".into(),
        }],
        system_allow: vec![],
        mobile_adapter: None,
    };
    config.validate().unwrap();
    let scopes = discover(&config, &root).unwrap();
    assert_eq!(scopes.len(), 1);
    assert_eq!(scopes[0].app, "chat");
    assert!(scopes[0].inode > 0);
    config.apps.push(App {
        id: "other".into(),
        unit_pattern: "*.scope".into(),
    });
    assert!(discover(&config, &root).is_err());
    std::fs::remove_dir_all(root).unwrap();
}
