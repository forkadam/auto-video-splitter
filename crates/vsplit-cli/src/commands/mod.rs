pub mod approve;
pub mod clean;
pub mod collect;
pub mod cuts;
pub mod doctor;
pub mod ignore;
pub mod logs;
pub mod one;
pub mod reset;
pub mod run;
pub mod scan;
pub mod status;
pub mod unignore;

use std::path::PathBuf;

use vsplit_core::filter::{apply_filters, compile_slug_glob, parse_filter, FilterPredicate};
use vsplit_core::index::load_index;

use crate::output::CliError;

#[derive(Debug, Clone)]
pub struct GlobalOpts {
    pub working_dir: PathBuf,
    pub config_path: Option<PathBuf>,
    pub json: bool,
    pub non_interactive: bool,
}

pub fn resolve_filtered_slugs(
    opts: &GlobalOpts,
    slug_glob: Option<&str>,
    filters: &[String],
) -> Result<Vec<String>, CliError> {
    let idx = load_index(&opts.working_dir)
        .map_err(|e| CliError::runtime("INDEX_ERROR", e.to_string()))?;
    let slugs: Vec<String> = idx.workspaces.keys().cloned().collect();
    let mut preds: Vec<FilterPredicate> = Vec::new();
    if let Some(glob) = slug_glob {
        preds.push(
            compile_slug_glob(glob)
                .map_err(|e| CliError::usage("INVALID_FILTER", e.to_string()))?,
        );
    }
    for raw in filters {
        preds
            .push(parse_filter(raw).map_err(|e| CliError::usage("INVALID_FILTER", e.to_string()))?);
    }
    Ok(apply_filters(&opts.working_dir, &slugs, &preds))
}
