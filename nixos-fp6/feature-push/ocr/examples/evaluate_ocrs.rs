//! Optional evaluation harness. Models are fetched explicitly, never by the app.
use anyhow::{Context, Result};
use ocrs::{ImageSource, OcrEngine, OcrEngineParams};
use std::time::Instant;
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let started = Instant::now();
    let engine = OcrEngine::new(OcrEngineParams {
        detection_model: Some(rten::Model::load_file(
            "evaluation/models/text-detection.onnx",
        )?),
        recognition_model: Some(rten::Model::load_file(
            "evaluation/models/text-recognition.onnx",
        )?),
        ..Default::default()
    })?;
    let image = image::open(args.get(1).context("Pass an image path")?)?.into_rgb8();
    let input =
        engine.prepare_input(ImageSource::from_bytes(image.as_raw(), image.dimensions())?)?;
    let text = engine.get_text(&input)?;
    println!(
        "{}",
        serde_json::json!({"text":text, "elapsed_ms":started.elapsed().as_millis()})
    );
    Ok(())
}
