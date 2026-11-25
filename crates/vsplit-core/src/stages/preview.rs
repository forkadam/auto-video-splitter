use std::collections::BTreeMap;
use std::fs;

use minijinja::{context, AutoEscape, Environment};
use serde::Serialize;
use serde_json::json;

use crate::atomic::atomic_write_text;
use crate::keyframes::{load_keyframes, snap_preceding};
use crate::process::run_command;
use crate::schemas::cuts::format_tc;
use crate::schemas::scenes::ScenesJson;

use super::{StageContext, StageExecError, StageResult};

const PREVIEW_TEMPLATE: &str = include_str!("../../templates/preview_index.html.j2");

#[derive(Serialize)]
struct SceneTemplateRow {
    id: usize,
    id_padded: String,
    start_tc: String,
    end_tc: String,
    actual_start_tc: String,
    drift_seconds: f64,
    drift_str: String,
    duration: f64,
    duration_str: String,
    images: Vec<String>,
}

fn format_duration_label(duration: Option<f64>) -> Option<String> {
    let dur = duration?;
    let total = dur.round().max(0.0) as u64;
    let hours = total / 3600;
    let rem = total % 3600;
    let minutes = rem / 60;
    let secs = rem % 60;
    if hours > 0 {
        Some(format!("{hours}:{minutes:02}:{secs:02}"))
    } else {
        Some(format!("{minutes}:{secs:02}"))
    }
}

pub async fn run(ctx: &mut StageContext<'_>) -> Result<StageResult, StageExecError> {
    let outdir = ctx.workspace_dir.join("preview");
    let imgs = outdir.join("images");
    fs::create_dir_all(&imgs)?;

    let ffmpeg = ctx.toolchain.ffmpeg_path()?;
    let src = ctx.source_path.to_string_lossy().into_owned();

    let scenes_raw = fs::read_to_string(ctx.workspace_dir.join("detect").join("scenes.json"))?;
    let scenes_data: ScenesJson = serde_json::from_str(&scenes_raw)?;

    let n = ctx.config.preview.images_per_scene.max(1);
    let w = ctx.config.preview.image_width;
    let q = ctx.config.preview.image_quality;
    let qv = (31i64 - ((q as f64 / 100.0) * 29.0) as i64).clamp(2, 31);
    let qv_str = qv.to_string();
    let vf_thumb = format!("scale={w}:-1,format=yuvj420p");

    let keyframes = load_keyframes(&ctx.workspace_dir.join("probe").join("keyframes.csv"))?;

    let mut scenes_for_template = Vec::with_capacity(scenes_data.scenes.len());
    for scene in &scenes_data.scenes {
        let duration = scene.end_time - scene.start_time;
        let safe_end = scene.end_time - (0.1f64).min(duration * 0.05);
        let n_eff = if duration < 1.0 { 1 } else { n };
        let sample_ts: Vec<f64> = if n_eff > 1 {
            (0..n_eff)
                .map(|i| {
                    let frac = i as f64 / (n_eff - 1) as f64;
                    (scene.start_time + duration * frac).min(safe_end)
                })
                .collect()
        } else {
            vec![scene.start_time + duration / 2.0]
        };

        let mut img_names = Vec::new();
        for (k, t) in sample_ts.iter().enumerate() {
            if ctx.cancel.is_cancelled() {
                return Err(StageExecError::Cancelled);
            }
            let name = format!("scene-{:03}-{:02}.jpg", scene.id, k + 1);
            let out_path = imgs.join(&name);
            let out_str = out_path.to_string_lossy().into_owned();
            let ss_str = format!("{t:.3}");
            run_command(
                ffmpeg,
                &[
                    "-y",
                    "-loglevel",
                    "error",
                    "-ss",
                    &ss_str,
                    "-i",
                    &src,
                    "-frames:v",
                    "1",
                    "-vf",
                    &vf_thumb,
                    "-q:v",
                    &qv_str,
                    &out_str,
                ],
                Some(&ctx.cancel),
            )
            .await?;

            if out_path.metadata().map(|m| m.len() > 0).unwrap_or(false) {
                img_names.push(name);
            }
        }

        let actual_start = snap_preceding(scene.start_time, &keyframes);
        let drift = scene.start_time - actual_start;
        scenes_for_template.push(SceneTemplateRow {
            id: scene.id,
            id_padded: format!("{:03}", scene.id),
            start_tc: format_tc(scene.start_time),
            end_tc: format_tc(scene.end_time),
            actual_start_tc: format_tc(actual_start),
            drift_seconds: drift,
            drift_str: format!("{drift:.2}"),
            duration,
            duration_str: format!("{duration:.2}"),
            images: img_names,
        });
    }

    let proxy_height = ctx.config.preview.proxy_height.unwrap_or(0);
    let proxy_name = "source-proxy.mp4";
    let mut has_proxy = false;
    if proxy_height > 0 {
        if ctx.cancel.is_cancelled() {
            return Err(StageExecError::Cancelled);
        }
        let proxy_path = outdir.join(proxy_name);
        let proxy_str = proxy_path.to_string_lossy().into_owned();
        let vf_proxy = format!("scale=-2:{proxy_height},format=yuv420p");
        ctx.logger.info(&format!(
            "preview: generating H.264 proxy ({proxy_height}p) -> {proxy_name}"
        ));
        run_command(
            ffmpeg,
            &[
                "-y",
                "-loglevel",
                "error",
                "-i",
                &src,
                "-vf",
                &vf_proxy,
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-crf",
                "28",
                "-c:a",
                "aac",
                "-b:a",
                "96k",
                "-movflags",
                "+faststart",
                &proxy_str,
            ],
            Some(&ctx.cancel),
        )
        .await?;
        has_proxy = true;
    }

    let duration_label = format_duration_label(ctx.manifest.metadata.duration_seconds);

    let mut env = Environment::new();
    env.set_auto_escape_callback(|_| AutoEscape::Html);
    env.add_template("preview_index.html.j2", PREVIEW_TEMPLATE)
        .map_err(|e| StageExecError::Message(format!("template compile error: {e}")))?;
    let tmpl = env
        .get_template("preview_index.html.j2")
        .map_err(|e| StageExecError::Message(format!("template load error: {e}")))?;
    let rendered = tmpl
        .render(context! {
            slug => &ctx.manifest.id,
            source_rel => &ctx.manifest.source.relative_path,
            scenes => &scenes_for_template,
            has_proxy => has_proxy,
            proxy_name => proxy_name,
            duration_label => duration_label,
        })
        .map_err(|e| StageExecError::Message(format!("template render error: {e}")))?;

    atomic_write_text(&outdir.join("index.html"), &rendered)?;

    let images_generated: usize = scenes_for_template.iter().map(|s| s.images.len()).sum();
    let mut artifacts = BTreeMap::new();
    artifacts.insert("images_generated".to_string(), json!(images_generated));
    artifacts.insert("proxy_generated".to_string(), json!(has_proxy));
    Ok(StageResult::done(artifacts))
}
