pub mod app_config;
pub mod config;
pub mod cuts;
pub mod index;
pub mod manifest;
pub mod plan;
pub mod scenes;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SchemaError {
    #[error("unsupported schema_version {found} in {file} (expected 1)")]
    UnsupportedVersion { file: &'static str, found: u32 },
    #[error("{0}")]
    Validation(String),
}
