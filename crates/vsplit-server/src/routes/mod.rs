pub mod actions;
pub mod config;
pub mod doctor;
pub mod logs;
pub mod media;
pub mod project;
pub mod workspaces;

use std::path::PathBuf;

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use vsplit_core::actions::ActionError;
use vsplit_core::locks::LockError;

use crate::auth::verify_api_request;
use crate::session::AppState;

pub struct ApiError {
    pub status: StatusCode,
    pub body: Value,
}

impl ApiError {
    pub fn new(status: StatusCode, detail: impl Into<String>) -> Self {
        Self {
            status,
            body: json!({ "detail": detail.into() }),
        }
    }

    pub fn bad_request(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, detail)
    }

    pub fn not_found(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, detail)
    }

    pub fn conflict(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, detail)
    }

    pub fn unprocessable(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, detail)
    }

    pub fn internal(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, detail)
    }

    pub fn from_action(err: ActionError) -> Self {
        match err {
            ActionError::Lock(LockError::Held { slug, lease }) => Self {
                status: StatusCode::CONFLICT,
                body: json!({
                    "detail": format!(
                        "workspace {slug:?} is locked by {} (pid {}, stage {})",
                        lease.actor.as_str(), lease.pid, lease.stage
                    ),
                    "locked_by": lease,
                }),
            },
            ActionError::HashMismatch { actual, .. } => Self {
                status: StatusCode::CONFLICT,
                body: json!({
                    "detail": "cuts.json changed on disk",
                    "current_hash": actual,
                }),
            },
            ActionError::Path(e) => Self::bad_request(e.to_string()),
            ActionError::Schema(e) => Self::unprocessable(e.to_string()),
            ActionError::UnknownStage(s) => Self::unprocessable(format!("unknown stage: {s:?}")),
            ActionError::Validation(msg) => Self::unprocessable(msg),
            ActionError::CutsNotFound(_) => Self::not_found("cuts.json not yet written"),
            ActionError::LogNotFound { .. } => Self::not_found("log not found"),
            other => Self::internal(other.to_string()),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

pub fn require_working_root(state: &AppState) -> Result<PathBuf, ApiError> {
    state
        .working_root()
        .ok_or_else(|| ApiError::bad_request("no project folder is currently open"))
}

async fn api_auth_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    verify_api_request(req.headers(), &state.token)?;
    Ok(next.run(req).await)
}

async fn health() -> Json<Value> {
    Json(json!({ "ok": true }))
}

pub fn router(state: AppState) -> Router {
    let api_routes = Router::new()
        .route("/health", get(health))
        .route("/doctor", get(doctor::get_doctor))
        .route("/project/open", post(project::open_project))
        .route("/project/recent", get(project::recent_projects))
        .route("/project/adopt-file", post(project::adopt_file))
        .route("/scan", post(project::scan))
        .route("/jobs/cancel", post(project::cancel_jobs))
        .route("/jobs/active", get(project::active_jobs))
        .route("/workspaces", get(workspaces::list_workspaces))
        .route("/workspaces/{slug}", get(workspaces::get_workspace))
        .route(
            "/workspaces/{slug}/cuts",
            get(workspaces::get_cuts).put(workspaces::put_cuts),
        )
        .route(
            "/workspaces/{slug}/open-preview",
            post(workspaces::open_preview),
        )
        .route("/workspaces/{slug}/logs/{stage}", get(logs::get_log))
        .route("/workspaces/{slug}/run", post(actions::run_one))
        .route("/workspaces/{slug}/reset", post(actions::reset_one))
        .route("/workspaces/{slug}/clean", post(actions::clean_one))
        .route("/workspaces/{slug}/approve", post(actions::approve_one))
        .route("/workspaces/{slug}/ignore", post(actions::ignore_one))
        .route("/workspaces/{slug}/unignore", post(actions::unignore_one))
        .route("/batch/run", post(actions::batch_run))
        .route("/batch/reset", post(actions::batch_reset))
        .route("/batch/clean", post(actions::batch_clean))
        .route("/batch/approve", post(actions::batch_approve))
        .route("/batch/collect", post(actions::batch_collect))
        .route("/config", get(config::get_config).put(config::put_config))
        .route(
            "/config/raw",
            get(config::get_config_raw).put(config::put_config_raw),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            api_auth_middleware,
        ));

    let media_routes = Router::new()
        .route("/m/{token}/events", get(media::events))
        .route(
            "/m/{token}/workspaces/{slug}/preview/{*path}",
            get(media::serve_preview),
        )
        .route(
            "/m/{token}/workspaces/{slug}/thumbs/{name}",
            get(media::serve_thumb),
        );

    Router::new()
        .nest("/api", api_routes)
        .merge(media_routes)
        .with_state(state)
}
