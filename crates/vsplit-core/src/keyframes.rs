use std::fs;
use std::io;
use std::path::Path;

/// Parse `ffprobe -of csv=p=0` keyframe timestamps, tolerating extra fields (e.g. SEI annotations).
pub fn load_keyframes(path: &Path) -> io::Result<Vec<f64>> {
    let raw = fs::read_to_string(path)?;
    Ok(parse_keyframes_str(&raw))
}

pub fn parse_keyframes_str(raw: &str) -> Vec<f64> {
    let mut out = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(first) = trimmed.split(',').next() {
            if let Ok(v) = first.trim().parse::<f64>() {
                out.push(v);
            }
        }
    }
    out
}

/// Return the largest keyframe `<= t`, or `0.0` if none.
pub fn snap_preceding(t: f64, keyframes: &[f64]) -> f64 {
    keyframes.iter().copied().rfind(|&k| k <= t).unwrap_or(0.0)
}
