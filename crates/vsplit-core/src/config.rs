use std::fs;
use std::path::{Path, PathBuf};

use serde_yml::{Mapping, Value};
use thiserror::Error;

use crate::atomic::atomic_write_text;
use crate::paths::{
    app_config_path, legacy_windows_config_path, user_global_config_path, working_state_dir,
};
use crate::schemas::app_config::AppConfig;
use crate::schemas::config::Config;
use crate::schemas::SchemaError;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("--config path not found: {0}")]
    ExplicitNotFound(PathBuf),
    #[error("failed to read config at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid YAML in {path}: {source}")]
    Yaml {
        path: PathBuf,
        #[source]
        source: serde_yml::Error,
    },
    #[error("config at {0} must be a YAML mapping")]
    NotMapping(PathBuf),
    #[error("invalid TOML in {path}: {source}")]
    Toml {
        path: PathBuf,
        #[source]
        source: Box<toml::de::Error>,
    },
    #[error("failed to serialize TOML: {0}")]
    TomlSer(#[from] toml::ser::Error),
    #[error("config schema validation failed: {0}")]
    Schema(#[from] SchemaError),
}

fn read_yaml_mapping(path: &Path) -> Result<Value, ConfigError> {
    let raw = fs::read_to_string(path).map_err(|e| ConfigError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    if raw.trim().is_empty() {
        return Ok(Value::Mapping(Mapping::new()));
    }
    let val: Value = serde_yml::from_str(&raw).map_err(|e| ConfigError::Yaml {
        path: path.to_path_buf(),
        source: e,
    })?;
    match val {
        Value::Null => Ok(Value::Mapping(Mapping::new())),
        Value::Mapping(_) => Ok(val),
        _ => Err(ConfigError::NotMapping(path.to_path_buf())),
    }
}

fn deep_merge(base: Value, overlay: Value) -> Value {
    match (base, overlay) {
        (Value::Mapping(mut b), Value::Mapping(o)) => {
            for (k, v) in o {
                if let Some(existing) = b.remove(&k) {
                    b.insert(k, deep_merge(existing, v));
                } else {
                    b.insert(k, v);
                }
            }
            Value::Mapping(b)
        }
        (_, over) => over,
    }
}

/// Resolve effective user-global `config.yaml` path for reading (includes read-only Windows
/// legacy `platformdirs` fallback when `%APPDATA%\vsplit\config.yaml` is absent).
pub fn resolve_user_global_config_read_path() -> Option<PathBuf> {
    let primary = user_global_config_path();
    if primary.exists() {
        return Some(primary);
    }
    if let Some(legacy) = legacy_windows_config_path() {
        if legacy.exists() {
            return Some(legacy);
        }
    }
    None
}

/// 4-layer config resolution: defaults -> user global -> `.vsplit/config.yaml` -> explicit `--config`.
pub fn load_resolved_config(
    working_root: &Path,
    explicit_path: Option<&Path>,
) -> Result<Config, ConfigError> {
    let mut merged = Value::Mapping(Mapping::new());

    if let Some(global_path) = resolve_user_global_config_read_path() {
        merged = deep_merge(merged, read_yaml_mapping(&global_path)?);
    }

    let project_path = working_state_dir(working_root).join("config.yaml");
    if project_path.exists() {
        merged = deep_merge(merged, read_yaml_mapping(&project_path)?);
    }

    if let Some(exp) = explicit_path {
        if !exp.exists() {
            return Err(ConfigError::ExplicitNotFound(exp.to_path_buf()));
        }
        merged = deep_merge(merged, read_yaml_mapping(exp)?);
    }

    let cfg: Config = serde_yml::from_value(merged).map_err(|e| ConfigError::Yaml {
        path: explicit_path.map(Path::to_path_buf).unwrap_or(project_path),
        source: e,
    })?;
    cfg.validate()?;
    Ok(cfg)
}

pub fn project_config_path(working_root: &Path) -> PathBuf {
    working_state_dir(working_root).join("config.yaml")
}

pub fn read_project_config_raw(working_root: &Path) -> Result<String, ConfigError> {
    let path = project_config_path(working_root);
    if !path.exists() {
        return Ok(String::new());
    }
    fs::read_to_string(&path).map_err(|e| ConfigError::Io { path, source: e })
}

/// Validate raw YAML (merged against defaults + user global) and atomically persist `.vsplit/config.yaml`.
pub fn write_project_config_raw(
    working_root: &Path,
    raw_yaml: &str,
) -> Result<Config, ConfigError> {
    let path = project_config_path(working_root);
    let overlay: Value = if raw_yaml.trim().is_empty() {
        Value::Mapping(Mapping::new())
    } else {
        let parsed: Value = serde_yml::from_str(raw_yaml).map_err(|e| ConfigError::Yaml {
            path: path.clone(),
            source: e,
        })?;
        match parsed {
            Value::Null => Value::Mapping(Mapping::new()),
            Value::Mapping(_) => parsed,
            _ => return Err(ConfigError::NotMapping(path)),
        }
    };

    let mut merged = Value::Mapping(Mapping::new());
    if let Some(global_path) = resolve_user_global_config_read_path() {
        merged = deep_merge(merged, read_yaml_mapping(&global_path)?);
    }
    merged = deep_merge(merged, overlay);

    let cfg: Config = serde_yml::from_value(merged).map_err(|e| ConfigError::Yaml {
        path: path.clone(),
        source: e,
    })?;
    cfg.validate()?;

    let mut to_write = raw_yaml.to_string();
    if !to_write.is_empty() && !to_write.ends_with('\n') {
        to_write.push('\n');
    }
    atomic_write_text(&path, &to_write).map_err(|e| ConfigError::Io { path, source: e })?;
    Ok(cfg)
}

pub fn load_app_config() -> Result<AppConfig, ConfigError> {
    let path = app_config_path();
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    let raw = fs::read_to_string(&path).map_err(|e| ConfigError::Io {
        path: path.clone(),
        source: e,
    })?;
    if raw.trim().is_empty() {
        return Ok(AppConfig::default());
    }
    toml::from_str(&raw).map_err(|e| ConfigError::Toml {
        path,
        source: Box::new(e),
    })
}

pub fn save_app_config(cfg: &AppConfig) -> Result<(), ConfigError> {
    let path = app_config_path();
    let mut rendered = toml::to_string_pretty(cfg)?;
    if !rendered.ends_with('\n') {
        rendered.push('\n');
    }
    atomic_write_text(&path, &rendered).map_err(|e| ConfigError::Io { path, source: e })
}
