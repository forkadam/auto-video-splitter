use std::path::Path;

use globset::{GlobBuilder, GlobMatcher};
use thiserror::Error;

use crate::manifest::load_manifest;
use crate::paths::workspace_dir;
use crate::schemas::manifest::{StageStatus, STAGE_NAMES};
use crate::stages::StageKind;

#[derive(Debug, Error)]
pub enum FilterError {
    #[error("filter must be key=value, got {0:?}")]
    MissingEquals(String),
    #[error("unknown filter key {0:?} (allowed: slug, status, stage)")]
    UnknownKey(String),
    #[error("unknown status value {0:?} (allowed: pending, running, done, failed, awaiting_user, skipped)")]
    InvalidStatus(String),
    #[error(
        "unknown stage value {0:?} (allowed: probe, detect, preview, review, plan, split, verify)"
    )]
    InvalidStage(String),
    #[error("invalid slug glob {pattern:?}: {source}")]
    InvalidGlob {
        pattern: String,
        #[source]
        source: globset::Error,
    },
}

#[derive(Debug, Clone)]
pub enum FilterPredicate {
    Slug {
        pattern: String,
        matcher: GlobMatcher,
    },
    Status(StageStatus),
    Stage(StageKind),
}

pub fn compile_slug_glob(pattern: &str) -> Result<FilterPredicate, FilterError> {
    let glob = GlobBuilder::new(pattern)
        .literal_separator(false)
        .build()
        .map_err(|e| FilterError::InvalidGlob {
            pattern: pattern.to_string(),
            source: e,
        })?;
    Ok(FilterPredicate::Slug {
        pattern: pattern.to_string(),
        matcher: glob.compile_matcher(),
    })
}

/// Parse a `key=value` filter (`slug=<glob>`, `status=<status>`, `stage=<stage>`),
/// rejecting unknown keys or invalid enum values cleanly.
pub fn parse_filter(raw: &str) -> Result<FilterPredicate, FilterError> {
    let Some((k, v)) = raw.split_once('=') else {
        return Err(FilterError::MissingEquals(raw.to_string()));
    };
    match k {
        "slug" => compile_slug_glob(v),
        "status" => StageStatus::parse(v)
            .map(FilterPredicate::Status)
            .ok_or_else(|| FilterError::InvalidStatus(v.to_string())),
        "stage" => StageKind::parse(v)
            .map(FilterPredicate::Stage)
            .ok_or_else(|| FilterError::InvalidStage(v.to_string())),
        other => Err(FilterError::UnknownKey(other.to_string())),
    }
}

/// Return the first stage in `STAGE_NAMES` whose status is not `done`.
pub fn current_stage_of(manifest: &crate::schemas::manifest::Manifest) -> Option<&'static str> {
    STAGE_NAMES
        .iter()
        .copied()
        .find(|&stage| manifest.stage(stage).status != StageStatus::Done)
}

/// Evaluate AND-combined predicates across candidate workspace slugs.
pub fn apply_filters(
    working_root: &Path,
    slugs: &[String],
    predicates: &[FilterPredicate],
) -> Vec<String> {
    let mut out: Vec<String> = slugs.to_vec();
    for pred in predicates {
        match pred {
            FilterPredicate::Slug { matcher, .. } => {
                out.retain(|s| matcher.is_match(s));
            }
            FilterPredicate::Status(want) => {
                out.retain(|s| {
                    load_manifest(&workspace_dir(working_root, s))
                        .map(|m| m.stages.values().any(|b| b.status == *want))
                        .unwrap_or(false)
                });
            }
            FilterPredicate::Stage(want) => {
                let want_name = want.name();
                out.retain(|s| {
                    load_manifest(&workspace_dir(working_root, s))
                        .map(|m| current_stage_of(&m) == Some(want_name))
                        .unwrap_or(false)
                });
            }
        }
    }
    out
}
