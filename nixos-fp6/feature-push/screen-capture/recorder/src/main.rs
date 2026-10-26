// SPDX-License-Identifier: GPL-3.0-only
//! fp6-screenrecord: encodes the frames Lomiri's ScreenRecorder item reads
//! back from its own scene graph into an MP4 in ~/Videos/Screencasts.
//!
//! Lomiri starts this program itself (only after the user confirmed a
//! recording in the shell) and passes one end of a socketpair as `--fd`.
//! Each frame on it is a 32-byte header (`FP6F`, version 1, width, height,
//! stride, reserved, CLOCK_MONOTONIC pts in ns, little endian) followed by
//! stride * height bytes of RGBA, top row first. EOF (Lomiri stopped the
//! recording, the screen locked, or the shell went away) or SIGTERM/SIGINT
//! finish the file. There is no other input: this process cannot capture
//! anything by itself.
//!
//! Video: H.264 from the Iris V4L2 encoder (v4l2h264enc) when available, else
//! x264; frames are timestamped from their capture time (variable rate,
//! capped/filled to --fps). Audio (optional): microphone and/or what the
//! phone plays (the default sink's PipeWire/Pulse monitor), AAC.

use gstreamer as gst;
use gstreamer_app as gst_app;
use gst::prelude::*;

use std::collections::HashMap;
use std::fs;
use std::io::{self, Read};
use std::os::fd::FromRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MAGIC: &[u8; 4] = b"FP6F";
const HEADER_LEN: usize = 32;

static STOP: AtomicBool = AtomicBool::new(false);
/// --report-stdout: the caller (Lomiri) shows the notifications. Lomiri's
/// notification server drops a notification as soon as its sender leaves
/// the bus, and this process exits right after saving.
static REPORT_STDOUT: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

#[derive(Clone, Debug)]
struct Options {
    fd: i32,
    rotate: u32,
    fps: u32,
    microphone: bool,
    system_audio: bool,
    out_dir: PathBuf,
    encoder: String,
    bitrate_kbit: u32,
}

fn usage() -> ! {
    eprintln!(
        "usage: fp6-screenrecord --fd N [--rotate 0|90|180|270] [--fps N] [--microphone] \
         [--system-audio] [--out-dir DIR] [--encoder auto|hw|sw] [--bitrate KBIT] [--report-stdout]"
    );
    std::process::exit(2);
}

fn parse_args() -> Options {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"));
    let videos = std::env::var_os("XDG_VIDEOS_DIR").map(PathBuf::from).unwrap_or_else(|| home.join("Videos"));
    let mut o = Options {
        fd: -1,
        rotate: 0,
        fps: 30,
        microphone: false,
        system_audio: false,
        out_dir: videos.join("Screencasts"),
        encoder: "auto".into(),
        bitrate_kbit: 6000,
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| usage());
        match a.as_str() {
            "--fd" => o.fd = value().parse().unwrap_or_else(|_| usage()),
            "--rotate" => o.rotate = value().parse().unwrap_or_else(|_| usage()),
            "--fps" => o.fps = value().parse().unwrap_or_else(|_| usage()),
            "--microphone" => o.microphone = true,
            "--system-audio" => o.system_audio = true,
            "--out-dir" => o.out_dir = PathBuf::from(value()),
            "--encoder" => o.encoder = value(),
            "--report-stdout" => REPORT_STDOUT.store(true, Ordering::SeqCst),
            "--bitrate" => o.bitrate_kbit = value().parse().unwrap_or_else(|_| usage()),
            _ => usage(),
        }
    }
    if o.fd < 0 || ![0, 90, 180, 270].contains(&o.rotate) || o.fps == 0 || o.fps > 60 {
        usage();
    }
    if !["auto", "hw", "sw"].contains(&o.encoder.as_str()) {
        usage();
    }
    o
}

struct Frame {
    width: u32,
    height: u32,
    stride: u32,
    pts_ns: i64,
    data: Vec<u8>,
}

/// Reads one frame; Ok(None) on a clean EOF (also a frame cut short by it).
fn read_frame(input: &mut fs::File) -> io::Result<Option<Frame>> {
    let mut header = [0u8; HEADER_LEN];
    if !read_full(input, &mut header)? {
        return Ok(None);
    }
    if &header[0..4] != MAGIC || u32::from_le_bytes(header[4..8].try_into().unwrap()) != 1 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "bad frame header"));
    }
    let word = |i: usize| u32::from_le_bytes(header[i..i + 4].try_into().unwrap());
    let (width, height, stride) = (word(8), word(12), word(16));
    let pts_ns = i64::from_le_bytes(header[24..32].try_into().unwrap());
    if width == 0 || height == 0 || width > 8192 || height > 8192 || stride < width * 4 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "bad frame size"));
    }
    let mut data = vec![0u8; stride as usize * height as usize];
    if !read_full(input, &mut data)? {
        return Ok(None);
    }
    Ok(Some(Frame { width, height, stride, pts_ns, data }))
}

/// Fills `buf`; false on EOF (before or inside the buffer). EINTR from a
/// stop signal is reported as EOF, too.
fn read_full(input: &mut fs::File, buf: &mut [u8]) -> io::Result<bool> {
    let mut done = 0;
    while done < buf.len() {
        match input.read(&mut buf[done..]) {
            Ok(0) => return Ok(false),
            Ok(n) => done += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {
                if STOP.load(Ordering::SeqCst) {
                    return Ok(false);
                }
            }
            Err(e) => return Err(e),
        }
    }
    Ok(true)
}

fn monotonic_ns() -> i64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as i64 * 1_000_000_000 + ts.tv_nsec as i64
}

fn hw_encoder_available() -> bool {
    gst::ElementFactory::find("v4l2h264enc").is_some()
}

fn flip_method(rotate: u32) -> &'static str {
    // The shell is drawn rotated by `rotate` degrees on the (portrait) panel;
    // turn the picture back so the video is upright.
    match rotate {
        90 => "counterclockwise",
        180 => "rotate-180",
        270 => "clockwise",
        _ => "none",
    }
}

fn pipeline_description(o: &Options, width: u32, height: u32, hw: bool, location: &Path) -> String {
    let encoder = if hw {
        let bits = o.bitrate_kbit as u64 * 1000;
        format!(
            "v4l2h264enc extra-controls=\"controls,video_bitrate_mode=0,video_bitrate={bits},\
             video_peak_bitrate={peak},video_gop_size={gop}\" ! video/x-h264,profile=high",
            peak = bits * 3 / 2,
            gop = o.fps * 2
        )
    } else {
        format!(
            "x264enc speed-preset=superfast tune=zerolatency bitrate={} key-int-max={} \
             ! video/x-h264,profile=main",
            o.bitrate_kbit,
            o.fps * 2
        )
    };
    let mut d = format!(
        "appsrc name=video format=time is-live=true do-timestamp=false block=false max-bytes=0 \
           caps=video/x-raw,format=RGBA,width={width},height={height},framerate=0/1 \
         ! queue max-size-buffers=6 max-size-bytes=0 max-size-time=0 leaky=downstream \
         ! videoflip method={flip} \
         ! videorate max-rate={fps} ! video/x-raw,framerate={fps}/1 \
         ! videoconvert ! video/x-raw,format=NV12 \
         ! {encoder} ! h264parse name=parsed ! queue ! mux. ",
        flip = flip_method(o.rotate),
        fps = o.fps
    );
    let mut sources = Vec::new();
    if o.microphone {
        sources.push(("mic", None));
    }
    if o.system_audio {
        sources.push(("system", Some("@DEFAULT_MONITOR@")));
    }
    if !sources.is_empty() {
        let mixed = sources.len() > 1;
        if mixed {
            d.push_str("audiomixer name=amix ! audioconvert ! audioresample ! audio/x-raw,rate=48000,channels=2 ! voaacenc bitrate=128000 ! queue ! mux. ");
        }
        for (name, device) in sources {
            let device = device.map(|d| format!(" device={d}")).unwrap_or_default();
            d.push_str(&format!(
                "pulsesrc name={name} client-name=\"Screen recorder\" provide-clock=false do-timestamp=true{device} \
                 ! audio/x-raw,rate=48000,channels=2 ! audioconvert ! audioresample \
                 ! queue max-size-time=1000000000 leaky=downstream ! "
            ));
            if mixed {
                d.push_str("amix. ");
            } else {
                d.push_str("audioconvert ! audioresample ! audio/x-raw,rate=48000,channels=2 ! voaacenc bitrate=128000 ! queue ! mux. ");
            }
        }
    }
    // Fragmented while recording (a recording cut off by a crash or a kill
    // stays playable), rewritten as a normal MP4 at the end.
    d.push_str(&format!(
        "mp4mux name=mux fragment-duration=1000 fragment-mode=first-moov-then-finalise \
         ! filesink location=\"{}\"",
        location.display()
    ));
    d
}

struct Session {
    pipeline: gst::Pipeline,
    appsrc: gst_app::AppSrc,
    encoded: Arc<AtomicU64>,
    hw: bool,
}

fn build(o: &Options, width: u32, height: u32, hw: bool, location: &Path) -> Result<Session, String> {
    let description = pipeline_description(o, width, height, hw, location);
    let pipeline = gst::parse::launch(&description)
        .map_err(|e| format!("pipeline: {e}"))?
        .downcast::<gst::Pipeline>()
        .map_err(|_| "not a pipeline".to_string())?;
    // One clock for everything: CLOCK_MONOTONIC, the clock Lomiri stamps the
    // frames with (the audio sources must not provide their own).
    let clock = gst::SystemClock::obtain();
    clock.set_property_from_str("clock-type", "monotonic");
    pipeline.use_clock(Some(&clock));
    let appsrc = pipeline
        .by_name("video")
        .and_then(|e| e.downcast::<gst_app::AppSrc>().ok())
        .ok_or("no appsrc")?;
    let encoded = Arc::new(AtomicU64::new(0));
    if let Some(pad) = pipeline.by_name("parsed").and_then(|p| p.static_pad("src")) {
        let counter = encoded.clone();
        pad.add_probe(gst::PadProbeType::BUFFER, move |_, _| {
            counter.fetch_add(1, Ordering::Relaxed);
            gst::PadProbeReturn::Ok
        });
    }
    pipeline
        .set_state(gst::State::Playing)
        .map_err(|e| format!("cannot start the pipeline ({e})"))?;
    Ok(Session { pipeline, appsrc, encoded, hw })
}

fn timestamp_name() -> String {
    let now = gst::glib::DateTime::now_local().ok();
    let stamp = now
        .and_then(|t| t.format("%Y%m%d_%H%M%S").ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("{}", monotonic_ns() / 1_000_000_000));
    format!("screencast{stamp}")
}

/// Where a recording is written until it is finished: outside the media
/// folders, so the media scanner and the gallery never see a partial file.
fn work_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("fp6-screenrecord")
}

/// Rename, or copy + remove when the work dir is on another file system.
fn move_file(from: &Path, to: &Path) -> io::Result<()> {
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {
            fs::copy(from, to)?;
            fs::remove_file(from)
        }
        Err(e) => Err(e),
    }
}

/// Earlier recordings that never got finished (crash, power loss): move them
/// to the output folder; they are fragmented MP4s, playable up to the last
/// fragment.
fn recover_parts(work: &Path, out: &Path) {
    let Ok(entries) = fs::read_dir(work) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some(stem) = name.strip_suffix(".mp4.part") {
            if stem.starts_with("screencast") {
                let target = out.join(format!("{stem}.mp4"));
                if !target.exists() && move_file(&entry.path(), &target).is_ok() {
                    eprintln!("fp6-screenrecord: recovered {}", target.display());
                }
            }
        }
    }
}

fn notify(summary: &str, body: &str, icon: &str) {
    if REPORT_STDOUT.load(Ordering::SeqCst) {
        use std::io::Write;
        let clean = |t: &str| t.replace(['\t', '\n'], " ");
        let mut out = io::stdout().lock();
        let _ = writeln!(out, "FP6-NOTIFY\t{}\t{}\t{}", clean(icon), clean(summary), clean(body));
        let _ = out.flush();
        return;
    }
    let result = (|| -> zbus::Result<()> {
        let bus = zbus::blocking::Connection::session()?;
        let hints: HashMap<&str, zbus::zvariant::Value> = HashMap::new();
        bus.call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &("Screen recorder", 0u32, icon, summary, body, Vec::<&str>::new(), hints, 5000i32),
        )?;
        Ok(())
    })();
    if let Err(e) = result {
        eprintln!("fp6-screenrecord: notification failed: {e}");
    }
}

fn duration_text(ns: i64) -> String {
    let s = (ns.max(0) / 1_000_000_000) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

fn main() {
    let o = parse_args();
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = on_signal as extern "C" fn(libc::c_int) as usize;
        libc::sigemptyset(&mut sa.sa_mask);
        sa.sa_flags = 0; // no SA_RESTART: a blocked read returns EINTR
        libc::sigaction(libc::SIGTERM, &sa, std::ptr::null_mut());
        libc::sigaction(libc::SIGINT, &sa, std::ptr::null_mut());
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
    if let Err(e) = gst::init() {
        eprintln!("fp6-screenrecord: GStreamer: {e}");
        std::process::exit(1);
    }
    let mut input = unsafe { fs::File::from_raw_fd(o.fd) };

    let first = match read_frame(&mut input) {
        Ok(Some(f)) => f,
        Ok(None) => {
            eprintln!("fp6-screenrecord: no frames, nothing saved");
            return;
        }
        Err(e) => {
            eprintln!("fp6-screenrecord: {e}");
            std::process::exit(1);
        }
    };
    let (width, height) = (first.width, first.height);
    if let Err(e) = fs::create_dir_all(&o.out_dir) {
        eprintln!("fp6-screenrecord: {}: {e}", o.out_dir.display());
        notify("Screen recording failed", &format!("Cannot create {}", o.out_dir.display()), "dialog-warning-symbolic");
        std::process::exit(1);
    }
    let work = work_dir();
    let _ = fs::create_dir_all(&work);
    recover_parts(&work, &o.out_dir);
    let name = timestamp_name();
    let part = work.join(format!("{name}.mp4.part"));
    let final_path = o.out_dir.join(format!("{name}.mp4"));

    let want_hw = match o.encoder.as_str() {
        "hw" => true,
        "sw" => false,
        _ => hw_encoder_available(),
    };
    let session = match build(&o, width, height, want_hw, &part) {
        Ok(s) => s,
        Err(e) if want_hw && o.encoder == "auto" => {
            eprintln!("fp6-screenrecord: hardware encoder: {e}; using x264");
            match build(&o, width, height, false, &part) {
                Ok(s) => s,
                Err(e) => fail(&e, &part),
            }
        }
        Err(e) => fail(&e, &part),
    };
    eprintln!(
        "fp6-screenrecord: {}x{} rotate {} at <= {} fps, {} encoder, microphone {}, system audio {} -> {}",
        width,
        height,
        o.rotate,
        o.fps,
        if session.hw { "Iris V4L2" } else { "x264" },
        o.microphone,
        o.system_audio,
        final_path.display()
    );

    let current = Arc::new(Mutex::new(session));
    let first_pts = Arc::new(Mutex::new(None::<i64>));
    let last_pts = Arc::new(Mutex::new(0i64));
    let input_done = Arc::new(AtomicBool::new(false));
    let received = Arc::new(AtomicU64::new(0));

    // Reader: frames -> appsrc (of whichever pipeline is current).
    let reader = {
        let current = current.clone();
        let first_pts = first_pts.clone();
        let last_pts = last_pts.clone();
        let input_done = input_done.clone();
        let received = received.clone();
        let debug = std::env::var_os("FP6_SCREENRECORD_DEBUG").is_some();
        std::thread::spawn(move || {
            let mut next = Some(first);
            loop {
                let frame = match next.take() {
                    Some(f) => f,
                    None => match read_frame(&mut input) {
                        Ok(Some(f)) => f,
                        Ok(None) => break,
                        Err(e) => {
                            eprintln!("fp6-screenrecord: input: {e}");
                            break;
                        }
                    },
                };
                received.fetch_add(1, Ordering::Relaxed);
                if debug {
                    eprintln!("fp6-screenrecord: frame pts {} ms (monotonic)", frame.pts_ns / 1_000_000);
                }
                if frame.width != width || frame.height != height || frame.stride != width * 4 {
                    // The output size changed mid-recording (should not
                    // happen on a phone): the encoder keeps the first size.
                    continue;
                }
                let session = current.lock().unwrap();
                let Some(base) = session.pipeline.base_time() else { continue };
                let mut pts = frame.pts_ns - base.nseconds() as i64;
                let mut last = last_pts.lock().unwrap();
                if pts <= *last {
                    pts = *last + 1;
                }
                *last = pts;
                first_pts.lock().unwrap().get_or_insert(pts);
                let mut buffer = gst::Buffer::from_mut_slice(frame.data);
                buffer.get_mut().unwrap().set_pts(gst::ClockTime::from_nseconds(pts as u64));
                let _ = session.appsrc.push_buffer(buffer);
            }
            input_done.store(true, Ordering::SeqCst);
        })
    };

    // Main: bus messages, stop requests, hardware-encoder fallback.
    let mut eos_sent = false;
    let mut eos_deadline: Option<Instant> = None;
    let mut failed: Option<String> = None;
    loop {
        if !eos_sent && (input_done.load(Ordering::SeqCst) || STOP.load(Ordering::SeqCst)) {
            let session = current.lock().unwrap();
            let _ = session.appsrc.end_of_stream();
            for name in ["mic", "system"] {
                if let Some(src) = session.pipeline.by_name(name) {
                    src.send_event(gst::event::Eos::new());
                }
            }
            eos_sent = true;
            eos_deadline = Some(Instant::now() + Duration::from_secs(10));
        }
        if let Some(deadline) = eos_deadline {
            if Instant::now() > deadline {
                eprintln!("fp6-screenrecord: no EOS after 10 s, closing the file as it is");
                break;
            }
        }
        let bus = current.lock().unwrap().pipeline.bus().unwrap();
        let Some(msg) = bus.timed_pop_filtered(
            gst::ClockTime::from_mseconds(200),
            &[gst::MessageType::Eos, gst::MessageType::Error, gst::MessageType::Warning],
        ) else {
            continue;
        };
        match msg.view() {
            gst::MessageView::Eos(_) => break,
            gst::MessageView::Warning(w) => {
                eprintln!("fp6-screenrecord: warning from {:?}: {}", w.src().map(|s| s.path_string()), w.error());
            }
            gst::MessageView::Error(err) => {
                let source = err.src().map(|s| s.path_string().to_string()).unwrap_or_default();
                eprintln!("fp6-screenrecord: error from {source}: {} ({:?})", err.error(), err.debug());
                let mut session = current.lock().unwrap();
                if session.hw && o.encoder == "auto" && session.encoded.load(Ordering::Relaxed) == 0 && !eos_sent {
                    eprintln!("fp6-screenrecord: hardware encoder failed before the first frame; using x264");
                    let _ = session.pipeline.set_state(gst::State::Null);
                    match build(&o, width, height, false, &part) {
                        Ok(s) => {
                            *session = s;
                            *last_pts.lock().unwrap() = 0;
                            *first_pts.lock().unwrap() = None;
                            continue;
                        }
                        Err(e) => {
                            failed = Some(e);
                            break;
                        }
                    }
                }
                failed = Some(err.error().to_string());
                break;
            }
            _ => {}
        }
    }
    let session = current.lock().unwrap();
    let _ = session.pipeline.set_state(gst::State::Null);
    let encoded = session.encoded.load(Ordering::Relaxed);
    drop(session);
    // Unblock the reader if it is still waiting for input (error path).
    unsafe { libc::shutdown(o.fd, libc::SHUT_RDWR) };
    let _ = reader.join();

    let duration = {
        let first = first_pts.lock().unwrap().unwrap_or(0);
        *last_pts.lock().unwrap() - first
    };
    if encoded == 0 {
        let _ = fs::remove_file(&part);
        let reason = failed.unwrap_or_else(|| "no video frames were encoded".into());
        eprintln!("fp6-screenrecord: nothing saved: {reason}");
        notify("Screen recording failed", &reason, "dialog-warning-symbolic");
        std::process::exit(1);
    }
    if let Err(e) = move_file(&part, &final_path) {
        eprintln!("fp6-screenrecord: rename: {e}");
        notify("Screen recording failed", &format!("Cannot save {}", final_path.display()), "dialog-warning-symbolic");
        std::process::exit(1);
    }
    let shown = final_path
        .strip_prefix(std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| final_path.display().to_string());
    eprintln!("fp6-screenrecord: saved {} ({}, {} frames from {} captured){}", final_path.display(), duration_text(duration), encoded,
              received.load(Ordering::Relaxed),
              failed.as_ref().map(|f| format!(" after an error: {f}")).unwrap_or_default());
    notify(
        "Screen recording saved",
        &format!("{shown} ({})", duration_text(duration)),
        "camera-video-symbolic",
    );
}

fn fail(e: &str, part: &Path) -> ! {
    eprintln!("fp6-screenrecord: {e}");
    let _ = fs::remove_file(part);
    notify("Screen recording failed", e, "dialog-warning-symbolic");
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn header(width: u32, height: u32, pts: i64) -> Vec<u8> {
        let mut h = Vec::new();
        h.extend_from_slice(MAGIC);
        for v in [1u32, width, height, width * 4, 0] {
            h.extend_from_slice(&v.to_le_bytes());
        }
        h.extend_from_slice(&pts.to_le_bytes());
        h
    }

    fn file_with(bytes: &[u8]) -> fs::File {
        let path = std::env::temp_dir().join(format!("fp6-screenrecord-test-{}-{}", std::process::id(), bytes.len()));
        fs::File::create(&path).unwrap().write_all(bytes).unwrap();
        let f = fs::File::open(&path).unwrap();
        let _ = fs::remove_file(&path);
        f
    }

    #[test]
    fn reads_frames_and_clean_eof() {
        let mut bytes = header(2, 2, 42);
        bytes.extend(std::iter::repeat(7u8).take(16));
        let mut f = file_with(&bytes);
        let frame = read_frame(&mut f).unwrap().unwrap();
        assert_eq!((frame.width, frame.height, frame.stride, frame.pts_ns), (2, 2, 8, 42));
        assert_eq!(frame.data, vec![7u8; 16]);
        assert!(read_frame(&mut f).unwrap().is_none());
    }

    #[test]
    fn truncated_frame_is_eof() {
        let mut bytes = header(4, 4, 1);
        bytes.extend_from_slice(&[0u8; 10]);
        assert!(read_frame(&mut file_with(&bytes)).unwrap().is_none());
    }

    #[test]
    fn rejects_garbage() {
        let mut bytes = header(4, 4, 1);
        bytes[0] = b'X';
        bytes.extend_from_slice(&[0u8; 64]);
        assert!(read_frame(&mut file_with(&bytes)).is_err());
        let mut bytes = header(0, 4, 1);
        bytes.extend_from_slice(&[0u8; 64]);
        assert!(read_frame(&mut file_with(&bytes)).is_err());
    }

    #[test]
    fn rotation_turns_the_picture_back() {
        assert_eq!(flip_method(0), "none");
        assert_eq!(flip_method(90), "counterclockwise");
        assert_eq!(flip_method(180), "rotate-180");
        assert_eq!(flip_method(270), "clockwise");
    }

    #[test]
    fn pipeline_has_requested_parts() {
        let mut o = Options {
            fd: 3, rotate: 90, fps: 30, microphone: false, system_audio: false,
            out_dir: PathBuf::from("/x"), encoder: "auto".into(), bitrate_kbit: 6000,
        };
        let d = pipeline_description(&o, 720, 1602, true, Path::new("/x/.a.mp4.part"));
        assert!(d.contains("v4l2h264enc") && d.contains("video_bitrate=6000000"));
        assert!(d.contains("width=720,height=1602") && d.contains("method=counterclockwise"));
        assert!(!d.contains("pulsesrc"));
        o.microphone = true;
        let d = pipeline_description(&o, 720, 1602, false, Path::new("/x/.a.mp4.part"));
        assert!(d.contains("x264enc") && d.contains("pulsesrc name=mic") && !d.contains("audiomixer"));
        o.system_audio = true;
        let d = pipeline_description(&o, 720, 1602, false, Path::new("/x/.a.mp4.part"));
        assert!(d.contains("audiomixer") && d.contains("device=@DEFAULT_MONITOR@"));
        assert_eq!(d.matches("amix.").count(), 2);
    }

    #[test]
    fn durations() {
        assert_eq!(duration_text(83_000_000_000), "1:23");
        assert_eq!(duration_text(3_723_000_000_000), "1:02:03");
    }
}
