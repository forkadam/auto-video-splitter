use super::{FrameDetector, FrameView};

/// Converts RGB24 pixels into OpenCV-compatible 8-bit HSV channels:
/// H in 0..=179, S in 0..=255, V in 0..=255.
fn rgb_to_hsv_planes(rgb: &[u8], hue: &mut Vec<u8>, sat: &mut Vec<u8>, lum: &mut Vec<u8>) {
    let px = rgb.len() / 3;
    hue.clear();
    sat.clear();
    lum.clear();
    hue.reserve(px);
    sat.reserve(px);
    lum.reserve(px);

    for chunk in rgb.as_chunks::<3>().0 {
        let r = chunk[0];
        let g = chunk[1];
        let b = chunk[2];

        let v = r.max(g).max(b);
        let min_c = r.min(g).min(b);
        let diff = v - min_c;

        let s = if v == 0 {
            0u8
        } else {
            (((diff as u32) * 255 + (v as u32) / 2) / (v as u32)).min(255) as u8
        };

        let h = if diff == 0 {
            0u8
        } else {
            let df = diff as f32;
            let mut h_deg = if v == r {
                60.0 * (g as f32 - b as f32) / df
            } else if v == g {
                120.0 + 60.0 * (b as f32 - r as f32) / df
            } else {
                240.0 + 60.0 * (r as f32 - g as f32) / df
            };
            if h_deg < 0.0 {
                h_deg += 360.0;
            }
            ((h_deg * 0.5).round() as i32).clamp(0, 179) as u8
        };

        hue.push(h);
        sat.push(s);
        lum.push(v);
    }
}

fn mean_pixel_distance(a: &[u8], b: &[u8]) -> f64 {
    debug_assert_eq!(a.len(), b.len());
    if a.is_empty() {
        return 0.0;
    }
    let sum: u64 = a
        .iter()
        .zip(b.iter())
        .map(|(&x, &y)| x.abs_diff(y) as u64)
        .sum();
    sum as f64 / a.len() as f64
}

pub struct ContentScorer {
    luma_only: bool,
    has_prev: bool,
    prev_hue: Vec<u8>,
    prev_sat: Vec<u8>,
    prev_lum: Vec<u8>,
    curr_hue: Vec<u8>,
    curr_sat: Vec<u8>,
    curr_lum: Vec<u8>,
}

impl ContentScorer {
    pub fn new(luma_only: bool) -> Self {
        Self {
            luma_only,
            has_prev: false,
            prev_hue: Vec::new(),
            prev_sat: Vec::new(),
            prev_lum: Vec::new(),
            curr_hue: Vec::new(),
            curr_sat: Vec::new(),
            curr_lum: Vec::new(),
        }
    }

    pub fn score_frame(&mut self, rgb: &[u8]) -> f64 {
        if self.luma_only {
            self.curr_lum.clear();
            self.curr_lum.reserve(rgb.len() / 3);
            for c in rgb.as_chunks::<3>().0 {
                self.curr_lum.push(c[0].max(c[1]).max(c[2]));
            }
            if !self.has_prev || self.prev_lum.len() != self.curr_lum.len() {
                std::mem::swap(&mut self.prev_lum, &mut self.curr_lum);
                self.has_prev = true;
                return 0.0;
            }
            let d = mean_pixel_distance(&self.curr_lum, &self.prev_lum);
            std::mem::swap(&mut self.prev_lum, &mut self.curr_lum);
            return d;
        }

        rgb_to_hsv_planes(
            rgb,
            &mut self.curr_hue,
            &mut self.curr_sat,
            &mut self.curr_lum,
        );
        if !self.has_prev || self.prev_lum.len() != self.curr_lum.len() {
            std::mem::swap(&mut self.prev_hue, &mut self.curr_hue);
            std::mem::swap(&mut self.prev_sat, &mut self.curr_sat);
            std::mem::swap(&mut self.prev_lum, &mut self.curr_lum);
            self.has_prev = true;
            return 0.0;
        }

        let dh = mean_pixel_distance(&self.curr_hue, &self.prev_hue);
        let ds = mean_pixel_distance(&self.curr_sat, &self.prev_sat);
        let dl = mean_pixel_distance(&self.curr_lum, &self.prev_lum);

        std::mem::swap(&mut self.prev_hue, &mut self.curr_hue);
        std::mem::swap(&mut self.prev_sat, &mut self.curr_sat);
        std::mem::swap(&mut self.prev_lum, &mut self.curr_lum);

        (dh + ds + dl) / 3.0
    }
}

/// Port of PySceneDetect `FlashFilter(Mode.MERGE)` (scenedetect/detector.py:189-224).
pub struct FlashFilterMerge {
    filter_secs: f64,
    last_above: Option<(usize, f64)>,
    merge_enabled: bool,
    merge_triggered: bool,
    merge_start_pts: f64,
}

impl FlashFilterMerge {
    pub fn new(filter_secs: f64) -> Self {
        Self {
            filter_secs,
            last_above: None,
            merge_enabled: false,
            merge_triggered: false,
            merge_start_pts: 0.0,
        }
    }

    pub fn filter(&mut self, frame_idx: usize, pts: f64, above_threshold: bool) -> Vec<usize> {
        if self.filter_secs <= 0.0 {
            return if above_threshold {
                vec![frame_idx]
            } else {
                vec![]
            };
        }
        let (_, last_pts) = *self.last_above.get_or_insert((frame_idx, pts));
        let min_length_met = (pts - last_pts) >= self.filter_secs;

        if above_threshold {
            self.last_above = Some((frame_idx, pts));
        }

        if self.merge_triggered {
            let (curr_last_idx, curr_last_pts) = self.last_above.unwrap();
            if min_length_met
                && !above_threshold
                && (curr_last_pts - self.merge_start_pts) >= self.filter_secs
            {
                self.merge_triggered = false;
                return vec![curr_last_idx];
            }
            return vec![];
        }

        if !above_threshold {
            return vec![];
        }

        if min_length_met {
            self.merge_enabled = true;
            return vec![frame_idx];
        }

        if self.merge_enabled {
            self.merge_triggered = true;
            self.merge_start_pts = pts;
        }
        vec![]
    }
}

pub struct ContentDetector {
    threshold: f64,
    scorer: ContentScorer,
    flash_filter: FlashFilterMerge,
}

impl ContentDetector {
    pub fn new(threshold: f64, min_scene_secs: f64, luma_only: bool) -> Self {
        Self {
            threshold,
            scorer: ContentScorer::new(luma_only),
            flash_filter: FlashFilterMerge::new(min_scene_secs),
        }
    }
}

impl FrameDetector for ContentDetector {
    fn process_frame(&mut self, frame: &FrameView<'_>) -> (Option<f64>, Vec<usize>) {
        let score = self.scorer.score_frame(frame.rgb);
        let above = score >= self.threshold;
        let cuts = self
            .flash_filter
            .filter(frame.index, frame.pts_seconds, above);
        (Some(score), cuts)
    }
}
