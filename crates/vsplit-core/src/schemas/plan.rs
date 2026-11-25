use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedScene {
    pub id: usize,
    pub label: Option<String>,
    pub requested_start: f64,
    pub requested_end: f64,
    pub actual_start: f64,
    pub actual_end: f64,
    pub drift_seconds: f64,
    pub strategy: String,
    pub output_filename: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CutsResolvedJson {
    pub scenes: Vec<ResolvedScene>,
}
