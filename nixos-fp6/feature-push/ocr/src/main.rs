#![forbid(unsafe_code)]
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use fairphone_ocr::{ocr, service, ui};
use std::path::PathBuf;
#[derive(Parser)]
#[command(version, about)]
struct Args {
    #[command(subcommand)]
    command: Option<Action>,
}
#[derive(Subcommand)]
enum Action {
    /// Recognize locally without starting D-Bus or retaining an index.
    Recognize {
        image: String,
        #[arg(long, default_value = "eng+ron")]
        lang: String,
        #[arg(long)]
        json: bool,
    },
    /// Open the Lomiri app; the session service is activated through D-Bus.
    Ui { image: Option<String> },
    /// Run the session D-Bus service.
    Serve {
        #[arg(long)]
        data_dir: Option<PathBuf>,
        #[arg(long)]
        screenshot_dir: Option<PathBuf>,
    },
}
fn main() -> Result<()> {
    match Args::parse().command.unwrap_or(Action::Ui { image: None }) {
        Action::Recognize { image, lang, json } => {
            let result = ocr::recognize(&image, &lang, |_, _| {}, || false)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("{}", result.text);
            }
        }
        Action::Ui { image } => ui::run(image)?,
        Action::Serve {
            data_dir,
            screenshot_dir,
        } => {
            let home = PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?);
            let data = data_dir.unwrap_or_else(|| {
                std::env::var_os("XDG_DATA_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.join(".local/share"))
                    .join("fairphone-ocr")
            });
            let screenshots = screenshot_dir
                .or_else(|| std::env::var_os("OCR_SCREENSHOT_DIR").map(PathBuf::from))
                .unwrap_or_else(|| home.join("Pictures/Screenshots"));
            service::serve(data, screenshots)?;
        }
    }
    Ok(())
}
