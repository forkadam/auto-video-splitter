use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenesDetectorConfig {
    pub threshold: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneEntry {
    pub id: usize,
    pub start_time: f64,
    pub end_time: f64,
    pub start_frame: usize,
    pub end_frame: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenesJson {
    pub detector: String,
    pub config: ScenesDetectorConfig,
    pub scenes: Vec<SceneEntry>,
}
