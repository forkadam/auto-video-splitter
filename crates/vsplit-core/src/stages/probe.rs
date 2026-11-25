use std::collections::BTreeMap;
use std::fs;

use serde_json::{json, Value};

use crate::atomic::{atomic_write_json, atomic_write_text};
use crate::process::run_command;

use super::{StageContext, StageExecError, StageResult};

fn parse_fps_fraction(s: &str) -> Option<f64> {
    let (num, den) = s.split_once('/')?;
    let n: i64 = num.trim().parse().ok()?;
    let d: i64 = den.trim().parse().ok()?;
    if d != 0 {
        Some(n as f64 / d as f64)
    } else {
        None
    }
}

pub async fn run(ctx: &mut StageContext<'_>) -> Result<StageResult, StageExecError> {
    let outdir = ctx.workspace_dir.join("probe");
    fs::create_dir_all(&outdir)?;
    let ffprobe = ctx.toolchain.ffprobe_path()?;
    let src = ctx.source_path.to_string_lossy().into_owned();

    ctx.logger.info(&format!("probe: running ffprobe on {src}"));
    let meta_out = run_command(
        ffprobe,
        &[
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_streams",
            "-show_format",
            &src,
        ],
        Some(&ctx.cancel),
    )
    .await?;
    let meta: Value = serde_json::from_slice(&meta_out.stdout)?;
    atomic_write_json(&outdir.join("ffprobe.json"), &meta)?;

    let kf_out = run_command(
        ffprobe,
        &[
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-skip_frame",
            "nokey",
            "-show_frames",
            "-show_entries",
            "frame=pts_time",
            "-of",
            "csv=p=0",
            &src,
        ],
        Some(&ctx.cancel),
    )
    .await?;
    let kf_text = String::from_utf8_lossy(&kf_out.stdout).into_owned();
    atomic_write_text(&outdir.join("keyframes.csv"), &kf_text)?;

    let streams = meta.get("streams").and_then(Value::as_array);
    let v_stream = streams.and_then(|arr| {
        arr.iter()
            .find(|s| s.get("codec_type").and_then(Value::as_str) == Some("video"))
    });
    let a_stream = streams.and_then(|arr| {
        arr.iter()
            .find(|s| s.get("codec_type").and_then(Value::as_str) == Some("audio"))
    });

    let fps = v_stream
        .and_then(|v| v.get("avg_frame_rate").and_then(Value::as_str))
        .and_then(parse_fps_fraction);
    let keyframe_count = kf_text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count() as u64;

    let duration_seconds = meta
        .get("format")
        .and_then(|f| f.get("duration"))
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|&d| d > 0.0);

    ctx.manifest.metadata.duration_seconds = duration_seconds;
    ctx.manifest.metadata.container = meta
        .get("format")
        .and_then(|f| f.get("format_name"))
        .and_then(Value::as_str)
        .map(ToString::to_string);
    ctx.manifest.metadata.video_codec = v_stream
        .and_then(|v| v.get("codec_name"))
        .and_then(Value::as_str)
        .map(ToString::to_string);
    ctx.manifest.metadata.audio_codec = a_stream
        .and_then(|a| a.get("codec_name"))
        .and_then(Value::as_str)
        .map(ToString::to_string);
    ctx.manifest.metadata.fps = fps;
    ctx.manifest.metadata.width = v_stream
        .and_then(|v| v.get("width"))
        .and_then(Value::as_u64)
        .map(|w| w as u32);
    ctx.manifest.metadata.height = v_stream
        .and_then(|v| v.get("height"))
        .and_then(Value::as_u64)
        .map(|h| h as u32);
    ctx.manifest.metadata.keyframe_count = Some(keyframe_count);

    ctx.logger.info(&format!(
        "probe: duration={:?} fps={:?} keyframes={keyframe_count}",
        duration_seconds, fps
    ));

    let mut artifacts = BTreeMap::new();
    artifacts.insert("keyframe_count".to_string(), json!(keyframe_count));
    Ok(StageResult::done(artifacts))
}
