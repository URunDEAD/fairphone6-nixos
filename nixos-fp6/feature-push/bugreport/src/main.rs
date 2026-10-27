#![forbid(unsafe_code)]
use clap::Parser;
use fp6_bugreport::{
    bundle,
    config::{Cli, Options},
};
fn main() {
    let cli = Cli::parse();
    let options = Options {
        since: cli.since,
        sections: cli.sections,
        privileged: cli.privileged,
        screenshot: cli.screenshot,
    };
    if let Err(error) = options.validate() {
        eprintln!("{error}");
        std::process::exit(2);
    }
    if cli.preview {
        println!("{}", serde_json::to_string_pretty(&options).unwrap());
        println!("Text will be redacted; no core files or uploads. Screenshot pixels, if explicitly selected, are unredacted.");
        return;
    }
    match bundle::create(&options) {
        Ok(path) => println!("{}", path.display()),
        Err(error) => {
            eprintln!("Could not create report: {error}");
            std::process::exit(1);
        }
    }
}
