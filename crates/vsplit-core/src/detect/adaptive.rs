use std::collections::VecDeque;

use super::content::ContentScorer;
use super::{FrameDetector, FrameView};

/// Port of PySceneDetect `AdaptiveDetector` (scenedetect/detectors/adaptive_detector.py).
pub struct AdaptiveDetector {
    adaptive_threshold: f64,
    min_scene_secs: f64,
    window_width: usize,
    min_content_val: f64,
    scorer: ContentScorer,
    buffer: VecDeque<(usize, f64, f64)>,
    last_cut_pts: Option<f64>,
}

impl AdaptiveDetector {
    pub fn new(
        adaptive_threshold: f64,
        min_scene_secs: f64,
        window_width: usize,
        min_content_val: f64,
        luma_only: bool,
    ) -> Self {
        let w = window_width.max(1);
        Self {
            adaptive_threshold,
            min_scene_secs,
            window_width: w,
            min_content_val,
            scorer: ContentScorer::new(luma_only),
            buffer: VecDeque::with_capacity(1 + 2 * w),
            last_cut_pts: None,
        }
    }
}

impl FrameDetector for AdaptiveDetector {
    fn process_frame(&mut self, frame: &FrameView<'_>) -> (Option<f64>, Vec<usize>) {
        let content_score = self.scorer.score_frame(frame.rgb);
        let last_cut = *self.last_cut_pts.get_or_insert(frame.pts_seconds);

        let required = 1 + 2 * self.window_width;
        self.buffer
            .push_back((frame.index, frame.pts_seconds, content_score));
        if self.buffer.len() < required {
            return (None, vec![]);
        }
        while self.buffer.len() > required {
            self.buffer.pop_front();
        }

        let (target_idx, target_pts, target_score) = self.buffer[self.window_width];
        let neighbor_sum: f64 = self
            .buffer
            .iter()
            .enumerate()
            .filter(|&(i, _)| i != self.window_width)
            .map(|(_, &(_, _, s))| s)
            .sum();
        let avg_window_score = neighbor_sum / (2.0 * self.window_width as f64);

        let avg_is_zero = avg_window_score.abs() < 1e-5;
        let adaptive_ratio = if !avg_is_zero {
            (target_score / avg_window_score).min(255.0)
        } else if target_score >= self.min_content_val {
            255.0
        } else {
            0.0
        };

        let threshold_met =
            adaptive_ratio >= self.adaptive_threshold && target_score >= self.min_content_val;
        let min_length_met = (frame.pts_seconds - last_cut) >= self.min_scene_secs;
        if threshold_met && min_length_met {
            self.last_cut_pts = Some(target_pts);
            return (Some(adaptive_ratio), vec![target_idx]);
        }

        (Some(adaptive_ratio), vec![])
    }
}
