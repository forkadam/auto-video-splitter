use std::collections::BTreeMap;
use std::fs;

use serde_json::json;

use crate::atomic::atomic_write_json;
use crate::keyframes::{load_keyframes, snap_preceding};
use crate::schemas::cuts::{parse_tc, CutsJson, Scene};
use crate::schemas::plan::{CutsResolvedJson, ResolvedScene};

use super::{StageContext, StageExecError, StageResult};

pub fn sanitize_label(label: Option<&str>) -> Option<String> {
    let raw = label?;
    if raw.is_empty() {
        return None;
    }
    let mapped: String = raw
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let collapsed: Vec<&str> = mapped.split('-').filter(|s| !s.is_empty()).collect();
    if collapsed.is_empty() {
        None
    } else {
        Some(collapsed.join("-"))
    }
}

pub async fn run(ctx: &mut StageContext<'_>) -> Result<StageResult, StageExecError> {
    let outdir = ctx.workspace_dir.join("plan");
    fs::create_dir_all(&outdir)?;

    let cuts_raw = fs::read_to_string(ctx.workspace_dir.join("decisions").join("cuts.json"))?;
    let cuts: CutsJson = serde_json::from_str(&cuts_raw)?;
    cuts.validate()
        .map_err(|e| StageExecError::Message(format!("cuts.json invalid: {e}")))?;

    let keyframes = load_keyframes(&ctx.workspace_dir.join("probe").join("keyframes.csv"))?;
    let kept: Vec<&Scene> = cuts.scenes.iter().filter(|s| s.keep).collect();

    // Merge adjacent kept scenes ONLY when prev.merge_with_next && s.id == prev.id + 1.
    let mut merged: Vec<Vec<&Scene>> = Vec::new();
    for s in kept {
        let join = merged
            .last()
            .and_then(|g| g.last())
            .map(|prev| prev.merge_with_next && s.id == prev.id + 1)
            .unwrap_or(false);
        if join {
            merged.last_mut().unwrap().push(s);
        } else {
            merged.push(vec![s]);
        }
    }

    let mut resolved = Vec::new();
    let mut dropped_degenerate: Vec<usize> = Vec::new();
    let mut out_index: usize = 1;

    for group in merged {
        let start_req =
            parse_tc(&group[0].start).map_err(|e| StageExecError::Message(e.to_string()))?;
        let end_req = parse_tc(&group[group.len() - 1].end)
            .map_err(|e| StageExecError::Message(e.to_string()))?;

        // Preserve live plan.py invariant: do NOT drop kept segments shorter than
        // plan.min_segment_length; only drop degenerate (end_req <= start_req) spans.
        if end_req <= start_req {
            dropped_degenerate.push(group[0].id);
            ctx.logger.warn(&format!(
                "plan: dropping scene {} — non-positive length ({start_req:.3}s -> {end_req:.3}s)",
                group[0].id
            ));
            continue;
        }

        let actual_start = snap_preceding(start_req, &keyframes);
        let actual_end = end_req;
        let label = sanitize_label(group[0].label.as_deref());
        let base = format!("{}-highlight-{out_index:02}", ctx.manifest.id);
        let fname = match &label {
            Some(l) => format!("{base}-{l}.{}", ctx.config.split.output_container),
            None => format!("{base}.{}", ctx.config.split.output_container),
        };
        let drift = ((start_req - actual_start) * 1_000_000.0).round() / 1_000_000.0;

        resolved.push(ResolvedScene {
            id: group[0].id,
            label,
            requested_start: start_req,
            requested_end: end_req,
            actual_start,
            actual_end,
            drift_seconds: drift,
            strategy: "stream_copy".to_string(),
            output_filename: fname,
        });
        out_index += 1;
    }

    let payload = CutsResolvedJson { scenes: resolved };
    atomic_write_json(&outdir.join("cuts-resolved.json"), &payload)?;

    let mut artifacts = BTreeMap::new();
    artifacts.insert("segments".into(), json!(payload.scenes.len()));
    artifacts.insert("dropped_degenerate".into(), json!(dropped_degenerate));
    Ok(StageResult::done(artifacts))
}
