use fp6_netpolicy::nft::{counter_name, AppNet, PermScope, Rules, Scope};
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    // App network permission: netns_rules CGROUP perm WIFI MOBILE VPN LOCAL PREV
    // (0/1 each; PREV = "new" for a new table, else the number of permission
    // chains the current table has).
    if args.len() == 8 && args[2] == "perm" {
        let on = |i: usize| args[i] == "1";
        let access = AppNet {
            wifi: on(3),
            mobile: on(4),
            vpn: on(5),
            local: on(6),
        };
        let (exists, previous) = match args[7].as_str() {
            "new" => (false, 0),
            n => (true, n.parse()?),
        };
        let rules = Rules {
            uid: 1000,
            interfaces: vec![],
            perm: if access.restricted() {
                vec![PermScope {
                    app: "test".into(),
                    path: args[1].clone(),
                    inode: 1,
                    access,
                }]
            } else {
                vec![]
            },
            mobile_interfaces: vec!["np9".into()],
            ..Default::default()
        };
        print!(
            "{}",
            rules.render_with(&Default::default(), exists, previous)?
        );
        return Ok(());
    }
    anyhow::ensure!(
        args.len() == 4,
        "usage: netns_rules CGROUP allow|block new|update | CGROUP perm WIFI MOBILE VPN LOCAL PREV"
    );
    let update = args[3] == "update";
    let rules = Rules {
        uid: 1000,
        interfaces: vec!["np0".into()],
        scopes: vec![Scope {
            app: "test".into(),
            path: args[1].clone(),
            inode: 1,
            allowed: args[2] == "allow",
        }],
        system_allow: vec![],
        restrict_unknown: false,
        ..Default::default()
    };
    let names = if update {
        [counter_name("test", "tx"), counter_name("test", "rx")]
            .into_iter()
            .collect()
    } else {
        Default::default()
    };
    print!("{}", rules.render(&names, update)?);
    Ok(())
}
