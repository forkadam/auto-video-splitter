use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use vsplit_core::detect::{detect_scenes, DetectOptions, DetectorKind};

#[derive(Deserialize)]
struct GoldenFile {
    fixture: String,
    runs: BTreeMap<String, GoldenRun>,
}

#[derive(Deserialize)]
struct GoldenRun {
    detector: DetectorKind,
    threshold: f64,
    luma_only: bool,
    elapsed_ms: f64,
    scenes: Vec<GoldenScene>,
}

#[derive(Deserialize)]
struct GoldenScene {
    id: usize,
    start_time: f64,
    end_time: f64,
    start_frame: usize,
    end_frame: usize,
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests")
        .join("fixtures")
}

fn assert_golden_file(golden_name: &str, vfr_expected_frames: Option<&[(usize, usize)]>) {
    let dir = fixtures_dir();
    let golden_path = dir.join("golden").join(golden_name);
    let raw = std::fs::read_to_string(&golden_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", golden_path.display()));
    let golden: GoldenFile = serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("failed to parse {}: {e}", golden_path.display()));
    let clip_path = dir.join(&golden.fixture);
    assert!(
        clip_path.exists(),
        "missing fixture {} (run tests/fixtures/make_fixtures.py)",
        clip_path.display()
    );

    for (run_key, expected) in &golden.runs {
        let opts = DetectOptions {
            detector: expected.detector,
            threshold: expected.threshold,
            min_scene_length_secs: 0.6,
            luma_only: expected.luma_only,
            max_width: 320,
            ..DetectOptions::default()
        };
        let actual = detect_scenes(&clip_path, &opts).unwrap_or_else(|e| {
            panic!(
                "detect_scenes failed for {} / {run_key}: {e}",
                golden.fixture
            )
        });

        println!(
            "[Gate A] {:<16} {:<13} scenes={}/{} rust={:.1}ms py={:.1}ms fps={:.0} ({}x{}) scores=[{:.2}, p50={:.2}, p95={:.2}, max={:.2}]",
            golden.fixture,
            run_key,
            actual.scenes.len(),
            expected.scenes.len(),
            actual.stats.elapsed_ms,
            expected.elapsed_ms,
            actual.stats.decode_fps,
            actual.stats.frame_width,
            actual.stats.frame_height,
            actual.stats.score_min,
            actual.stats.score_p50,
            actual.stats.score_p95,
            actual.stats.score_max,
        );

        assert_eq!(
            actual.scenes.len(),
            expected.scenes.len(),
            "{} / {run_key}: scene count mismatch (actual={:?}, expected={:?})",
            golden.fixture,
            actual.scenes,
            expected
                .scenes
                .iter()
                .map(|s| (s.start_frame, s.end_frame))
                .collect::<Vec<_>>()
        );

        for (idx, (a, e)) in actual.scenes.iter().zip(expected.scenes.iter()).enumerate() {
            assert_eq!(a.id, e.id);
            let start_time_delta = (a.start_time - e.start_time).abs();
            let end_time_delta = (a.end_time - e.end_time).abs();
            assert!(
                start_time_delta <= 0.08 && end_time_delta <= 0.08,
                "{} / {run_key} scene {}: time delta exceeded +/-0.08s (actual={:.3}..{:.3}, expected={:.3}..{:.3})",
                golden.fixture,
                e.id,
                a.start_time,
                a.end_time,
                e.start_time,
                e.end_time
            );

            // On CFR clips compare directly to PySceneDetect frame numbers.
            // On VFR clips PySceneDetect's OpenCV backend reports synthetic avg_fps frame numbers
            // (e.g. round(2.0 * 23.39) = 47), whereas Rust decodes true VFR frames (60, 108, 138);
            // verify against the true VFR decoded frame boundaries in addition to <= +/-0.08s PTS parity.
            let (exp_start_frame, exp_end_frame) = match vfr_expected_frames {
                Some(frames) => frames[idx],
                None => (e.start_frame, e.end_frame),
            };
            let start_frame_delta = a.start_frame.abs_diff(exp_start_frame);
            let end_frame_delta = a.end_frame.abs_diff(exp_end_frame);
            assert!(
                start_frame_delta <= 2 && end_frame_delta <= 2,
                "{} / {run_key} scene {}: frame delta exceeded +/-2 (actual={}..{}, expected={}..{})",
                golden.fixture,
                e.id,
                a.start_frame,
                a.end_frame,
                exp_start_frame,
                exp_end_frame
            );
        }
    }
}

#[test]
fn gate_a_short_mp4_golden_parity() {
    assert_golden_file("short_mp4.json", None);
}

#[test]
fn gate_a_short_mkv_golden_parity() {
    assert_golden_file("short_mkv.json", None);
}

#[test]
fn gate_a_vfr_hevc10_mov_golden_parity() {
    // True VFR frame counts: 2s@30fps=60, 2s@24fps=48 (108), 2s@15fps=30 (138).
    assert_golden_file(
        "vfr_hevc10_mov.json",
        Some(&[(0, 60), (60, 108), (108, 138)]),
    );
}

#[test]
#[ignore = "Set VSPLIT_TEST_MOV=/path/to/iphone.mov to benchmark a real 10-bit HEVC iPhone clip"]
fn gate_a_real_iphone_mov_smoke() {
    let path = std::env::var("VSPLIT_TEST_MOV").expect("VSPLIT_TEST_MOV env var not set");
    for (kind, thresh) in [
        (DetectorKind::Content, 27.0),
        (DetectorKind::Adaptive, 3.0),
        (DetectorKind::Histogram, 0.05),
        (DetectorKind::Hash, 0.395),
    ] {
        let out = detect_scenes(
            Path::new(&path),
            &DetectOptions {
                detector: kind,
                threshold: thresh,
                ..DetectOptions::default()
            },
        )
        .expect("detect_scenes failed");
        println!(
            "[Real iPhone] {:?}: {} scenes in {:.1}ms ({:.0} fps, {} frames)",
            kind,
            out.scenes.len(),
            out.stats.elapsed_ms,
            out.stats.decode_fps,
            out.stats.frames_decoded
        );
    }
}
