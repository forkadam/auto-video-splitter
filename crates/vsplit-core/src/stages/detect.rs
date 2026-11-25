use std::collections::BTreeMap;
use std::fs;

use serde_json::json;

use crate::atomic::atomic_write_json;
use crate::detect::{detect_scenes, DetectError, DetectOptions};
use crate::schemas::scenes::{SceneEntry, ScenesDetectorConfig, ScenesJson};

use super::{StageContext, StageExecError, StageResult};

pub async fn run(ctx: &mut StageContext<'_>) -> Result<StageResult, StageExecError> {
    let outdir = ctx.workspace_dir.join("detect");
    fs::create_dir_all(&outdir)?;

    let ffmpeg_bin = ctx.toolchain.ffmpeg_path()?.to_path_buf();
    let detector_kind = ctx.config.detect.detector;
    let threshold = ctx.config.detect.threshold;
    let opts = DetectOptions {
        detector: detector_kind,
        threshold,
        min_scene_length_secs: ctx.config.detect.min_scene_length,
        luma_only: ctx.config.detect.luma_only,
        max_width: 320,
        ffmpeg_bin: Some(ffmpeg_bin),
        cancel: Some(ctx.cancel.clone()),
    };
    let source_path = ctx.source_path.clone();

    let detect_out = tokio::task::spawn_blocking(move || detect_scenes(&source_path, &opts))
        .await
        .map_err(|e| StageExecError::Message(format!("detect worker panicked: {e}")))?
        .map_err(|e| match e {
            DetectError::Cancelled => StageExecError::Cancelled,
            other => StageExecError::Message(other.to_string()),
        })?;

    ctx.logger.info(&format!(
        "detect: detector={} threshold={} scenes={} frames={} elapsed_ms={:.1} decode_fps={:.1} scores=[min={:.2} p50={:.2} p95={:.2} max={:.2}]",
        detector_kind.as_str(),
        threshold,
        detect_out.scenes.len(),
        detect_out.stats.frames_decoded,
        detect_out.stats.elapsed_ms,
        detect_out.stats.decode_fps,
        detect_out.stats.score_min,
        detect_out.stats.score_p50,
        detect_out.stats.score_p95,
        detect_out.stats.score_max,
    ));

    let payload = ScenesJson {
        detector: detector_kind.as_str().to_string(),
        config: ScenesDetectorConfig { threshold },
        scenes: detect_out
            .scenes
            .iter()
            .map(|s| SceneEntry {
                id: s.id,
                start_time: s.start_time,
                end_time: s.end_time,
                start_frame: s.start_frame,
                end_frame: s.end_frame,
            })
            .collect(),
    };
    atomic_write_json(&outdir.join("scenes.json"), &payload)?;

    let mut artifacts = BTreeMap::new();
    artifacts.insert("scenes_found".to_string(), json!(payload.scenes.len()));
    artifacts.insert("detector".to_string(), json!(detector_kind.as_str()));
    Ok(StageResult::done(artifacts))
}
