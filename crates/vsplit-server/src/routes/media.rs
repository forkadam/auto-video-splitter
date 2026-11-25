use std::convert::Infallible;

use axum::extract::{Path as AxumPath, Request, State};
use axum::http::StatusCode;
use axum::response::sse::Event;
use axum::response::{IntoResponse, Response, Sse};
use futures_util::stream::Stream;
use tower::ServiceExt;
use tower_http::services::ServeFile;
use vsplit_core::paths::{validate_slug, workspace_dir};

use crate::auth::{redact_token_in_path, verify_path_token};
use crate::routes::workspaces::require_indexed_slug;
use crate::session::AppState;

pub async fn events(
    State(state): State<AppState>,
    AxumPath(token): AxumPath<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    verify_path_token(&token, &state.token)?;
    Ok(state.broker.stream())
}

pub async fn serve_preview(
    State(state): State<AppState>,
    AxumPath((token, slug, rel_path)): AxumPath<(String, String, String)>,
    req: Request,
) -> Response {
    if verify_path_token(&token, &state.token).is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Some(root) = state.working_root() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if require_indexed_slug(&root, &slug).is_err() {
        return StatusCode::NOT_FOUND.into_response();
    }
    if rel_path.contains("..") || rel_path.starts_with('/') || rel_path.starts_with('\\') {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let base = workspace_dir(&root, &slug).join("preview");
    let Ok(canon_base) = base.canonicalize() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let target = base.join(&rel_path);
    let Ok(canon_target) = target.canonicalize() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !canon_target.starts_with(&canon_base) || !canon_target.is_file() {
        return StatusCode::NOT_FOUND.into_response();
    }

    match ServeFile::new(canon_target).oneshot(req).await {
        Ok(res) => res.into_response(),
        Err(err) => {
            tracing::debug!(
                path = %redact_token_in_path(&rel_path, &state.token),
                error = %err,
                "preview range response interrupted"
            );
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub async fn serve_thumb(
    State(state): State<AppState>,
    AxumPath((token, slug, name)): AxumPath<(String, String, String)>,
    req: Request,
) -> Response {
    if verify_path_token(&token, &state.token).is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if validate_slug(&slug).is_err() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Some(root) = state.working_root() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if require_indexed_slug(&root, &slug).is_err() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let img = workspace_dir(&root, &slug)
        .join("preview")
        .join("images")
        .join(&name);
    if !img.is_file() {
        return StatusCode::NOT_FOUND.into_response();
    }
    match ServeFile::new(img).oneshot(req).await {
        Ok(res) => res.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
