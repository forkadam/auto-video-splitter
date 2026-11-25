use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::SchemaError;

/// Exact match for `^\d{2}:\d{2}:\d{2}\.\d{3}$`.
pub fn is_valid_timestamp(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 12 {
        return false;
    }
    b[0].is_ascii_digit()
        && b[1].is_ascii_digit()
        && b[2] == b':'
        && b[3].is_ascii_digit()
        && b[4].is_ascii_digit()
        && b[5] == b':'
        && b[6].is_ascii_digit()
        && b[7].is_ascii_digit()
        && b[8] == b'.'
        && b[9].is_ascii_digit()
        && b[10].is_ascii_digit()
        && b[11].is_ascii_digit()
}

pub fn format_tc(seconds: f64) -> String {
    let clamped = seconds.max(0.0);
    let total_ms = (clamped * 1000.0).round() as u64;
    let ms = total_ms % 1000;
    let total_secs = total_ms / 1000;
    let secs = total_secs % 60;
    let mins = (total_secs / 60) % 60;
    let hours = total_secs / 3600;
    format!("{hours:02}:{mins:02}:{secs:02}.{ms:03}")
}

pub fn parse_tc(tc: &str) -> Result<f64, SchemaError> {
    if !is_valid_timestamp(tc) {
        return Err(SchemaError::Validation(format!(
            "expected HH:MM:SS.fff, got {tc:?}"
        )));
    }
    let h: f64 = tc[0..2].parse().unwrap();
    let m: f64 = tc[3..5].parse().unwrap();
    let s: f64 = tc[6..12].parse().unwrap();
    Ok(h * 3600.0 + m * 60.0 + s)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    pub id: usize,
    pub start: String,
    pub end: String,
    pub keep: bool,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub merge_with_next: bool,
}

fn default_schema_version() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CutsJson {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub scenes: Vec<Scene>,
}

impl Default for CutsJson {
    fn default() -> Self {
        Self {
            schema_version: 1,
            scenes: Vec::new(),
        }
    }
}

impl CutsJson {
    pub fn validate(&self) -> Result<(), SchemaError> {
        if self.schema_version != 1 {
            return Err(SchemaError::UnsupportedVersion {
                file: "cuts.json",
                found: self.schema_version,
            });
        }
        let mut counts: BTreeMap<usize, usize> = BTreeMap::new();
        for scene in &self.scenes {
            if !is_valid_timestamp(&scene.start) {
                return Err(SchemaError::Validation(format!(
                    "expected HH:MM:SS.fff, got {:?}",
                    scene.start
                )));
            }
            if !is_valid_timestamp(&scene.end) {
                return Err(SchemaError::Validation(format!(
                    "expected HH:MM:SS.fff, got {:?}",
                    scene.end
                )));
            }
            *counts.entry(scene.id).or_insert(0) += 1;
        }
        let dups: Vec<usize> = counts
            .into_iter()
            .filter_map(|(id, c)| (c > 1).then_some(id))
            .collect();
        if !dups.is_empty() {
            return Err(SchemaError::Validation(format!(
                "duplicate scene ids: {dups:?}"
            )));
        }
        Ok(())
    }
}
