use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, FixedOffset, Local, NaiveDateTime, TimeZone};
use globset::GlobBuilder;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::hash::partial_hash;
use crate::index::{index_path, load_index, save_index, IndexError};
use crate::locks::{
    inspect_workspace_lock, lock_index, try_acquire_workspace_lock, LockActor, LockError,
    LockLease, WorkspaceLockGuard,
};
use crate::manifest::{load_manifest, save_manifest, ManifestError};
use crate::paths::{resolve_rel_path, to_posix_rel_path, workspace_dir, VSPLIT_DIR};
use crate::process::run_command;
use crate::schemas::config::Config;
use crate::schemas::index::Index;
use crate::slugify::{resolve_collision, slugify, SlugError};
use crate::toolchain::{MediaToolchain, ToolchainError};

pub const DEFAULT_RENAME_FORMAT: &str = "%Y-%m-%d %H-%M-%S";

const PROBE_CONCURRENCY: usize = 4;

#[derive(Debug, Clone, Default)]
pub struct RenameOptions {
    pub recursive: Option<bool>,
    pub glob: Option<String>,
    pub format: Option<String>,
    pub dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameItem {
    pub old_path: String,
    pub new_path: String,
    pub source_tag: String,
    pub old_slug: Option<String>,
    pub new_slug: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenameSkipReason {
    AlreadyNamed,
    NoCreationDate,
    WorkspaceLocked,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameSkippedItem {
    pub path: String,
    pub reason: RenameSkipReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameCounts {
    pub renamed: usize,
    pub workspaces_synced: usize,
    pub skipped_already_named: usize,
    pub skipped_no_date: usize,
    pub skipped_locked: usize,
    pub skipped_error: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameReport {
    pub dry_run: bool,
    pub format: String,
    pub renamed: Vec<RenameItem>,
    pub skipped: Vec<RenameSkippedItem>,
    pub warnings: Vec<String>,
    pub counts: RenameCounts,
}

#[derive(Debug, Error)]
pub enum RenameError {
    #[error("invalid rename format {format:?}: {reason}")]
    InvalidFormat { format: String, reason: String },
    #[error("invalid glob pattern {pattern:?}: {source}")]
    InvalidGlob {
        pattern: String,
        #[source]
        source: globset::Error,
    },
    #[error("not a directory: {0}")]
    NotADirectory(PathBuf),
    #[error(transparent)]
    Toolchain(#[from] ToolchainError),
    #[error(transparent)]
    Lock(#[from] LockError),
    #[error(transparent)]
    Index(#[from] IndexError),
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error(transparent)]
    Slug(#[from] SlugError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
struct PlannedRename {
    old_abs: PathBuf,
    new_abs: PathBuf,
    old_path: String,
    new_path: String,
    new_file_name: String,
    source_tag: String,
    old_slug: Option<String>,
    projected_new_slug: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RenamePlan {
    format: String,
    has_index: bool,
    entries: Vec<PlannedRename>,
    skipped: Vec<RenameSkippedItem>,
    warnings: Vec<String>,
}

impl RenamePlan {
    /// Sorted, deduplicated workspace slugs that would be affected by this plan.
    pub fn affected_slugs(&self) -> Vec<String> {
        let mut slugs: Vec<String> = self
            .entries
            .iter()
            .filter_map(|e| e.old_slug.clone())
            .collect();
        slugs.sort();
        slugs.dedup();
        slugs
    }

    /// Convert this plan into a dry-run `RenameReport` without mutating disk.
    pub fn into_dry_run_report(self) -> RenameReport {
        let renamed: Vec<RenameItem> = self
            .entries
            .into_iter()
            .map(|e| RenameItem {
                old_path: e.old_path,
                new_path: e.new_path,
                source_tag: e.source_tag,
                old_slug: e.old_slug,
                new_slug: e.projected_new_slug,
            })
            .collect();
        let workspaces_synced = renamed.iter().filter(|r| r.old_slug.is_some()).count();
        let counts = build_counts(&renamed, workspaces_synced, &self.skipped);
        RenameReport {
            dry_run: true,
            format: self.format,
            renamed,
            skipped: self.skipped,
            warnings: self.warnings,
            counts,
        }
    }
}

fn is_forbidden_filename_char(c: char) -> bool {
    matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control()
}

/// Validate a `chrono::strftime` filename format string for cross-platform safety.
pub fn validate_format(fmt: &str) -> Result<(), RenameError> {
    if fmt.trim().is_empty() {
        return Err(RenameError::InvalidFormat {
            format: fmt.to_string(),
            reason: "format must not be empty or whitespace".to_string(),
        });
    }
    if fmt.chars().any(is_forbidden_filename_char) {
        return Err(RenameError::InvalidFormat {
            format: fmt.to_string(),
            reason: "format contains forbidden filename or control characters".to_string(),
        });
    }

    let sample = DateTime::parse_from_rfc3339("2026-05-01T14:23:05+08:00")
        .expect("static sample RFC3339 timestamp must parse");
    let mut rendered = String::new();
    if write!(&mut rendered, "{}", sample.format(fmt)).is_err() {
        return Err(RenameError::InvalidFormat {
            format: fmt.to_string(),
            reason: "invalid chrono format specifier".to_string(),
        });
    }
    if rendered.trim().is_empty() {
        return Err(RenameError::InvalidFormat {
            format: fmt.to_string(),
            reason: "rendered filename stem is empty or whitespace".to_string(),
        });
    }
    if rendered.chars().any(is_forbidden_filename_char) {
        return Err(RenameError::InvalidFormat {
            format: fmt.to_string(),
            reason: format!(
                "rendered filename {rendered:?} contains forbidden filename or control characters"
            ),
        });
    }
    if rendered.ends_with('.') || rendered.ends_with(' ') {
        return Err(RenameError::InvalidFormat {
            format: fmt.to_string(),
            reason: format!("rendered filename {rendered:?} must not end with '.' or space"),
        });
    }
    Ok(())
}

fn format_lock_detail(lease: &LockLease) -> String {
    format!(
        "locked by {} (pid {}, stage {})",
        lease.actor.as_str(),
        lease.pid,
        lease.stage
    )
}

fn get_tag_ci<'a>(tags: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a str> {
    tags.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .and_then(|(_, v)| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn parse_fixed_offset(raw: &str) -> Option<DateTime<FixedOffset>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        return Some(dt);
    }
    const PATTERNS: &[&str] = &[
        "%Y-%m-%dT%H:%M:%S%z",
        "%Y-%m-%dT%H:%M:%S%.f%z",
        "%Y-%m-%d %H:%M:%S%z",
        "%Y-%m-%d %H:%M:%S%.f%z",
        "%Y-%m-%d %H:%M:%S%:z",
        "%Y-%m-%d %H:%M:%S%.f%:z",
    ];
    for pat in PATTERNS {
        if let Ok(dt) = DateTime::parse_from_str(raw, pat) {
            return Some(dt);
        }
    }
    None
}

fn parse_naive_as_local(raw: &str) -> Option<DateTime<Local>> {
    const PATTERNS: &[&str] = &[
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
    ];
    for pat in PATTERNS {
        if let Ok(naive) = NaiveDateTime::parse_from_str(raw, pat) {
            if let Some(dt) = Local.from_local_datetime(&naive).earliest() {
                return Some(dt);
            }
        }
    }
    None
}

fn parse_local_timestamp(raw: &str) -> Option<DateTime<Local>> {
    if let Some(fixed) = parse_fixed_offset(raw) {
        return Some(fixed.with_timezone(&Local));
    }
    parse_naive_as_local(raw)
}

fn format_datetime<Tz: TimeZone>(dt: &DateTime<Tz>, fmt: &str) -> Option<String>
where
    Tz::Offset: std::fmt::Display,
{
    let mut out = String::new();
    write!(&mut out, "{}", dt.format(fmt)).ok()?;
    if out.trim().is_empty() {
        return None;
    }
    Some(out)
}

fn extract_creation_time_from_tags(
    tags: &serde_json::Map<String, Value>,
    fmt: &str,
) -> Option<String> {
    let raw = get_tag_ci(tags, "creation_time")?;
    let dt = parse_local_timestamp(raw)?;
    format_datetime(&dt, fmt)
}

/// Extract and format the creation timestamp from `ffprobe` JSON output.
///
/// Priority:
/// 1. `format.tags["com.apple.quicktime.creationdate"]` (preserves capture wall-clock offset)
/// 2. `format.tags["creation_time"]` (converted to local timezone)
/// 3. Any video stream `tags["creation_time"]` (converted to local timezone)
pub(crate) fn extract_creation_timestamp(
    ffprobe_json: &Value,
    fmt: &str,
) -> Option<(String, &'static str)> {
    if let Some(format_tags) = ffprobe_json
        .get("format")
        .and_then(|f| f.get("tags"))
        .and_then(Value::as_object)
    {
        if let Some(raw) = get_tag_ci(format_tags, "com.apple.quicktime.creationdate") {
            if let Some(dt) = parse_fixed_offset(raw) {
                if let Some(formatted) = format_datetime(&dt, fmt) {
                    return Some((formatted, "com.apple.quicktime.creationdate"));
                }
            }
        }

        if let Some(formatted) = extract_creation_time_from_tags(format_tags, fmt) {
            return Some((formatted, "creation_time"));
        }
    }

    if let Some(streams) = ffprobe_json.get("streams").and_then(Value::as_array) {
        for video_tags in streams
            .iter()
            .filter(|s| s.get("codec_type").and_then(Value::as_str) == Some("video"))
            .filter_map(|s| s.get("tags").and_then(Value::as_object))
        {
            if let Some(formatted) = extract_creation_time_from_tags(video_tags, fmt) {
                return Some((formatted, "creation_time"));
            }
        }
    }

    None
}

fn is_already_named(current_stem: &str, desired_stem: &str) -> bool {
    let cur = current_stem.to_lowercase();
    let want = desired_stem.to_lowercase();
    if cur == want {
        return true;
    }
    if let Some(rest) = cur.strip_prefix(&want) {
        if let Some(inner) = rest.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
            if !inner.is_empty()
                && !inner.starts_with('0')
                && inner.bytes().all(|b| b.is_ascii_digit())
            {
                if let Ok(n) = inner.parse::<u64>() {
                    return n >= 1;
                }
            }
        }
    }
    false
}

fn format_with_ext(stem: &str, ext: &str) -> String {
    if ext.is_empty() {
        stem.to_string()
    } else if ext.starts_with('.') {
        format!("{stem}{ext}")
    } else {
        format!("{stem}.{ext}")
    }
}

fn allocate_target_filename(
    desired_stem: &str,
    ext: &str,
    occupied_lower: &mut HashSet<String>,
) -> String {
    let base_candidate = format_with_ext(desired_stem, ext);
    let base_lower = base_candidate.to_lowercase();
    if occupied_lower.insert(base_lower) {
        return base_candidate;
    }

    let mut n: u64 = 1;
    loop {
        let candidate = format_with_ext(&format!("{desired_stem}({n})"), ext);
        let lower = candidate.to_lowercase();
        if occupied_lower.insert(lower) {
            return candidate;
        }
        n += 1;
    }
}

fn collect_video_candidates(
    working_root: &Path,
    dir: &Path,
    recursive: bool,
    extensions: &HashSet<String>,
    glob: Option<&globset::GlobMatcher>,
    out: &mut Vec<(PathBuf, String)>,
) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        if dir == working_root && file_name == VSPLIT_DIR {
            continue;
        }
        let ft = entry.file_type()?;
        if ft.is_dir() {
            if recursive {
                collect_video_candidates(working_root, &path, true, extensions, glob, out)?;
            }
            continue;
        }
        if !ft.is_file() {
            continue;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        let Some(ext) = ext else {
            continue;
        };
        if !extensions.contains(&ext) {
            continue;
        }
        if let Some(matcher) = glob {
            let Some(name) = path.file_name() else {
                continue;
            };
            if !matcher.is_match(name) {
                continue;
            }
        }
        if let Ok(rel_path) = path.strip_prefix(working_root) {
            let rel = to_posix_rel_path(rel_path);
            out.push((path, rel));
        }
    }
    Ok(())
}

fn try_load_cached_probe(
    working_root: &Path,
    slug: &str,
    abs_path: &Path,
    expected_size: u64,
) -> Option<Value> {
    let actual_size = fs::metadata(abs_path).ok()?.len();
    if actual_size != expected_size {
        return None;
    }
    let probe_path = workspace_dir(working_root, slug)
        .join("probe")
        .join("ffprobe.json");
    if !probe_path.is_file() {
        return None;
    }
    let raw = fs::read_to_string(&probe_path).ok()?;
    serde_json::from_str(&raw).ok()
}

struct CandidateMeta {
    abs_path: PathBuf,
    rel_path: String,
    old_slug: Option<String>,
}

struct ProbedCandidate {
    abs_path: PathBuf,
    rel_path: String,
    old_slug: Option<String>,
    desired_stem: String,
    source_tag: String,
}

fn build_counts(
    renamed: &[RenameItem],
    workspaces_synced: usize,
    skipped: &[RenameSkippedItem],
) -> RenameCounts {
    let mut counts = RenameCounts {
        renamed: renamed.len(),
        workspaces_synced,
        ..RenameCounts::default()
    };
    for item in skipped {
        match item.reason {
            RenameSkipReason::AlreadyNamed => counts.skipped_already_named += 1,
            RenameSkipReason::NoCreationDate => counts.skipped_no_date += 1,
            RenameSkipReason::WorkspaceLocked => counts.skipped_locked += 1,
            RenameSkipReason::Error => counts.skipped_error += 1,
        }
    }
    counts
}

fn record_probed_candidate(
    cand: CandidateMeta,
    json: &Value,
    format: &str,
    probed: &mut Vec<ProbedCandidate>,
    skipped: &mut Vec<RenameSkippedItem>,
) {
    match extract_creation_timestamp(json, format) {
        Some((desired_stem, source_tag)) => {
            probed.push(ProbedCandidate {
                abs_path: cand.abs_path,
                rel_path: cand.rel_path,
                old_slug: cand.old_slug,
                desired_stem,
                source_tag: source_tag.to_string(),
            });
        }
        None => {
            skipped.push(RenameSkippedItem {
                path: cand.rel_path,
                reason: RenameSkipReason::NoCreationDate,
                detail: None,
            });
        }
    }
}

fn match_candidates_to_workspaces(
    working_root: &Path,
    raw_candidates: Vec<(PathBuf, String)>,
    idx: &Index,
    skipped: &mut Vec<RenameSkippedItem>,
) -> Vec<CandidateMeta> {
    let by_path: HashMap<String, String> = idx
        .workspaces
        .iter()
        .map(|(slug, w)| (w.source_relative_path.clone(), slug.clone()))
        .collect();
    let by_hash: HashMap<String, String> = idx
        .workspaces
        .iter()
        .map(|(slug, w)| (w.partial_hash.clone(), slug.clone()))
        .collect();

    let mut matched: Vec<CandidateMeta> = Vec::with_capacity(raw_candidates.len());
    let mut claimed_slugs: HashSet<String> = HashSet::new();
    let mut unmatched_indices = Vec::new();

    // Primary pass: exact source_relative_path matches claim their workspace slug first.
    for (abs_path, rel_path) in raw_candidates {
        let old_slug = by_path.get(&rel_path).cloned();
        if let Some(ref slug) = old_slug {
            claimed_slugs.insert(slug.clone());
        } else if !idx.workspaces.is_empty() {
            unmatched_indices.push(matched.len());
        }
        matched.push(CandidateMeta {
            abs_path,
            rel_path,
            old_slug,
        });
    }

    // Fallback pass: partial_hash lookup only for unmatched files when workspaces exist,
    // and never steal a workspace whose recorded source file still exists on disk.
    let mut hash_error_indices: HashSet<usize> = HashSet::new();
    for idx_pos in unmatched_indices {
        let cand = &mut matched[idx_pos];
        match partial_hash(&cand.abs_path) {
            Ok(ph) => {
                if let Some(slug) = by_hash.get(&ph) {
                    let recorded_exists = idx
                        .workspaces
                        .get(slug)
                        .map(|w| resolve_rel_path(working_root, &w.source_relative_path).exists())
                        .unwrap_or(false);
                    if !recorded_exists && claimed_slugs.insert(slug.clone()) {
                        cand.old_slug = Some(slug.clone());
                    }
                }
            }
            Err(e) => {
                skipped.push(RenameSkippedItem {
                    path: cand.rel_path.clone(),
                    reason: RenameSkipReason::Error,
                    detail: Some(format!("failed to hash file: {e}")),
                });
                hash_error_indices.insert(idx_pos);
            }
        }
    }

    matched
        .into_iter()
        .enumerate()
        .filter_map(|(pos, cand)| (!hash_error_indices.contains(&pos)).then_some(cand))
        .collect()
}

async fn probe_uncached_candidates(
    to_probe: Vec<CandidateMeta>,
    ffprobe_path: PathBuf,
    format: &str,
    probed: &mut Vec<ProbedCandidate>,
    skipped: &mut Vec<RenameSkippedItem>,
) -> Result<(), RenameError> {
    let sem = Arc::new(Semaphore::new(PROBE_CONCURRENCY));
    let mut join_set: JoinSet<(usize, Result<Value, String>)> = JoinSet::new();

    for (i, cand) in to_probe.iter().enumerate() {
        let sem = Arc::clone(&sem);
        let ffprobe_bin = ffprobe_path.clone();
        let src_str = cand.abs_path.to_string_lossy().into_owned();
        join_set.spawn(async move {
            let Ok(_permit) = sem.acquire_owned().await else {
                return (i, Err("probe semaphore closed".to_string()));
            };
            let args = [
                "-v",
                "error",
                "-print_format",
                "json",
                "-show_format",
                "-show_streams",
                &src_str,
            ];
            let res = match run_command(&ffprobe_bin, &args, None).await {
                Ok(out) => serde_json::from_slice::<Value>(&out.stdout)
                    .map_err(|e| format!("invalid ffprobe JSON: {e}")),
                Err(e) => Err(format!("ffprobe failed: {e}")),
            };
            (i, res)
        });
    }

    let mut probe_results: Vec<Option<Result<Value, String>>> = vec![None; to_probe.len()];
    while let Some(joined) = join_set.join_next().await {
        match joined {
            Ok((i, res)) => probe_results[i] = Some(res),
            Err(e) => {
                return Err(RenameError::Io(std::io::Error::other(format!(
                    "probe worker panicked: {e}"
                ))));
            }
        }
    }

    for (cand, res_opt) in to_probe.into_iter().zip(probe_results) {
        let res = res_opt.unwrap_or_else(|| Err("missing probe result".to_string()));
        match res {
            Ok(json) => record_probed_candidate(cand, &json, format, probed, skipped),
            Err(detail) => {
                skipped.push(RenameSkippedItem {
                    path: cand.rel_path,
                    reason: RenameSkipReason::Error,
                    detail: Some(detail),
                });
            }
        }
    }
    Ok(())
}

fn plan_directory_targets(
    working_root: &Path,
    probed: Vec<ProbedCandidate>,
    skipped: &mut Vec<RenameSkippedItem>,
) -> Result<Vec<PlannedRename>, RenameError> {
    let mut moving_by_dir: BTreeMap<PathBuf, Vec<ProbedCandidate>> = BTreeMap::new();
    for item in probed {
        let current_stem = item
            .abs_path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if is_already_named(&current_stem, &item.desired_stem) {
            skipped.push(RenameSkippedItem {
                path: item.rel_path,
                reason: RenameSkipReason::AlreadyNamed,
                detail: None,
            });
            continue;
        }
        let parent_dir = item.abs_path.parent().unwrap_or(working_root).to_path_buf();
        moving_by_dir.entry(parent_dir).or_default().push(item);
    }

    let mut entries: Vec<PlannedRename> = Vec::new();
    for (parent_dir, mut items) in moving_by_dir {
        items.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));

        // Count lowercase filenames so case-sensitive filesystems keep an entry occupied
        // if another file with differing case is not moving.
        let mut occupied_counts: HashMap<String, usize> = HashMap::new();
        for dir_entry in fs::read_dir(&parent_dir)? {
            let dir_entry = dir_entry?;
            let lower = dir_entry.file_name().to_string_lossy().to_lowercase();
            *occupied_counts.entry(lower).or_insert(0) += 1;
        }
        for item in &items {
            if let Some(name) = item.abs_path.file_name() {
                let lower = name.to_string_lossy().to_lowercase();
                if let Some(count) = occupied_counts.get_mut(&lower) {
                    if *count <= 1 {
                        occupied_counts.remove(&lower);
                    } else {
                        *count -= 1;
                    }
                }
            }
        }
        let mut occupied_lower: HashSet<String> = occupied_counts.into_keys().collect();

        for item in items {
            let ext = item
                .abs_path
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy()))
                .unwrap_or_default();
            let new_file_name =
                allocate_target_filename(&item.desired_stem, &ext, &mut occupied_lower);
            let new_abs = parent_dir.join(&new_file_name);
            let new_rel = new_abs
                .strip_prefix(working_root)
                .map(to_posix_rel_path)
                .unwrap_or_else(|_| new_file_name.clone());

            entries.push(PlannedRename {
                old_abs: item.abs_path,
                new_abs,
                old_path: item.rel_path,
                new_path: new_rel,
                new_file_name,
                source_tag: item.source_tag,
                old_slug: item.old_slug,
                projected_new_slug: None,
            });
        }
    }

    entries.sort_by(|a, b| a.old_path.cmp(&b.old_path));
    Ok(entries)
}

fn resolve_target_slug(
    old_slug: &str,
    new_file_name: &str,
    reserved_slugs: &mut HashSet<String>,
    retired_slugs: &HashSet<String>,
) -> Result<String, SlugError> {
    let base = slugify(new_file_name);
    if base == old_slug {
        reserved_slugs.insert(old_slug.to_string());
        return Ok(old_slug.to_string());
    }
    let was_present = reserved_slugs.remove(old_slug);
    let resolved = resolve_collision(&base, reserved_slugs, retired_slugs);
    match resolved {
        Ok(new_slug) => {
            if was_present && new_slug != old_slug {
                reserved_slugs.insert(old_slug.to_string());
            }
            reserved_slugs.insert(new_slug.clone());
            Ok(new_slug)
        }
        Err(e) => {
            if was_present {
                reserved_slugs.insert(old_slug.to_string());
            }
            Err(e)
        }
    }
}

/// Discover videos, probe creation timestamps (using cached `ffprobe.json` when valid),
/// and build a collision-free rename plan without mutating disk.
pub async fn plan_renames(
    working_root: &Path,
    config: &Config,
    toolchain: &MediaToolchain,
    opts: &RenameOptions,
) -> Result<RenamePlan, RenameError> {
    if !working_root.is_dir() {
        return Err(RenameError::NotADirectory(working_root.to_path_buf()));
    }

    let format = opts
        .format
        .as_deref()
        .unwrap_or(DEFAULT_RENAME_FORMAT)
        .to_string();
    validate_format(&format)?;

    let glob_matcher = match opts.glob.as_deref() {
        Some(pattern) => Some(
            GlobBuilder::new(pattern)
                .case_insensitive(true)
                .literal_separator(false)
                .build()
                .map_err(|source| RenameError::InvalidGlob {
                    pattern: pattern.to_string(),
                    source,
                })?
                .compile_matcher(),
        ),
        None => None,
    };

    let recursive = opts.recursive.unwrap_or(config.scan.recursive);
    let extensions: HashSet<String> = config
        .scan
        .extensions
        .iter()
        .map(|e| e.trim_start_matches('.').to_ascii_lowercase())
        .collect();

    let mut raw_candidates = Vec::new();
    collect_video_candidates(
        working_root,
        working_root,
        recursive,
        &extensions,
        glob_matcher.as_ref(),
        &mut raw_candidates,
    )?;
    raw_candidates.sort_by(|a, b| a.1.cmp(&b.1));

    let has_index = index_path(working_root).is_file();
    let idx = if has_index {
        load_index(working_root)?
    } else {
        Index::default()
    };

    let mut skipped = Vec::new();
    let matched = match_candidates_to_workspaces(working_root, raw_candidates, &idx, &mut skipped);

    let mut probed: Vec<ProbedCandidate> = Vec::new();
    let mut to_probe: Vec<CandidateMeta> = Vec::new();

    for cand in matched {
        if let Some(ref slug) = cand.old_slug {
            if let Some(lease) = inspect_workspace_lock(working_root, slug) {
                skipped.push(RenameSkippedItem {
                    path: cand.rel_path,
                    reason: RenameSkipReason::WorkspaceLocked,
                    detail: Some(format_lock_detail(&lease)),
                });
                continue;
            }

            if let Some(ws_entry) = idx.workspaces.get(slug) {
                if let Some(cached_json) =
                    try_load_cached_probe(working_root, slug, &cand.abs_path, ws_entry.size_bytes)
                {
                    record_probed_candidate(cand, &cached_json, &format, &mut probed, &mut skipped);
                    continue;
                }
            }
        }
        to_probe.push(cand);
    }

    if !to_probe.is_empty() {
        let ffprobe_path = toolchain.ffprobe_path()?.to_path_buf();
        probe_uncached_candidates(to_probe, ffprobe_path, &format, &mut probed, &mut skipped)
            .await?;
    }

    let mut entries = plan_directory_targets(working_root, probed, &mut skipped)?;
    skipped.sort_by(|a, b| a.path.cmp(&b.path));

    // Compute projected workspace slugs (reserving all initial slugs to avoid mid-batch collisions,
    // while excluding the item's own `old_slug` during collision resolution so e.g. `foo-2` stays `foo-2`).
    let mut reserved_slugs: HashSet<String> = idx.workspaces.keys().cloned().collect();
    let retired_slugs: HashSet<String> = idx.retired_slugs.iter().cloned().collect();
    for entry in &mut entries {
        if let Some(ref old_slug) = entry.old_slug {
            let new_slug = resolve_target_slug(
                old_slug,
                &entry.new_file_name,
                &mut reserved_slugs,
                &retired_slugs,
            )?;
            entry.projected_new_slug = Some(new_slug);
        }
    }

    Ok(RenamePlan {
        format,
        has_index,
        entries,
        skipped,
        warnings: Vec::new(),
    })
}

/// Fixed-point pre-check before staging: remove any entry whose target currently exists on disk
/// and is NOT being vacated by another remaining executable entry (e.g., chain tail became locked
/// or a file appeared concurrently between plan and apply).
fn prune_occupied_targets(
    mut executable: Vec<PlannedRename>,
    skipped: &mut Vec<RenameSkippedItem>,
) -> Vec<PlannedRename> {
    loop {
        let exec_old_lower: HashSet<String> = executable
            .iter()
            .map(|e| e.old_abs.to_string_lossy().to_lowercase())
            .collect();
        let mut kept = Vec::with_capacity(executable.len());
        let mut removed_any = false;
        for entry in executable {
            let new_lower = entry.new_abs.to_string_lossy().to_lowercase();
            if entry.new_abs.exists() && !exec_old_lower.contains(&new_lower) {
                skipped.push(RenameSkippedItem {
                    path: entry.old_path,
                    reason: RenameSkipReason::Error,
                    detail: Some(format!("target path remained occupied: {}", entry.new_path)),
                });
                removed_any = true;
            } else {
                kept.push(entry);
            }
        }
        executable = kept;
        if !removed_any {
            return executable;
        }
    }
}

fn allocate_staging_path(parent: &Path, orig_name: &str) -> PathBuf {
    loop {
        let candidate = parent.join(format!(
            ".vsplit-rename-tmp-{:08x}-{orig_name}.tmp",
            rand::random::<u32>()
        ));
        if !candidate.exists() {
            return candidate;
        }
    }
}

fn restore_staged_non_destructive(
    staged_path: &Path,
    old_abs: &Path,
    old_rel: &str,
    warnings: &mut Vec<String>,
) {
    if !old_abs.exists() {
        if let Err(e) = fs::rename(staged_path, old_abs) {
            warnings.push(format!(
                "failed to restore staged file {} back to {old_rel} ({e}); staged file left intact at {}",
                staged_path.display(),
                staged_path.display()
            ));
        }
    } else {
        warnings.push(format!(
            "could not restore {old_rel} because original path is now occupied; staged file left intact at {}",
            staged_path.display()
        ));
    }
}

struct WorkspaceSyncOutcome {
    old_slug: Option<String>,
    new_slug: Option<String>,
    idx_modified: bool,
    synced: bool,
}

/// Synchronize a single adopted workspace after its source video has already been renamed.
/// Loads `manifest.json` BEFORE moving the workspace directory so corrupt/missing manifests
/// never leave a half-moved directory, and records warnings instead of aborting sibling syncs.
fn sync_renamed_workspace(
    working_root: &Path,
    plan_item: &PlannedRename,
    idx: &mut Index,
    reserved_slugs: &mut HashSet<String>,
    retired_slugs: &HashSet<String>,
    warnings: &mut Vec<String>,
) -> WorkspaceSyncOutcome {
    let Some(ref old_slug) = plan_item.old_slug else {
        return WorkspaceSyncOutcome {
            old_slug: None,
            new_slug: None,
            idx_modified: false,
            synced: false,
        };
    };
    if !idx.workspaces.contains_key(old_slug) {
        return WorkspaceSyncOutcome {
            old_slug: None,
            new_slug: None,
            idx_modified: false,
            synced: false,
        };
    }

    let old_ws = workspace_dir(working_root, old_slug);
    let mut manifest = match load_manifest(&old_ws) {
        Ok(m) => m,
        Err(e) => {
            if let Some(ws_entry) = idx.workspaces.get_mut(old_slug) {
                ws_entry.source_relative_path = plan_item.new_path.clone();
            }
            warnings.push(format!(
                "renamed {} -> {}, but failed to load manifest for workspace {old_slug} ({e}); kept slug {old_slug}",
                plan_item.old_path, plan_item.new_path
            ));
            return WorkspaceSyncOutcome {
                old_slug: Some(old_slug.clone()),
                new_slug: Some(old_slug.clone()),
                idx_modified: true,
                synced: false,
            };
        }
    };

    let target_slug = match resolve_target_slug(
        old_slug,
        &plan_item.new_file_name,
        reserved_slugs,
        retired_slugs,
    ) {
        Ok(slug) => slug,
        Err(e) => {
            warnings.push(format!(
                "renamed {} -> {}, but failed to resolve target slug for workspace {old_slug} ({e}); kept slug {old_slug}",
                plan_item.old_path, plan_item.new_path
            ));
            old_slug.clone()
        }
    };

    if target_slug != *old_slug {
        let new_ws = workspace_dir(working_root, &target_slug);
        match fs::rename(&old_ws, &new_ws) {
            Ok(()) => {
                manifest.id = target_slug.clone();
                manifest.source.relative_path = plan_item.new_path.clone();
                let synced = match save_manifest(&new_ws, &mut manifest) {
                    Ok(()) => true,
                    Err(e) => {
                        warnings.push(format!(
                            "renamed {} -> {} and moved workspace {old_slug} -> {target_slug}, but failed to save manifest ({e})",
                            plan_item.old_path, plan_item.new_path
                        ));
                        false
                    }
                };
                if let Some(mut ws_entry) = idx.workspaces.remove(old_slug) {
                    ws_entry.source_relative_path = plan_item.new_path.clone();
                    idx.workspaces.insert(target_slug.clone(), ws_entry);
                }
                return WorkspaceSyncOutcome {
                    old_slug: Some(old_slug.clone()),
                    new_slug: Some(target_slug),
                    idx_modified: true,
                    synced,
                };
            }
            Err(e) => {
                warnings.push(format!(
                    "renamed {} -> {}, but failed to rename workspace directory {old_slug} -> {target_slug} ({e}); kept slug {old_slug}",
                    plan_item.old_path, plan_item.new_path
                ));
            }
        }
    }

    // Either target_slug == old_slug, slug resolution fell back, or workspace dir rename failed.
    manifest.source.relative_path = plan_item.new_path.clone();
    let synced = match save_manifest(&old_ws, &mut manifest) {
        Ok(()) => true,
        Err(e) => {
            warnings.push(format!(
                "renamed {} -> {}, but failed to save manifest for workspace {old_slug} ({e})",
                plan_item.old_path, plan_item.new_path
            ));
            false
        }
    };
    if let Some(ws_entry) = idx.workspaces.get_mut(old_slug) {
        ws_entry.source_relative_path = plan_item.new_path.clone();
    }
    WorkspaceSyncOutcome {
        old_slug: Some(old_slug.clone()),
        new_slug: Some(old_slug.clone()),
        idx_modified: true,
        synced,
    }
}

/// Apply a previously computed `RenamePlan`, synchronizing adopted `.vsplit/` workspaces
/// under canonical lock ordering (`index.lock` -> `<slug>.lock`).
pub fn apply_rename_plan(
    working_root: &Path,
    plan: RenamePlan,
    actor: LockActor,
) -> Result<RenameReport, RenameError> {
    let RenamePlan {
        format,
        has_index,
        entries,
        mut skipped,
        mut warnings,
    } = plan;

    if entries.is_empty() {
        skipped.sort_by(|a, b| a.path.cmp(&b.path));
        let counts = build_counts(&[], 0, &skipped);
        return Ok(RenameReport {
            dry_run: false,
            format,
            renamed: Vec::new(),
            skipped,
            warnings,
            counts,
        });
    }

    let should_sync_index = has_index || index_path(working_root).is_file();
    let _index_guard = if should_sync_index {
        Some(lock_index(working_root)?)
    } else {
        None
    };
    let mut idx = if should_sync_index {
        load_index(working_root)?
    } else {
        Index::default()
    };

    // Phase 1: Acquire workspace locks, prune occupied targets, and stage colliding sources.
    let run_id = format!("rename-{:08x}", rand::random::<u32>());
    let mut workspace_locks: HashMap<String, WorkspaceLockGuard> = HashMap::new();
    let mut executable: Vec<PlannedRename> = Vec::with_capacity(entries.len());

    for entry in entries {
        if let Some(ref old_slug) = entry.old_slug {
            if idx.workspaces.contains_key(old_slug) && !workspace_locks.contains_key(old_slug) {
                match try_acquire_workspace_lock(
                    working_root,
                    old_slug,
                    LockLease::new(actor, "rename", &run_id),
                ) {
                    Ok(guard) => {
                        workspace_locks.insert(old_slug.clone(), guard);
                    }
                    Err(LockError::Held { lease, .. }) => {
                        skipped.push(RenameSkippedItem {
                            path: entry.old_path,
                            reason: RenameSkipReason::WorkspaceLocked,
                            detail: Some(format_lock_detail(&lease)),
                        });
                        continue;
                    }
                    Err(e) => return Err(RenameError::Lock(e)),
                }
            }
        }
        executable.push(entry);
    }

    executable = prune_occupied_targets(executable, &mut skipped);

    if executable.is_empty() {
        drop(workspace_locks);
        skipped.sort_by(|a, b| a.path.cmp(&b.path));
        let counts = build_counts(&[], 0, &skipped);
        return Ok(RenameReport {
            dry_run: false,
            format,
            renamed: Vec::new(),
            skipped,
            warnings,
            counts,
        });
    }

    let exec_new_lower: HashSet<String> = executable
        .iter()
        .map(|e| e.new_abs.to_string_lossy().to_lowercase())
        .collect();

    struct StagedEntry {
        plan: PlannedRename,
        current_path: PathBuf,
        staged: bool,
    }

    let mut staged_entries: Vec<StagedEntry> = Vec::with_capacity(executable.len());
    for entry in executable {
        let old_lower = entry.old_abs.to_string_lossy().to_lowercase();
        if exec_new_lower.contains(&old_lower) {
            let parent = entry.old_abs.parent().unwrap_or(working_root);
            let orig_name = entry
                .old_abs
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let tmp_path = allocate_staging_path(parent, &orig_name);
            match fs::rename(&entry.old_abs, &tmp_path) {
                Ok(()) => {
                    staged_entries.push(StagedEntry {
                        plan: entry,
                        current_path: tmp_path,
                        staged: true,
                    });
                }
                Err(e) => {
                    skipped.push(RenameSkippedItem {
                        path: entry.old_path,
                        reason: RenameSkipReason::Error,
                        detail: Some(format!("failed to stage source file for rename: {e}")),
                    });
                }
            }
        } else {
            let current_path = entry.old_abs.clone();
            staged_entries.push(StagedEntry {
                plan: entry,
                current_path,
                staged: false,
            });
        }
    }

    // Phase 2: Complete ALL source file renames (no early `?` returns) so no staged temp file
    // can ever be stranded by a downstream workspace sync failure.
    let mut source_renamed: Vec<PlannedRename> = Vec::with_capacity(staged_entries.len());
    for item in staged_entries {
        if item.plan.new_abs.exists() {
            if item.staged {
                restore_staged_non_destructive(
                    &item.current_path,
                    &item.plan.old_abs,
                    &item.plan.old_path,
                    &mut warnings,
                );
            }
            skipped.push(RenameSkippedItem {
                path: item.plan.old_path,
                reason: RenameSkipReason::Error,
                detail: Some(format!(
                    "target path already exists: {}",
                    item.plan.new_path
                )),
            });
            continue;
        }

        if let Err(e) = fs::rename(&item.current_path, &item.plan.new_abs) {
            if item.staged {
                restore_staged_non_destructive(
                    &item.current_path,
                    &item.plan.old_abs,
                    &item.plan.old_path,
                    &mut warnings,
                );
            }
            skipped.push(RenameSkippedItem {
                path: item.plan.old_path,
                reason: RenameSkipReason::Error,
                detail: Some(format!("failed to rename source file: {e}")),
            });
            continue;
        }

        source_renamed.push(item.plan);
    }

    // Phase 3: Synchronize adopted workspaces for all successfully renamed source files.
    // Per-workspace errors emit warnings without aborting healthy siblings.
    let mut reserved_slugs: HashSet<String> = idx.workspaces.keys().cloned().collect();
    let retired_slugs: HashSet<String> = idx.retired_slugs.iter().cloned().collect();
    let mut renamed: Vec<RenameItem> = Vec::with_capacity(source_renamed.len());
    let mut workspaces_synced = 0usize;
    let mut idx_modified = false;

    for plan_item in source_renamed {
        let outcome = sync_renamed_workspace(
            working_root,
            &plan_item,
            &mut idx,
            &mut reserved_slugs,
            &retired_slugs,
            &mut warnings,
        );
        if outcome.idx_modified {
            idx_modified = true;
        }
        if outcome.synced {
            workspaces_synced += 1;
        }
        renamed.push(RenameItem {
            old_path: plan_item.old_path,
            new_path: plan_item.new_path,
            source_tag: plan_item.source_tag,
            old_slug: outcome.old_slug,
            new_slug: outcome.new_slug,
        });
    }

    if idx_modified {
        save_index(working_root, &mut idx)?;
    }

    // Explicitly drop workspace locks before index lock drops at scope exit.
    drop(workspace_locks);

    skipped.sort_by(|a, b| a.path.cmp(&b.path));
    let counts = build_counts(&renamed, workspaces_synced, &skipped);
    Ok(RenameReport {
        dry_run: false,
        format,
        renamed,
        skipped,
        warnings,
        counts,
    })
}

/// Plan and optionally apply creation-date video renaming in one call.
pub async fn rename_videos(
    working_root: &Path,
    config: &Config,
    toolchain: &MediaToolchain,
    opts: &RenameOptions,
    actor: LockActor,
) -> Result<RenameReport, RenameError> {
    let plan = plan_renames(working_root, config, toolchain, opts).await?;
    if opts.dry_run {
        return Ok(plan.into_dry_run_report());
    }
    apply_rename_plan(working_root, plan, actor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validate_format_accepts_valid_and_rejects_invalid() {
        assert!(validate_format(DEFAULT_RENAME_FORMAT).is_ok());
        assert!(validate_format("%Y%m%d_%H%M%S").is_ok());
        assert!(validate_format("clip_%Y-%m-%d_%H-%M-%S").is_ok());

        // Empty / whitespace
        assert!(validate_format("").is_err());
        assert!(validate_format("   ").is_err());
        // Forbidden raw characters
        assert!(validate_format("%Y/%m/%d").is_err());
        assert!(validate_format("%Y\\%m\\%d").is_err());
        assert!(validate_format("%Y-%m-%d %H:%M:%S").is_err());
        assert!(validate_format("%Y*%m").is_err());
        assert!(validate_format("%Y?%m").is_err());
        assert!(validate_format("%Y\"%m").is_err());
        assert!(validate_format("%Y<%m>").is_err());
        assert!(validate_format("%Y|%m").is_err());
        assert!(validate_format("%Y\n%m").is_err());
        // Specifiers that render forbidden chars (e.g., %T -> 14:23:05, %D -> 05/01/26)
        assert!(validate_format("%Y-%m-%d %T").is_err());
        assert!(validate_format("%D").is_err());
        // Trailing dot or space (invalid on Windows)
        assert!(validate_format("%Y-%m-%d.").is_err());
        assert!(validate_format("%Y-%m-%d ").is_err());
        // Invalid chrono specifier
        assert!(validate_format("%Q").is_err());
        assert!(validate_format("%Y-%").is_err());
    }

    #[test]
    fn extract_creation_timestamp_prefers_quicktime_and_preserves_offset() {
        // QuickTime tag without colon in offset (+0800) overrides creation_time UTC
        let probe = json!({
            "format": {
                "tags": {
                    "creation_time": "2026-05-01T06:23:05.000000Z",
                    "com.apple.quicktime.creationdate": "2026-05-01T14:23:05+0800"
                }
            }
        });
        let (stem, tag) = extract_creation_timestamp(&probe, DEFAULT_RENAME_FORMAT).unwrap();
        assert_eq!(stem, "2026-05-01 14-23-05");
        assert_eq!(tag, "com.apple.quicktime.creationdate");

        // RFC3339 with colon offset (+08:00), UTC Z, and negative offset (-05:00) preserve wall-clock time
        let probe_colon = json!({
            "format": {
                "tags": {
                    "com.apple.quicktime.creationdate": "2026-05-01T14:23:05+08:00"
                }
            }
        });
        let (stem, tag) = extract_creation_timestamp(&probe_colon, DEFAULT_RENAME_FORMAT).unwrap();
        assert_eq!(stem, "2026-05-01 14-23-05");
        assert_eq!(tag, "com.apple.quicktime.creationdate");

        let probe_utc = json!({
            "format": {
                "tags": {
                    "com.apple.quicktime.creationdate": "2026-05-01T06:23:05Z"
                }
            }
        });
        let (stem, tag) = extract_creation_timestamp(&probe_utc, DEFAULT_RENAME_FORMAT).unwrap();
        assert_eq!(stem, "2026-05-01 06-23-05");
        assert_eq!(tag, "com.apple.quicktime.creationdate");

        let probe_rfc = json!({
            "format": {
                "tags": {
                    "Com.Apple.QuickTime.CreationDate": "2026-05-01T09:15:42-05:00"
                }
            }
        });
        let (stem, tag) = extract_creation_timestamp(&probe_rfc, DEFAULT_RENAME_FORMAT).unwrap();
        assert_eq!(stem, "2026-05-01 09-15-42");
        assert_eq!(tag, "com.apple.quicktime.creationdate");
    }

    #[test]
    fn resolve_target_slug_keeps_existing_collision_suffix_when_base_taken() {
        let mut reserved: HashSet<String> = ["foo".to_string(), "foo-2".to_string()].into();
        let retired: HashSet<String> = HashSet::new();

        // foo-2 renaming to a filename whose base slug is "foo" stays "foo-2" (not "foo-3")
        let resolved = resolve_target_slug("foo-2", "foo.mp4", &mut reserved, &retired).unwrap();
        assert_eq!(resolved, "foo-2");
        assert!(reserved.contains("foo"));
        assert!(reserved.contains("foo-2"));
    }

    #[test]
    fn extract_creation_timestamp_falls_back_to_format_and_video_stream_creation_time() {
        let utc_str = "2026-05-01T06:23:05.000000Z";
        let expected_local = DateTime::parse_from_rfc3339(utc_str)
            .unwrap()
            .with_timezone(&Local)
            .format(DEFAULT_RENAME_FORMAT)
            .to_string();

        // Format creation_time (case-insensitive key)
        let probe_fmt = json!({
            "format": {
                "tags": {
                    "CREATION_TIME": utc_str
                }
            }
        });
        let (stem, tag) = extract_creation_timestamp(&probe_fmt, DEFAULT_RENAME_FORMAT).unwrap();
        assert_eq!(stem, expected_local);
        assert_eq!(tag, "creation_time");

        // Video stream creation_time fallback (ignores audio stream and cover-art video stream without tag)
        let probe_stream = json!({
            "format": { "tags": {} },
            "streams": [
                {
                    "codec_type": "audio",
                    "tags": { "creation_time": "2020-01-01T00:00:00Z" }
                },
                {
                    "codec_type": "video",
                    "tags": {}
                },
                {
                    "codec_type": "video",
                    "tags": { "creation_time": utc_str }
                }
            ]
        });
        let (stem, tag) = extract_creation_timestamp(&probe_stream, DEFAULT_RENAME_FORMAT).unwrap();
        assert_eq!(stem, expected_local);
        assert_eq!(tag, "creation_time");

        // Naive creation_time treated as local time
        let probe_naive = json!({
            "format": {
                "tags": {
                    "creation_time": "2026-05-01 18:05:09"
                }
            }
        });
        let (stem, tag) = extract_creation_timestamp(&probe_naive, DEFAULT_RENAME_FORMAT).unwrap();
        assert_eq!(stem, "2026-05-01 18-05-09");
        assert_eq!(tag, "creation_time");

        // Missing or unparseable tags return None
        assert!(
            extract_creation_timestamp(&json!({"format": {}}), DEFAULT_RENAME_FORMAT).is_none()
        );
        assert!(extract_creation_timestamp(
            &json!({"format": {"tags": {"creation_time": "not-a-date"}}}),
            DEFAULT_RENAME_FORMAT
        )
        .is_none());
    }

    #[test]
    fn is_already_named_handles_exact_and_numbered_suffixes() {
        let want = "2026-05-01 14-23-05";
        assert!(is_already_named("2026-05-01 14-23-05", want));
        assert!(is_already_named("2026-05-01 14-23-05(1)", want));
        assert!(is_already_named("2026-05-01 14-23-05(12)", want));
        assert!(is_already_named("CLIP_2026-05-01", "clip_2026-05-01"));

        assert!(!is_already_named("2026-05-01 14-23-05(0)", want));
        assert!(!is_already_named("2026-05-01 14-23-05(01)", want));
        assert!(!is_already_named("2026-05-01 14-23-05(-1)", want));
        assert!(!is_already_named("2026-05-01 14-23-05-extra", want));
        assert!(!is_already_named("IMG_1234", want));
    }

    #[test]
    fn allocate_target_filename_handles_duplicates_and_case_insensitive_collisions() {
        let mut occupied = HashSet::new();
        // Simulate pre-existing lowercase file on disk
        occupied.insert("2026-05-01 14-23-05.mov".to_string());

        // Preserves original .MOV extension, avoids case-insensitive collision
        let first = allocate_target_filename("2026-05-01 14-23-05", ".MOV", &mut occupied);
        assert_eq!(first, "2026-05-01 14-23-05(1).MOV");

        let second = allocate_target_filename("2026-05-01 14-23-05", ".MOV", &mut occupied);
        assert_eq!(second, "2026-05-01 14-23-05(2).MOV");

        // Different extension does not collide
        let mp4 = allocate_target_filename("2026-05-01 14-23-05", ".mp4", &mut occupied);
        assert_eq!(mp4, "2026-05-01 14-23-05.mp4");
        let mp4_dup = allocate_target_filename("2026-05-01 14-23-05", ".mp4", &mut occupied);
        assert_eq!(mp4_dup, "2026-05-01 14-23-05(1).mp4");
    }
}
