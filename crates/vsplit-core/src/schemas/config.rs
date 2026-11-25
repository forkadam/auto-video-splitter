use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::detect::DetectorKind;

use super::SchemaError;

pub const VALID_CONCURRENCY_STAGES: [&str; 6] =
    ["probe", "detect", "preview", "plan", "split", "verify"];

fn default_detector() -> DetectorKind {
    DetectorKind::Content
}
fn default_threshold() -> f64 {
    27.0
}
fn default_min_scene_length() -> f64 {
    0.6
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DetectConfig {
    #[serde(default = "default_detector")]
    pub detector: DetectorKind,
    #[serde(default = "default_threshold")]
    pub threshold: f64,
    #[serde(default = "default_min_scene_length")]
    pub min_scene_length: f64,
    #[serde(default)]
    pub luma_only: bool,
}

impl Default for DetectConfig {
    fn default() -> Self {
        Self {
            detector: default_detector(),
            threshold: default_threshold(),
            min_scene_length: default_min_scene_length(),
            luma_only: false,
        }
    }
}

fn default_images_per_scene() -> usize {
    3
}
fn default_image_width() -> u32 {
    480
}
fn default_image_quality() -> u32 {
    85
}
fn default_proxy_height() -> Option<u32> {
    Some(720)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewConfig {
    #[serde(default = "default_images_per_scene")]
    pub images_per_scene: usize,
    #[serde(default = "default_image_width")]
    pub image_width: u32,
    #[serde(default = "default_image_quality")]
    pub image_quality: u32,
    #[serde(default = "default_proxy_height")]
    pub proxy_height: Option<u32>,
}

impl Default for PreviewConfig {
    fn default() -> Self {
        Self {
            images_per_scene: default_images_per_scene(),
            image_width: default_image_width(),
            image_quality: default_image_quality(),
            proxy_height: default_proxy_height(),
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewConfig {
    #[serde(default = "default_true")]
    pub default_keep: bool,
    #[serde(default = "default_true")]
    pub auto_apply_template: bool,
}

impl Default for ReviewConfig {
    fn default() -> Self {
        Self {
            default_keep: true,
            auto_apply_template: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum KeyframeSnap {
    #[default]
    NearestPreceding,
}

fn default_min_segment_length() -> f64 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanConfig {
    #[serde(default)]
    pub keyframe_snap: KeyframeSnap,
    #[serde(default = "default_min_segment_length")]
    pub min_segment_length: f64,
}

impl Default for PlanConfig {
    fn default() -> Self {
        Self {
            keyframe_snap: KeyframeSnap::NearestPreceding,
            min_segment_length: default_min_segment_length(),
        }
    }
}

fn default_output_container() -> String {
    "mp4".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SplitConfig {
    #[serde(default = "default_output_container")]
    pub output_container: String,
}

impl Default for SplitConfig {
    fn default() -> Self {
        Self {
            output_container: default_output_container(),
        }
    }
}

fn default_duration_tolerance() -> f64 {
    0.5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyConfig {
    #[serde(default = "default_duration_tolerance")]
    pub duration_tolerance_seconds: f64,
}

impl Default for VerifyConfig {
    fn default() -> Self {
        Self {
            duration_tolerance_seconds: default_duration_tolerance(),
        }
    }
}

fn default_extensions() -> Vec<String> {
    vec![
        "mp4".into(),
        "mkv".into(),
        "mov".into(),
        "avi".into(),
        "m4v".into(),
        "webm".into(),
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScanConfig {
    #[serde(default = "default_extensions")]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub recursive: bool,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            extensions: default_extensions(),
            recursive: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CollectMode {
    #[default]
    Copy,
    Move,
    Link,
}

impl CollectMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Copy => "copy",
            Self::Move => "move",
            Self::Link => "link",
        }
    }
}

fn default_collect_dest() -> String {
    "collected".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectConfig {
    #[serde(default = "default_collect_dest")]
    pub destination: String,
    #[serde(default)]
    pub mode: CollectMode,
}

impl Default for CollectConfig {
    fn default() -> Self {
        Self {
            destination: default_collect_dest(),
            mode: CollectMode::Copy,
        }
    }
}

pub fn default_concurrency() -> BTreeMap<String, usize> {
    BTreeMap::from([
        ("probe".to_string(), 4),
        ("detect".to_string(), 2),
        ("preview".to_string(), 4),
        ("plan".to_string(), 8),
        ("split".to_string(), 1),
        ("verify".to_string(), 4),
    ])
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub detect: DetectConfig,
    #[serde(default)]
    pub preview: PreviewConfig,
    #[serde(default)]
    pub review: ReviewConfig,
    #[serde(default)]
    pub plan: PlanConfig,
    #[serde(default)]
    pub split: SplitConfig,
    #[serde(default)]
    pub verify: VerifyConfig,
    #[serde(default = "default_concurrency")]
    pub concurrency: BTreeMap<String, usize>,
    #[serde(default)]
    pub scan: ScanConfig,
    #[serde(default)]
    pub collect: CollectConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            detect: DetectConfig::default(),
            preview: PreviewConfig::default(),
            review: ReviewConfig::default(),
            plan: PlanConfig::default(),
            split: SplitConfig::default(),
            verify: VerifyConfig::default(),
            concurrency: default_concurrency(),
            scan: ScanConfig::default(),
            collect: CollectConfig::default(),
        }
    }
}

impl Config {
    pub fn validate(&self) -> Result<(), SchemaError> {
        let mut unknown: Vec<&str> = self
            .concurrency
            .keys()
            .map(String::as_str)
            .filter(|k| !VALID_CONCURRENCY_STAGES.contains(k))
            .collect();
        if !unknown.is_empty() {
            unknown.sort_unstable();
            return Err(SchemaError::Validation(format!(
                "unknown stage(s) in concurrency: {unknown:?}"
            )));
        }
        if self.concurrency.values().any(|&n| n < 1) {
            return Err(SchemaError::Validation(
                "concurrency values must be >= 1".to_string(),
            ));
        }
        Ok(())
    }

    /// Exact v1 `manifest.config_snapshot` shape (`detect`, `split`, `review.default_keep`).
    pub fn snapshot(&self) -> BTreeMap<String, Value> {
        let mut map = BTreeMap::new();
        map.insert(
            "detect".to_string(),
            serde_json::to_value(&self.detect).unwrap_or(Value::Null),
        );
        map.insert(
            "split".to_string(),
            serde_json::to_value(&self.split).unwrap_or(Value::Null),
        );
        map.insert(
            "review".to_string(),
            serde_json::json!({ "default_keep": self.review.default_keep }),
        );
        map
    }
}
