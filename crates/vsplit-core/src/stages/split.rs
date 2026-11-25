use std::collections::{BTreeMap, HashSet};
use std::fs;

use serde_json::json;

use crate::process::run_command;
use crate::schemas::plan::CutsResolvedJson;

use super::{reset_stage_block, StageContext, StageExecError, StageResult};

pub async fn run(ctx: &mut StageContext<'_>) -> Result<StageResult, StageExecError> {
    let outdir = ctx.workspace_dir.join("output");
    fs::create_dir_all(&outdir)?;
    let ffmpeg = ctx.toolchain.ffmpeg_path()?;
    let src = ctx.source_path.to_string_lossy().into_owned();

    let plan_raw = fs::read_to_string(ctx.workspace_dir.join("plan").join("cuts-resolved.json"))?;
    let plan: CutsResolvedJson = serde_json::from_str(&plan_raw)?;

    let mut produced = Vec::with_capacity(plan.scenes.len());
    for seg in &plan.scenes {
        if ctx.cancel.is_cancelled() {
            return Err(StageExecError::Cancelled);
        }
        let out_path = outdir.join(&seg.output_filename);
        let out_str = out_path.to_string_lossy().into_owned();
        let ss = format!("{:.3}", seg.actual_start);
        let to = format!("{:.3}", seg.actual_end);
        let args = [
            "-y",
            "-loglevel",
            "error",
            "-ss",
            &ss,
            "-to",
            &to,
            "-i",
            &src,
            "-map",
            "0",
            "-map",
            "-0:d",
            "-c",
            "copy",
            "-avoid_negative_ts",
            "make_zero",
            &out_str,
        ];
        ctx.logger
            .info(&format!("split: ffmpeg {}", args.join(" ")));
        run_command(ffmpeg, &args, Some(&ctx.cancel)).await?;
        produced.push(seg.output_filename.clone());
    }

    let mut artifacts = BTreeMap::new();
    artifacts.insert("outputs".into(), json!(produced));
    Ok(StageResult::done(artifacts))
}

/// Validated recovery: keep probe-valid planned outputs, delete incomplete or unexpected files.
pub async fn recover(ctx: &mut StageContext<'_>) {
    let outdir = ctx.workspace_dir.join("output");
    let plan_path = ctx.workspace_dir.join("plan").join("cuts-resolved.json");
    if plan_path.is_file() && outdir.is_dir() {
        if let Ok(raw) = fs::read_to_string(&plan_path) {
            if let Ok(plan) = serde_json::from_str::<CutsResolvedJson>(&raw) {
                let expected: HashSet<String> =
                    plan.scenes.into_iter().map(|s| s.output_filename).collect();
                if let Ok(entries) = fs::read_dir(&outdir) {
                    for entry in entries.flatten() {
                        let p = entry.path();
                        let name = entry.file_name().to_string_lossy().into_owned();
                        if !expected.contains(&name) {
                            let _ = fs::remove_file(&p);
                            continue;
                        }
                        if let Ok(ffprobe) = ctx.toolchain.ffprobe_path() {
                            let p_str = p.to_string_lossy().into_owned();
                            if run_command(ffprobe, &["-v", "error", "-show_format", &p_str], None)
                                .await
                                .is_err()
                            {
                                let _ = fs::remove_file(&p);
                            }
                        }
                    }
                }
            }
        }
    }
    reset_stage_block(ctx.manifest, "split");
}
