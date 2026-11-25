use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use tokio_util::sync::CancellationToken;

use super::DetectError;
use crate::process::apply_no_window_std;

#[derive(Debug, Default, Clone)]
pub struct StreamTimings {
    pub fps: f64,
    pub duration_seconds: f64,
    pub frame_pts: Vec<f64>,
    pub last_frame_duration: f64,
}

#[derive(Default)]
struct SharedTimings {
    fps: Option<f64>,
    duration_seconds: Option<f64>,
    frame_pts: Vec<f64>,
    last_frame_duration: f64,
}

fn parse_hms_duration(s: &str) -> Option<f64> {
    let mut parts = s.trim().split(':');
    let h: f64 = parts.next()?.parse().ok()?;
    let m: f64 = parts.next()?.parse().ok()?;
    let sec: f64 = parts.next()?.parse().ok()?;
    Some(h * 3600.0 + m * 60.0 + sec)
}

fn parse_duration_line(line: &str) -> Option<f64> {
    let idx = line.find("Duration:")?;
    let rest = &line[idx + "Duration:".len()..];
    let token = rest.split(',').next()?.trim();
    parse_hms_duration(token)
}

fn parse_video_fps_line(line: &str) -> Option<f64> {
    if !line.contains("Video:") {
        return None;
    }
    for segment in line.split(',') {
        let seg = segment.trim();
        if let Some(num) = seg.strip_suffix(" fps") {
            if let Ok(v) = num.trim().parse::<f64>() {
                if v > 0.0 {
                    return Some(v);
                }
            }
        }
    }
    None
}

fn parse_showinfo_line(line: &str) -> Option<(Option<f64>, Option<f64>)> {
    if !line.contains("Parsed_showinfo") || !line.contains("pts_time:") {
        return None;
    }
    let pts = line
        .split("pts_time:")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|tok| tok.parse::<f64>().ok());
    let dur = line
        .split("duration_time:")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|tok| tok.parse::<f64>().ok());
    Some((pts, dur))
}

fn read_ppm_token<R: BufRead>(reader: &mut R) -> Result<Option<String>, DetectError> {
    let mut token = Vec::with_capacity(16);
    let mut byte = [0u8; 1];

    loop {
        match reader.read(&mut byte)? {
            0 => return Ok(None),
            _ => {
                let b = byte[0];
                if b == b'#' {
                    let mut discard = Vec::new();
                    reader.read_until(b'\n', &mut discard)?;
                    continue;
                }
                if !b.is_ascii_whitespace() {
                    token.push(b);
                    break;
                }
            }
        }
    }

    loop {
        match reader.read(&mut byte)? {
            0 => break,
            _ => {
                let b = byte[0];
                if b.is_ascii_whitespace() {
                    break;
                }
                if b == b'#' {
                    let mut discard = Vec::new();
                    reader.read_until(b'\n', &mut discard)?;
                    break;
                }
                token.push(b);
            }
        }
    }

    String::from_utf8(token)
        .map(Some)
        .map_err(|e| DetectError::InvalidPpm(e.to_string()))
}

/// Streams downscaled RGB24 PPM frames from a single `ffmpeg` process while extracting
/// per-frame `pts_time` via `showinfo=checksum=0` on stderr (zero extra `ffprobe` pass).
pub fn stream_ppm_frames<F>(
    ffmpeg_bin: &Path,
    source: &Path,
    max_width: u32,
    cancel: Option<&CancellationToken>,
    mut on_frame: F,
) -> Result<StreamTimings, DetectError>
where
    F: FnMut(usize, f64, usize, usize, &[u8]),
{
    if let Some(tok) = cancel {
        if tok.is_cancelled() {
            return Err(DetectError::Cancelled);
        }
    }
    let vf = format!(
        "scale=w=min(iw\\,{max_width}):h=-2:flags=fast_bilinear,format=rgb24,showinfo=checksum=0"
    );
    let mut cmd = Command::new(ffmpeg_bin);
    cmd.args(["-hide_banner", "-loglevel", "info", "-i"]);
    cmd.arg(source);
    cmd.args([
        "-fps_mode",
        "passthrough",
        "-vf",
        &vf,
        "-f",
        "image2pipe",
        "-vcodec",
        "ppm",
        "pipe:1",
    ]);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    apply_no_window_std(&mut cmd);

    let mut child = cmd.spawn().map_err(|e| DetectError::Spawn {
        tool: "ffmpeg",
        source: e,
    })?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| DetectError::InvalidPpm("missing stdout pipe".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| DetectError::InvalidPpm("missing stderr pipe".into()))?;

    let shared = Arc::new(Mutex::new(SharedTimings::default()));
    let shared_bg = Arc::clone(&shared);

    // Drain stderr concurrently so the OS pipe buffer cannot deadlock, parsing
    // input metadata and per-frame showinfo pts_time along the way.
    let stderr_handle = thread::spawn(move || {
        const MAX_DIAG: usize = 16 * 1024;
        let mut diag = String::new();
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        let mut saw_output = false;

        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    if let Some((pts_opt, dur_opt)) = parse_showinfo_line(&line) {
                        if let Ok(mut guard) = shared_bg.lock() {
                            let idx = guard.frame_pts.len();
                            let fps = guard.fps.unwrap_or(24.0);
                            let pts = pts_opt.unwrap_or_else(|| idx as f64 / fps);
                            guard.frame_pts.push(pts);
                            if let Some(d) = dur_opt {
                                guard.last_frame_duration = d;
                            }
                        }
                        continue;
                    }

                    if line.starts_with("Output #") {
                        saw_output = true;
                    }
                    if !saw_output {
                        if let Some(dur) = parse_duration_line(&line) {
                            if let Ok(mut guard) = shared_bg.lock() {
                                if guard.duration_seconds.is_none() {
                                    guard.duration_seconds = Some(dur);
                                }
                            }
                        }
                        if let Some(fps) = parse_video_fps_line(&line) {
                            if let Ok(mut guard) = shared_bg.lock() {
                                if guard.fps.is_none() {
                                    guard.fps = Some(fps);
                                }
                            }
                        }
                    }

                    diag.push_str(&line);
                    if diag.len() > MAX_DIAG * 2 {
                        let keep_from = diag.len() - MAX_DIAG;
                        let boundary = diag.ceil_char_boundary(keep_from);
                        diag.drain(..boundary);
                    }
                }
                Err(_) => break,
            }
        }
        if diag.len() > MAX_DIAG {
            let keep_from = diag.len() - MAX_DIAG;
            let boundary = diag.ceil_char_boundary(keep_from);
            diag.drain(..boundary);
        }
        diag
    });

    let mut reader = BufReader::with_capacity(256 * 1024, stdout);
    let mut rgb_buf: Vec<u8> = Vec::new();
    let mut frame_index: usize = 0;

    while let Some(magic) = read_ppm_token(&mut reader)? {
        if let Some(tok) = cancel {
            if tok.is_cancelled() {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stderr_handle.join();
                return Err(DetectError::Cancelled);
            }
        }
        if magic != "P6" {
            return Err(DetectError::InvalidPpm(format!(
                "expected P6 magic, got {magic:?}"
            )));
        }
        let w_str = read_ppm_token(&mut reader)?
            .ok_or_else(|| DetectError::InvalidPpm("unexpected EOF reading width".into()))?;
        let h_str = read_ppm_token(&mut reader)?
            .ok_or_else(|| DetectError::InvalidPpm("unexpected EOF reading height".into()))?;
        let max_str = read_ppm_token(&mut reader)?
            .ok_or_else(|| DetectError::InvalidPpm("unexpected EOF reading maxval".into()))?;

        let width: usize = w_str
            .parse()
            .map_err(|_| DetectError::InvalidPpm(format!("invalid width: {w_str}")))?;
        let height: usize = h_str
            .parse()
            .map_err(|_| DetectError::InvalidPpm(format!("invalid height: {h_str}")))?;
        let maxval: u32 = max_str
            .parse()
            .map_err(|_| DetectError::InvalidPpm(format!("invalid maxval: {max_str}")))?;
        if maxval != 255 {
            return Err(DetectError::InvalidPpm(format!(
                "unsupported PPM maxval: {maxval}"
            )));
        }

        let byte_len = width
            .checked_mul(height)
            .and_then(|px| px.checked_mul(3))
            .ok_or_else(|| DetectError::InvalidPpm("frame size overflow".into()))?;
        rgb_buf.resize(byte_len, 0);
        reader.read_exact(&mut rgb_buf)?;

        let pts_seconds = {
            let guard = shared.lock().unwrap();
            let fps = guard.fps.unwrap_or(24.0);
            guard
                .frame_pts
                .get(frame_index)
                .copied()
                .unwrap_or_else(|| frame_index as f64 / fps)
        };

        on_frame(frame_index, pts_seconds, width, height, &rgb_buf);
        frame_index += 1;
    }

    let status = child.wait()?;
    let stderr_tail = stderr_handle.join().unwrap_or_default();
    if !status.success() {
        return Err(DetectError::ProcessFailed {
            tool: "ffmpeg",
            code: status.code(),
            stderr: stderr_tail,
        });
    }

    let guard = shared.lock().unwrap();
    let fps = guard.fps.unwrap_or(24.0);
    let duration_seconds = guard.duration_seconds.unwrap_or(0.0);
    Ok(StreamTimings {
        fps,
        duration_seconds,
        frame_pts: guard.frame_pts.clone(),
        last_frame_duration: guard.last_frame_duration,
    })
}
