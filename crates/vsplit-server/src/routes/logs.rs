use axum::extract::{Path as AxumPath, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use vsplit_core::actions::logs_read;

use crate::routes::workspaces::require_indexed_slug;
use crate::routes::{require_working_root, ApiError};
use crate::session::AppState;

pub async fn get_log(
    State(state): State<AppState>,
    AxumPath((slug, stage)): AxumPath<(String, String)>,
) -> Result<Response, ApiError> {
    let root = require_working_root(&state)?;
    require_indexed_slug(&root, &slug)?;
    let text = logs_read(&root, &slug, &stage, None).map_err(ApiError::from_action)?;
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        text,
    )
        .into_response())
}
