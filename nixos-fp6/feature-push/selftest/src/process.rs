use nix::{
    sys::signal::{killpg, Signal},
    unistd::Pid,
};
use std::{
    io::Read,
    os::unix::process::CommandExt,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub enum Failure {
    Timeout,
    Unavailable,
    Output,
}
/// Dedicated process group: deadlines also kill grandchildren (journalctl etc.).
/// Drain a bounded amount, then discard, so a noisy child cannot exhaust memory
/// or block on a full pipe. Never inherit stderr containing unredacted errors.
pub fn run(mut cmd: Command, timeout: Duration) -> Result<(bool, Vec<u8>), Failure> {
    cmd.stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .process_group(0);
    let mut child = cmd.spawn().map_err(|_| Failure::Unavailable)?;
    let pid = Pid::from_raw(child.id() as i32);
    let stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let mut input = stdout;
        let mut buf = [0u8; 8192];
        let mut overflow = false;
        loop {
            let n = input.read(&mut buf).map_err(|_| Failure::Output)?;
            if n == 0 {
                break;
            }
            if out.len() + n <= 2 * 1024 * 1024 {
                out.extend_from_slice(&buf[..n]);
            } else {
                overflow = true;
            }
        }
        if overflow {
            Err(Failure::Output)
        } else {
            Ok(out)
        }
    });
    let start = Instant::now();
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status.success()),
            Err(_) => break Err(Failure::Unavailable),
            _ if start.elapsed() >= timeout => break Err(Failure::Timeout),
            _ => std::thread::sleep(Duration::from_millis(10)),
        }
    };
    let _ = killpg(pid, Signal::SIGKILL);
    let _ = child.wait();
    let output = reader.join().map_err(|_| Failure::Output)?;
    result.and_then(|success| output.map(|bytes| (success, bytes)))
}

/// Used only inside a deadline-isolated worker. Inherit its process group so
/// the parent can kill the entire check tree, including this command.
pub fn capture(program: &str, args: &[&str]) -> Result<String, Failure> {
    let mut child = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .env("SYSTEMD_PAGER", "cat")
        .env("SYSTEMD_COLORS", "0")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|_| Failure::Unavailable)?;
    let mut bytes = Vec::new();
    let read = child
        .stdout
        .take()
        .unwrap()
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes);
    if read.is_err() || bytes.len() > 2 * 1024 * 1024 {
        let _ = child.kill();
        let _ = child.wait();
        return Err(Failure::Output);
    }
    if !child.wait().map_err(|_| Failure::Unavailable)?.success() {
        return Err(Failure::Output);
    }
    String::from_utf8(bytes).map_err(|_| Failure::Unavailable)
}
