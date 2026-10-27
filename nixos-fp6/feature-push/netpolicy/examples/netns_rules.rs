use fp6_netpolicy::nft::{counter_name, Rules, Scope};
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    anyhow::ensure!(
        args.len() == 4,
        "usage: netns_rules CGROUP allow|block new|update"
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
