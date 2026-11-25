pub mod adaptive;
pub mod content;
pub mod hash;
pub mod histogram;
pub mod pipe;

use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use adaptive::AdaptiveDetector;
use content::ContentDetector;
use hash::HashDetector;
use histogram::HistogramDetector;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DetectorKind {
    Content,
    Adaptive,
    Histogram,
    Hash,
}

#[derive(Debug, Clone)]
pub struct DetectOptions {
    pub detector: DetectorKind,
    pub threshold: f64,
    pub min_scene_length_secs: f64,
    pub luma_only: bool,
    pub max_width: u32,
    pub ffmpeg_bin: Option<PathBuf>,
    pub cancel: Option<CancellationToken>,
}

impl Default for DetectOptions {
    fn default() -> Self {
        Self {
            detector: DetectorKind::Content,
            threshold: 27.0,
            min_scene_length_secs: 0.6,
            luma_only: false,
            max_width: 320,
            ffmpeg_bin: None,
            cancel: None,
        }
    }
}

impl DetectorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Content => "content",
            Self::Adaptive => "adaptive",
            Self::Histogram => "histogram",
            Self::Hash => "hash",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedScene {
    pub id: usize,
    pub start_time: f64,
    pub end_time: f64,
    pub start_frame: usize,
    pub end_frame: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectStats {
    pub frames_decoded: usize,
    pub elapsed_ms: f64,
    pub decode_fps: f64,
    pub frame_width: usize,
    pub frame_height: usize,
    pub score_min: f64,
    pub score_p50: f64,
    pub score_p95: f64,
    pub score_max: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectOutput {
    pub scenes: Vec<DetectedScene>,
    pub stats: DetectStats,
}

#[derive(Debug, Error)]
pub enum DetectError {
    #[error("failed to spawn {tool}: {source}")]
    Spawn {
        tool: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("{tool} failed (exit {code:?}): {stderr}")]
    ProcessFailed {
        tool: &'static str,
        code: Option<i32>,
        stderr: String,
    },
    #[error("invalid PPM stream from ffmpeg: {0}")]
    InvalidPpm(String),
    #[error("scene detection cancelled")]
    Cancelled,
    #[error("I/O error reading frame stream: {0}")]
    Io(#[from] std::io::Error),
}

pub struct FrameView<'a> {
    pub index: usize,
    pub pts_seconds: f64,
    pub width: usize,
    pub height: usize,
    pub rgb: &'a [u8],
}

pub trait FrameDetector {
    fn process_frame(&mut self, frame: &FrameView<'_>) -> (Option<f64>, Vec<usize>);
}

pub fn detect_scenes(source: &Path, opts: &DetectOptions) -> Result<DetectOutput, DetectError> {
    let t0 = Instant::now();
    // Use min_scene_length_secs directly (tiny epsilon guards float rounding at exact frame edges).
    let min_scene_secs = (opts.min_scene_length_secs - 1e-4).max(0.0);

    let mut detector: Box<dyn FrameDetector> = match opts.detector {
        DetectorKind::Content => Box::new(ContentDetector::new(
            opts.threshold,
            min_scene_secs,
            opts.luma_only,
        )),
        DetectorKind::Adaptive => Box::new(AdaptiveDetector::new(
            opts.threshold,
            min_scene_secs,
            2,
            15.0,
            opts.luma_only,
        )),
        DetectorKind::Histogram => {
            Box::new(HistogramDetector::new(opts.threshold, 256, min_scene_secs))
        }
        DetectorKind::Hash => Box::new(HashDetector::new(opts.threshold, 16, 2, min_scene_secs)),
    };

    let mut cuts: Vec<usize> = Vec::new();
    let mut scores: Vec<f64> = Vec::new();
    let mut frames_decoded: usize = 0;
    let mut last_width: usize = 0;
    let mut last_height: usize = 0;

    let ffmpeg_path = opts
        .ffmpeg_bin
        .as_deref()
        .unwrap_or_else(|| Path::new("ffmpeg"));
    let timings = pipe::stream_ppm_frames(
        ffmpeg_path,
        source,
        opts.max_width,
        opts.cancel.as_ref(),
        |index, pts_seconds, width, height, rgb| {
            let view = FrameView {
                index,
                pts_seconds,
                width,
                height,
                rgb,
            };
            let (score, new_cuts) = detector.process_frame(&view);
            if let Some(s) = score {
                scores.push(s);
            }
            cuts.extend(new_cuts);
            frames_decoded = index + 1;
            last_width = width;
            last_height = height;
        },
    )?;

    let fps = if timings.fps > 0.0 { timings.fps } else { 24.0 };

    cuts.sort_unstable();
    cuts.dedup();
    cuts.retain(|&c| c > 0 && c < frames_decoded);

    let total_duration = if timings.duration_seconds > 0.0 {
        timings.duration_seconds
    } else if let Some(&last_pts) = timings.frame_pts.last() {
        let tail = if timings.last_frame_duration > 0.0 {
            timings.last_frame_duration
        } else {
            1.0 / fps
        };
        last_pts + tail
    } else {
        frames_decoded as f64 / fps
    };

    let pts_at = |frame_idx: usize| -> f64 {
        if frame_idx >= frames_decoded {
            total_duration
        } else {
            timings
                .frame_pts
                .get(frame_idx)
                .copied()
                .unwrap_or_else(|| frame_idx as f64 / fps)
        }
    };

    let mut scenes = Vec::new();
    if !cuts.is_empty() && frames_decoded > 0 {
        let mut start_frame = 0usize;
        for (i, &cut_frame) in cuts.iter().enumerate() {
            scenes.push(DetectedScene {
                id: i + 1,
                start_time: pts_at(start_frame),
                end_time: pts_at(cut_frame),
                start_frame,
                end_frame: cut_frame,
            });
            start_frame = cut_frame;
        }
        scenes.push(DetectedScene {
            id: cuts.len() + 1,
            start_time: pts_at(start_frame),
            end_time: total_duration,
            start_frame,
            end_frame: frames_decoded,
        });
    }

    let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let decode_fps = if elapsed_ms > 0.0 {
        frames_decoded as f64 / (elapsed_ms / 1000.0)
    } else {
        0.0
    };

    scores.sort_by(|a, b| a.total_cmp(b));
    let quantile = |q: f64| -> f64 {
        if scores.is_empty() {
            0.0
        } else {
            let idx = ((scores.len() - 1) as f64 * q).round() as usize;
            scores[idx.min(scores.len() - 1)]
        }
    };

    Ok(DetectOutput {
        scenes,
        stats: DetectStats {
            frames_decoded,
            elapsed_ms,
            decode_fps,
            frame_width: last_width,
            frame_height: last_height,
            score_min: scores.first().copied().unwrap_or(0.0),
            score_p50: quantile(0.50),
            score_p95: quantile(0.95),
            score_max: scores.last().copied().unwrap_or(0.0),
        },
    })
}
