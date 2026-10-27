use anyhow::{bail, Context, Result};
use image::{GenericImageView, ImageDecoder};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub const MAX_BYTES: u64 = 24 * 1024 * 1024;
pub const MAX_PIXELS: u64 = 12_000_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Word {
    pub text: String,
    pub confidence: f32,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub line: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Recognition {
    pub path: String,
    pub image_url: String,
    pub width: u32,
    pub height: u32,
    pub text: String,
    pub words: Vec<Word>,
    pub elapsed_ms: u128,
}

pub fn local_path(input: &str) -> Result<PathBuf> {
    let path = if input.starts_with("file:") {
        let url = url::Url::parse(input)?;
        if url.host_str().is_some_and(|host| host != "localhost") {
            bail!("Only local file URLs are accepted");
        }
        url.to_file_path()
            .map_err(|_| anyhow::anyhow!("Invalid file URL"))?
    } else {
        if input.contains("://") {
            bail!("Only local images are accepted");
        }
        PathBuf::from(input)
    };
    let path = path.canonicalize().context("Cannot open image")?;
    if !path.is_file() {
        bail!("Image must be a regular file");
    }
    Ok(path)
}

pub fn languages(lang: &str) -> Result<&str> {
    match lang {
        "eng" | "ron" | "eng+ron" | "ron+eng" => Ok(lang),
        _ => bail!("Choose eng, ron, or eng+ron"),
    }
}

pub fn parse_tsv(tsv: &str, width: u32, height: u32) -> Result<(Vec<Word>, String)> {
    let mut words = Vec::new();
    for row in tsv.lines().skip(1) {
        let cols: Vec<_> = row.splitn(12, '\t').collect();
        if cols.len() != 12 || cols[0] != "5" || cols[11].trim().is_empty() {
            continue;
        }
        let x: u32 = cols[6].parse().context("Invalid OCR coordinate")?;
        let y: u32 = cols[7].parse()?;
        let w: u32 = cols[8].parse()?;
        let h: u32 = cols[9].parse()?;
        let confidence: f32 = cols[10].parse()?;
        if x >= width || y >= height || !confidence.is_finite() || confidence < 0.0 {
            continue;
        }
        words.push(Word {
            text: cols[11].trim().to_owned(),
            confidence,
            x,
            y,
            width: w.min(width - x),
            height: h.min(height - y),
            line: cols[1..5].join(":"),
        });
    }
    let mut text = String::new();
    let mut last_line = "";
    for word in &words {
        if !text.is_empty() {
            text.push(if last_line == word.line { ' ' } else { '\n' });
        }
        text.push_str(&word.text);
        last_line = &word.line;
    }
    Ok((words, text))
}

pub fn recognize(
    input: &str,
    lang: &str,
    mut progress: impl FnMut(u8, &str),
    canceled: impl Fn() -> bool,
) -> Result<Recognition> {
    let started = Instant::now();
    languages(lang)?;
    let path = local_path(input)?;
    progress(5, "Preparing image");
    let temp = tempfile::tempdir()?;
    // Snapshot through a bounded read so a concurrently modified source cannot bypass limits.
    let mut source = fs::File::open(&path)?;
    let snapshot = temp.path().join("source");
    let mut destination = fs::File::create(&snapshot)?;
    let copied = std::io::copy(
        &mut std::io::Read::take(&mut source, MAX_BYTES + 1),
        &mut destination,
    )?;
    if copied > MAX_BYTES {
        bail!("Image exceeds the 24 MiB input limit");
    }
    let reader = image::ImageReader::open(&snapshot)?.with_guessed_format()?;
    if !matches!(
        reader.format(),
        Some(image::ImageFormat::Png | image::ImageFormat::Jpeg)
    ) {
        bail!("Choose a PNG or JPEG image");
    }
    let (w, h) = image::ImageReader::open(&snapshot)?
        .with_guessed_format()?
        .into_dimensions()?;
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        bail!("Image exceeds the 12 megapixel limit");
    }
    let mut decoder = reader.into_decoder()?;
    let orientation = decoder.orientation()?;
    let mut decoded = image::DynamicImage::from_decoder(decoder)?;
    decoded.apply_orientation(orientation);
    let (width, height) = decoded.dimensions();
    let resized = if width.max(height) > 2400 {
        decoded.resize(2400, 2400, image::imageops::FilterType::Triangle)
    } else {
        decoded
    };
    let (ocr_width, ocr_height) = resized.dimensions();
    let prepared = temp.path().join("input.png");
    resized.to_rgb8().save(&prepared)?;
    drop(resized);
    if canceled() {
        bail!("Canceled");
    }
    progress(20, "Recognizing text");
    let output = temp.path().join("output.tsv");
    let errors = temp.path().join("stderr");
    let mut child =
        Command::new(std::env::var_os("OCR_TESSERACT").unwrap_or_else(|| "tesseract".into()))
            .arg(&prepared)
            .args(["stdout", "-l", lang, "--psm", "3", "tsv"])
            .env("OMP_THREAD_LIMIT", "1")
            .stdin(Stdio::null())
            .stdout(fs::File::create(&output)?)
            .stderr(fs::File::create(&errors)?)
            .spawn()
            .context("Cannot start Tesseract; install the English and Romanian language data")?;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if canceled() || started.elapsed() > Duration::from_secs(90) {
            let _ = child.kill();
            let _ = child.wait();
            bail!(if canceled() {
                "Canceled"
            } else {
                "OCR exceeded the 90 second deadline"
            });
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    if !status.success() {
        use std::io::Read;
        let mut detail = String::new();
        fs::File::open(errors)?
            .take(4096)
            .read_to_string(&mut detail)?;
        bail!("Tesseract failed: {}", detail.trim());
    }
    progress(90, "Reading text regions");
    if fs::metadata(&output)?.len() > 4 * 1024 * 1024 {
        bail!("OCR output exceeds limit");
    }
    let (mut words, text) = parse_tsv(&fs::read_to_string(output)?, ocr_width, ocr_height)?;
    for word in &mut words {
        word.x = (u64::from(word.x) * u64::from(width) / u64::from(ocr_width)) as u32;
        word.y = (u64::from(word.y) * u64::from(height) / u64::from(ocr_height)) as u32;
        word.width = (u64::from(word.width) * u64::from(width) / u64::from(ocr_width)) as u32;
        word.height = (u64::from(word.height) * u64::from(height) / u64::from(ocr_height)) as u32;
    }
    progress(100, "Done");
    Ok(Recognition {
        path: path.to_string_lossy().into_owned(),
        image_url: file_url(&path)?,
        width,
        height,
        text,
        words,
        elapsed_ms: started.elapsed().as_millis(),
    })
}

pub fn file_url(path: &Path) -> Result<String> {
    Ok(url::Url::from_file_path(path)
        .map_err(|_| anyhow::anyhow!("Invalid local path"))?
        .into())
}

pub fn latest_screenshot(directory: &Path) -> Result<PathBuf> {
    // Only explicitly designated screenshot directories, never recursive home scans.
    let mut newest = None;
    for entry in fs::read_dir(directory).context("Screenshot directory is unavailable")? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !["png", "jpg", "jpeg"].contains(&ext.as_str()) {
            continue;
        }
        let modified = entry.metadata()?.modified()?;
        if newest.as_ref().is_none_or(|(time, _)| modified > *time) {
            newest = Some((modified, path));
        }
    }
    newest
        .map(|(_, path)| path)
        .context("No PNG or JPEG screenshots found")
}
