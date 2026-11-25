use std::collections::HashSet;
use std::path::Path;

use thiserror::Error;

const MAX_LEN: usize = 64;

const RESERVED_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

#[derive(Debug, Error)]
pub enum SlugError {
    #[error("more than 99 slug collisions for {0:?}; rename source files")]
    TooManyCollisions(String),
}

fn is_control(c: char) -> bool {
    let cp = c as u32;
    (cp <= 0x08) || (0x0E..=0x1F).contains(&cp) || cp == 0x7F
}

fn is_reserved_char(c: char) -> bool {
    matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
}

fn trim_edges(s: &str) -> &str {
    s.trim_matches(['.', ' ', '-'])
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

/// Port of `src/vsplit/core/slugify.py::slugify`.
pub fn slugify(filename: &str) -> String {
    let stem_raw = Path::new(filename)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| filename.to_string());
    let lower = stem_raw.to_lowercase();

    let mut collapsed = String::with_capacity(lower.len());
    let mut prev_hyphen = false;
    for ch in lower.chars() {
        if is_control(ch) || is_reserved_char(ch) {
            continue;
        }
        if ch.is_whitespace() || ch == '_' || ch == '-' {
            if !prev_hyphen {
                collapsed.push('-');
                prev_hyphen = true;
            }
        } else {
            collapsed.push(ch);
            prev_hyphen = false;
        }
    }

    let mut stem = trim_edges(&collapsed).to_string();
    if RESERVED_NAMES.contains(&stem.as_str()) {
        stem = format!("_{stem}");
    }
    stem = trim_edges(&truncate_chars(&stem, MAX_LEN)).to_string();
    if stem.is_empty() {
        "_unnamed".to_string()
    } else {
        stem
    }
}

/// Port of `src/vsplit/core/slugify.py::resolve_collision` (case-insensitive, `-2`..`-99`).
pub fn resolve_collision(
    slug: &str,
    live: &HashSet<String>,
    retired: &HashSet<String>,
) -> Result<String, SlugError> {
    let taken_lower: HashSet<String> = live
        .iter()
        .chain(retired.iter())
        .map(|s| s.to_lowercase())
        .collect();
    if !taken_lower.contains(&slug.to_lowercase()) {
        return Ok(slug.to_string());
    }
    let base = truncate_chars(slug, MAX_LEN - 3);
    let base = base.trim_end_matches(['.', ' ', '-']);
    for n in 2..100 {
        let candidate = format!("{base}-{n}");
        if !taken_lower.contains(&candidate.to_lowercase()) {
            return Ok(candidate);
        }
    }
    Err(SlugError::TooManyCollisions(slug.to_string()))
}
