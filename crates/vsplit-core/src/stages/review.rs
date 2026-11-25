use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use serde_json::json;

use crate::atomic::{atomic_write_json, atomic_write_text, robust_remove_dir_all};
use crate::hash::content_hash;
use crate::schemas::cuts::{format_tc, CutsJson, Scene};
use crate::schemas::manifest::StageStatus;
use crate::schemas::scenes::ScenesJson;

use super::{reset_stage_block, StageContext, StageExecError, StageResult};

pub fn validate_ids(parsed: &CutsJson, detected_ids: &BTreeSet<usize>) -> Result<(), String> {
    if detected_ids.is_empty() {
        return Ok(());
    }
    let cuts_ids: BTreeSet<usize> = parsed.scenes.iter().map(|s| s.id).collect();
    let missing: Vec<usize> = detected_ids.difference(&cuts_ids).copied().collect();
    if !missing.is_empty() {
        return Err(format!(
            "scene IDs {missing:?} from detect/scenes.json missing in cuts.json — use keep:false to drop a scene, don't remove the row"
        ));
    }
    let extra: Vec<usize> = cuts_ids.difference(detected_ids).copied().collect();
    if !extra.is_empty() {
        return Err(format!(
            "scene IDs {extra:?} in cuts.json not present in detect/scenes.json — add new scenes by editing detect/scenes.json then `reset --from detect`"
        ));
    }
    Ok(())
}

fn build_initial_cuts(scenes: &ScenesJson, default_keep: bool) -> CutsJson {
    CutsJson {
        schema_version: 1,
        scenes: scenes
            .scenes
            .iter()
            .map(|s| Scene {
                id: s.id,
                start: format_tc(s.start_time),
                end: format_tc(s.end_time),
                keep: default_keep,
                label: None,
                merge_with_next: false,
            })
            .collect(),
    }
}

pub async fn run(ctx: &mut StageContext<'_>) -> Result<StageResult, StageExecError> {
    let decisions_dir = ctx.workspace_dir.join("decisions");
    fs::create_dir_all(&decisions_dir)?;
    let cuts_path = decisions_dir.join("cuts.json");

    let scenes_raw = fs::read_to_string(ctx.workspace_dir.join("detect").join("scenes.json"))?;
    let scenes_data: ScenesJson = serde_json::from_str(&scenes_raw)?;
    let detected_ids: BTreeSet<usize> = scenes_data.scenes.iter().map(|s| s.id).collect();

    let is_first_entry = !cuts_path.exists();
    let template_path = ctx.working_root.join("cuts.json.template");

    if is_first_entry && ctx.config.review.auto_apply_template && template_path.exists() {
        let mut tpl_raw = fs::read_to_string(&template_path)?;
        if !tpl_raw.ends_with('\n') {
            tpl_raw.push('\n');
        }
        atomic_write_text(&cuts_path, &tpl_raw)?;
        let parsed: CutsJson = serde_json::from_str(&tpl_raw)
            .map_err(|e| StageExecError::Message(format!("cuts.json.template invalid: {e}")))?;
        parsed
            .validate()
            .map_err(|e| StageExecError::Message(format!("cuts.json.template invalid: {e}")))?;
        validate_ids(&parsed, &detected_ids).map_err(StageExecError::Message)?;

        let kept_final = parsed.scenes.iter().filter(|s| s.keep).count();
        let mut artifacts = BTreeMap::new();
        artifacts.insert("cuts_file_hash".into(), json!(content_hash(&cuts_path)?));
        artifacts.insert("scenes_total".into(), json!(parsed.scenes.len()));
        artifacts.insert("scenes_marked_keep_final".into(), json!(kept_final));
        artifacts.insert("applied_template".into(), json!(true));
        return Ok(StageResult::done(artifacts));
    }

    if is_first_entry {
        let initial = build_initial_cuts(&scenes_data, ctx.config.review.default_keep);
        atomic_write_json(&cuts_path, &initial)?;
        let kept_initial = initial.scenes.iter().filter(|s| s.keep).count();
        let mut artifacts = BTreeMap::new();
        artifacts.insert("cuts_file_hash".into(), json!(content_hash(&cuts_path)?));
        artifacts.insert("scenes_total".into(), json!(initial.scenes.len()));
        artifacts.insert("scenes_marked_keep_initial".into(), json!(kept_initial));
        return Ok(StageResult::awaiting_user(artifacts));
    }

    // Re-entry: check hash change or explicit approval.
    let current_hash = content_hash(&cuts_path)?;
    let existing_artifacts = ctx.manifest.stage("review").artifacts.clone();
    let recorded_hash = existing_artifacts
        .get("cuts_file_hash")
        .and_then(|v| v.as_str());
    let approved = existing_artifacts
        .get("approved_at")
        .map(|v| !v.is_null())
        .unwrap_or(false);

    if recorded_hash == Some(current_hash.as_str()) && !approved {
        return Ok(StageResult::awaiting_user(existing_artifacts));
    }

    let raw = fs::read_to_string(&cuts_path)?;
    let parsed: CutsJson = serde_json::from_str(&raw)
        .map_err(|e| StageExecError::Message(format!("cuts.json invalid: {e}")))?;
    parsed
        .validate()
        .map_err(|e| StageExecError::Message(format!("cuts.json invalid: {e}")))?;
    validate_ids(&parsed, &detected_ids).map_err(StageExecError::Message)?;

    let kept_final = parsed.scenes.iter().filter(|s| s.keep).count();
    let mut artifacts = existing_artifacts;
    artifacts.insert("cuts_file_hash".into(), json!(current_hash));
    artifacts.insert("scenes_marked_keep_final".into(), json!(kept_final));
    Ok(StageResult::done(artifacts))
}

/// Non-destructive recovery: if `decisions/cuts.json` exists and passes schema + ID validation,
/// preserve user edits and restore `review` to `awaiting_user` rather than wiping `decisions/`.
pub async fn recover(ctx: &mut StageContext<'_>) {
    let cuts_path = ctx.workspace_dir.join("decisions").join("cuts.json");
    let scenes_path = ctx.workspace_dir.join("detect").join("scenes.json");
    if cuts_path.is_file() {
        let valid = (|| -> Option<(CutsJson, String)> {
            let raw = fs::read_to_string(&cuts_path).ok()?;
            let parsed: CutsJson = serde_json::from_str(&raw).ok()?;
            parsed.validate().ok()?;
            if scenes_path.is_file() {
                let sraw = fs::read_to_string(&scenes_path).ok()?;
                let sdata: ScenesJson = serde_json::from_str(&sraw).ok()?;
                let ids: BTreeSet<usize> = sdata.scenes.iter().map(|s| s.id).collect();
                validate_ids(&parsed, &ids).ok()?;
            }
            let h = content_hash(&cuts_path).ok()?;
            Some((parsed, h))
        })();

        if let Some((parsed, h)) = valid {
            let block = ctx.manifest.stage_mut("review");
            block.status = StageStatus::AwaitingUser;
            block.error = None;
            block
                .artifacts
                .entry("cuts_file_hash".into())
                .or_insert_with(|| json!(h));
            block
                .artifacts
                .insert("scenes_total".into(), json!(parsed.scenes.len()));
            return;
        }
    }

    let _ = robust_remove_dir_all(&ctx.workspace_dir.join("decisions"));
    reset_stage_block(ctx.manifest, "review");
}
