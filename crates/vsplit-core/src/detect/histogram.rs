use super::{FrameDetector, FrameView};

/// Port of PySceneDetect `HistogramDetector` (scenedetect/detectors/histogram_detector.py):
/// 256-bin YUV Y-channel (luma) L2-normalized histogram compared via Pearson correlation.
pub struct HistogramDetector {
    corr_threshold: f64,
    bins: usize,
    min_scene_secs: f64,
    last_hist: Option<Vec<f64>>,
    last_cut_pts: Option<f64>,
}

impl HistogramDetector {
    pub fn new(threshold: f64, bins: usize, min_scene_secs: f64) -> Self {
        Self {
            corr_threshold: (1.0 - threshold).clamp(0.0, 1.0),
            bins: bins.max(2),
            min_scene_secs,
            last_hist: None,
            last_cut_pts: None,
        }
    }

    fn compute_luma_hist(&self, rgb: &[u8]) -> Vec<f64> {
        let mut hist = vec![0.0f64; self.bins];
        for c in rgb.as_chunks::<3>().0 {
            // BT.601 Y channel matching OpenCV COLOR_BGR2YUV.
            let y = ((77u32 * c[0] as u32 + 150u32 * c[1] as u32 + 29u32 * c[2] as u32 + 128) >> 8)
                .min(255) as usize;
            let bin = (y * self.bins) / 256;
            hist[bin] += 1.0;
        }
        // Match cv2.normalize(hist, hist) default NORM_L2.
        let norm = hist.iter().map(|v| v * v).sum::<f64>().sqrt();
        if norm > 0.0 {
            for v in &mut hist {
                *v /= norm;
            }
        }
        hist
    }

    fn pearson_correl(h1: &[f64], h2: &[f64]) -> f64 {
        let n = h1.len() as f64;
        let mean1 = h1.iter().sum::<f64>() / n;
        let mean2 = h2.iter().sum::<f64>() / n;

        let mut num = 0.0f64;
        let mut den1 = 0.0f64;
        let mut den2 = 0.0f64;
        for (&a, &b) in h1.iter().zip(h2.iter()) {
            let da = a - mean1;
            let db = b - mean2;
            num += da * db;
            den1 += da * da;
            den2 += db * db;
        }
        let denom = (den1 * den2).sqrt();
        if denom == 0.0 {
            if h1 == h2 {
                1.0
            } else {
                0.0
            }
        } else {
            num / denom
        }
    }
}

impl FrameDetector for HistogramDetector {
    fn process_frame(&mut self, frame: &FrameView<'_>) -> (Option<f64>, Vec<usize>) {
        let last_cut = *self.last_cut_pts.get_or_insert(frame.pts_seconds);
        let hist = self.compute_luma_hist(frame.rgb);
        let mut cuts = Vec::new();
        let mut score = None;

        if let Some(prev) = &self.last_hist {
            let corr = Self::pearson_correl(prev, &hist);
            score = Some(corr);
            if corr <= self.corr_threshold && (frame.pts_seconds - last_cut) >= self.min_scene_secs
            {
                cuts.push(frame.index);
                self.last_cut_pts = Some(frame.pts_seconds);
            }
        }

        self.last_hist = Some(hist);
        (score, cuts)
    }
}
