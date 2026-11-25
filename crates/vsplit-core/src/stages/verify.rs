use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::process::run_command;
use crate::schemas::plan::CutsResolvedJson;

use super::{StageContext, StageExecError, StageResult};

async fn probe_duration(
    ffprobe: &Path,
    file: &Path,
    cancel: &CancellationToken,
) -> Result<f64, StageExecError> {
    let path_str = file.to_string_lossy().into_owned();
    let out = run_command(
        ffprobe,
        &[
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            &path_str,
        ],
        Some(cancel),
    )
    .await?;
    let val: Value = serde_json::from_slice(&out.stdout)?;
    val.get("format")
        .and_then(|f| f.get("duration"))
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<f64>().ok())
        .ok_or_else(|| StageExecError::Message("missing format.duration in ffprobe output".into()))
}

pub async fn run(ctx: &mut StageContext<'_>) -> Result<StageResult, StageExecError> {
    let ffprobe = ctx.toolchain.ffprobe_path()?;
    let plan_raw = fs::read_to_string(ctx.workspace_dir.join("plan").join("cuts-resolved.json"))?;
    let plan: CutsResolvedJson = serde_json::from_str(&plan_raw)?;

    let tol = ctx.config.verify.duration_tolerance_seconds;
    let outdir = ctx.workspace_dir.join("output");
    let mut all_ok = true;
    let mut files = Vec::with_capacity(plan.scenes.len());

    for seg in &plan.scenes {
        if ctx.cancel.is_cancelled() {
            return Err(StageExecError::Cancelled);
        }
        let f = outdir.join(&seg.output_filename);
        let expected = seg.actual_end - seg.actual_start;
        if !f.exists() {
            all_ok = false;
            files.push(json!({
                "file": seg.output_filename,
                "expected_duration": expected,
                "status": "missing",
            }));
            continue;
        }

        match probe_duration(ffprobe, &f, &ctx.cancel).await {
            Ok(actual) => {
                let status = if (actual - expected).abs() > tol {
                    all_ok = false;
                    "duration_mismatch"
                } else {
                    "ok"
                };
                files.push(json!({
                    "file": seg.output_filename,
                    "expected_duration": expected,
                    "actual_duration": actual,
                    "status": status,
                }));
            }
            Err(e) if e.is_cancelled() => return Err(e),
            Err(_) => {
                all_ok = false;
                files.push(json!({
                    "file": seg.output_filename,
                    "expected_duration": expected,
                    "status": "unreadable",
                }));
            }
        }
    }

    // Preserve live verify.py semantics: returns `done` even when `report.all_ok == false`.
    let mut artifacts = BTreeMap::new();
    artifacts.insert(
        "report".into(),
        json!({
            "all_ok": all_ok,
            "files": files,
        }),
    );
    Ok(StageResult::done(artifacts))
}
