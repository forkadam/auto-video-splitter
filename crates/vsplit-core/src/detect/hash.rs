use std::f32::consts::PI;

use super::{FrameDetector, FrameView};

/// Port of PySceneDetect `HashDetector` (scenedetect/detectors/hash_detector.py):
/// 16x16 2D-DCT perceptual hash (pHash) on 32x32 area-averaged grayscale frames.
pub struct HashDetector {
    threshold: f64,
    hash_size: usize,
    lowpass: usize,
    min_scene_secs: f64,
    cos_table: Vec<f32>,
    last_hash: Option<Vec<bool>>,
    last_cut_pts: Option<f64>,
}

impl HashDetector {
    pub fn new(threshold: f64, hash_size: usize, lowpass: usize, min_scene_secs: f64) -> Self {
        let imsize = hash_size * lowpass;
        let cos_table = build_dct_matrix(imsize);
        Self {
            threshold,
            hash_size,
            lowpass,
            min_scene_secs,
            cos_table,
            last_hash: None,
            last_cut_pts: None,
        }
    }

    fn hash_frame(&self, rgb: &[u8], width: usize, height: usize) -> Vec<bool> {
        let imsize = self.hash_size * self.lowpass;
        let gray = rgb_to_gray(rgb);
        let resized = resize_area(&gray, width, height, imsize, imsize);

        let max_val = resized.iter().copied().max().unwrap_or(0).max(1) as f32;
        let norm: Vec<f32> = resized.iter().map(|&v| v as f32 / max_val).collect();

        let dct = dct_2d(&norm, imsize, &self.cos_table);

        // Extract top-left hash_size x hash_size low-frequency block.
        let mut low = Vec::with_capacity(self.hash_size * self.hash_size);
        for r in 0..self.hash_size {
            let row_off = r * imsize;
            low.extend_from_slice(&dct[row_off..row_off + self.hash_size]);
        }

        let mut sorted = low.clone();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let mid = sorted.len() / 2;
        let median = if sorted.len() % 2 == 0 {
            0.5 * (sorted[mid - 1] + sorted[mid])
        } else {
            sorted[mid]
        };

        low.into_iter().map(|v| v > median).collect()
    }
}

fn rgb_to_gray(rgb: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgb.len() / 3);
    for c in rgb.as_chunks::<3>().0 {
        let y = ((77u32 * c[0] as u32 + 150u32 * c[1] as u32 + 29u32 * c[2] as u32 + 128) >> 8)
            .min(255) as u8;
        out.push(y);
    }
    out
}

/// Exact area-averaged resize matching OpenCV `INTER_AREA` downscaling.
fn resize_area(src: &[u8], src_w: usize, src_h: usize, dst_w: usize, dst_h: usize) -> Vec<u8> {
    let mut dst = vec![0u8; dst_w * dst_h];
    if src_w == 0 || src_h == 0 {
        return dst;
    }
    let scale_x = src_w as f32 / dst_w as f32;
    let scale_y = src_h as f32 / dst_h as f32;

    for dy in 0..dst_h {
        let sy0 = dy as f32 * scale_y;
        let sy1 = ((dy + 1) as f32 * scale_y).min(src_h as f32);
        let y_start = sy0.floor() as usize;
        let y_end = (sy1.ceil() as usize).min(src_h);

        for dx in 0..dst_w {
            let sx0 = dx as f32 * scale_x;
            let sx1 = ((dx + 1) as f32 * scale_x).min(src_w as f32);
            let x_start = sx0.floor() as usize;
            let x_end = (sx1.ceil() as usize).min(src_w);

            let mut acc = 0.0f32;
            let mut weight_sum = 0.0f32;
            for sy in y_start..y_end {
                let wy = (sy1.min((sy + 1) as f32) - sy0.max(sy as f32)).max(0.0);
                let row = sy * src_w;
                for sx in x_start..x_end {
                    let wx = (sx1.min((sx + 1) as f32) - sx0.max(sx as f32)).max(0.0);
                    let w = wx * wy;
                    acc += src[row + sx] as f32 * w;
                    weight_sum += w;
                }
            }
            dst[dy * dst_w + dx] = if weight_sum > 0.0 {
                (acc / weight_sum).round().clamp(0.0, 255.0) as u8
            } else {
                0
            };
        }
    }
    dst
}

/// Precompute orthonormal 1D DCT-II basis matrix `M[k * n + i]`.
fn build_dct_matrix(n: usize) -> Vec<f32> {
    let mut m = vec![0.0f32; n * n];
    let nf = n as f32;
    let c0 = (1.0 / nf).sqrt();
    let ck = (2.0 / nf).sqrt();
    for k in 0..n {
        let alpha = if k == 0 { c0 } else { ck };
        for i in 0..n {
            let angle = PI * (2.0 * i as f32 + 1.0) * k as f32 / (2.0 * nf);
            m[k * n + i] = alpha * angle.cos();
        }
    }
    m
}

/// Separable orthonormal 2D DCT-II (`M * X * M^T`) matching `cv2.dct`.
fn dct_2d(input: &[f32], n: usize, basis: &[f32]) -> Vec<f32> {
    let mut tmp = vec![0.0f32; n * n];
    // 1D DCT along rows
    for r in 0..n {
        let row = &input[r * n..(r + 1) * n];
        for k in 0..n {
            let brow = &basis[k * n..(k + 1) * n];
            let mut sum = 0.0f32;
            for i in 0..n {
                sum += row[i] * brow[i];
            }
            tmp[r * n + k] = sum;
        }
    }
    // 1D DCT along columns
    let mut out = vec![0.0f32; n * n];
    for c in 0..n {
        for k in 0..n {
            let brow = &basis[k * n..(k + 1) * n];
            let mut sum = 0.0f32;
            for r in 0..n {
                sum += tmp[r * n + c] * brow[r];
            }
            out[k * n + c] = sum;
        }
    }
    out
}

impl FrameDetector for HashDetector {
    fn process_frame(&mut self, frame: &FrameView<'_>) -> (Option<f64>, Vec<usize>) {
        let last_cut = *self.last_cut_pts.get_or_insert(frame.pts_seconds);
        let curr_hash = self.hash_frame(frame.rgb, frame.width, frame.height);
        let mut cuts = Vec::new();
        let mut score = None;

        if let Some(prev) = &self.last_hash {
            let diff = curr_hash
                .iter()
                .zip(prev.iter())
                .filter(|&(a, b)| a != b)
                .count();
            let norm_dist = diff as f64 / (self.hash_size * self.hash_size) as f64;
            score = Some(norm_dist);
            if norm_dist >= self.threshold && (frame.pts_seconds - last_cut) >= self.min_scene_secs
            {
                cuts.push(frame.index);
                self.last_cut_pts = Some(frame.pts_seconds);
            }
        }

        self.last_hash = Some(curr_hash);
        (score, cuts)
    }
}
