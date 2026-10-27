use fp6_captions::streaming::{Segmenter, FRAME, WINDOW};
/// Local synthetic speech only. Nix dev shell/check inputs supply both tools.
#[test]
fn english_and_romanian_tts_segment_without_gaps() {
    let work = tempfile::tempdir().unwrap();
    for (lang, text) in [
        (
            "en",
            "The train arrives at the station in ten minutes. I will meet you there.",
        ),
        (
            "ro",
            "Trenul ajunge în gară peste zece minute. Ne întâlnim acolo.",
        ),
    ] {
        let raw = work.path().join("raw.wav");
        let wav = work.path().join("pcm.wav");
        assert!(std::process::Command::new("espeak-ng")
            .args(["-v", lang, "-s", "145", "-w"])
            .arg(&raw)
            .arg(text)
            .status()
            .expect("espeak-ng required; use nix-shell")
            .success());
        assert!(std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-i"])
            .arg(raw)
            .args(["-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le"])
            .arg(&wav)
            .status()
            .expect("ffmpeg required; use nix-shell")
            .success());
        let samples: Vec<f32> = hound::WavReader::open(wav)
            .unwrap()
            .samples::<i16>()
            .map(|s| s.unwrap() as f32 / 32768.)
            .collect();
        let mut s = Segmenter::default();
        let mut chunks = vec![];
        for frame in samples.chunks(FRAME) {
            let mut f = [0.; FRAME];
            f[..frame.len()].copy_from_slice(frame);
            if let Some(c) = s.push(&f) {
                chunks.push(c);
            }
        }
        if let Some(c) = s.finish() {
            chunks.push(c);
        }
        assert!(chunks.len() >= 2);
        assert!(chunks[0].end_sample <= 28800);
        assert!(chunks.iter().all(|c| c.samples.len() <= WINDOW));
        assert!(chunks.windows(2).all(|c| c[1].end_sample > c[0].end_sample));
    }
}
