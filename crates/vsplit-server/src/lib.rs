pub mod auth;
pub mod routes;
pub mod session;
pub mod sse;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{header, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use serde::Serialize;
use tokio::net::TcpListener;
use tower::ServiceExt;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeFile;

use crate::auth::{generate_session_token, verify_path_token};
use crate::session::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct ServerBootstrap {
    pub base_url: String,
    pub token: String,
}

pub struct ServerHandle {
    pub bootstrap: ServerBootstrap,
    pub state: AppState,
}

pub fn build_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::OPTIONS])
        .allow_headers([
            header::CONTENT_TYPE,
            header::RANGE,
            header::HeaderName::from_static("x-vsplit-token"),
        ]);

    routes::router(state).layer(cors)
}

/// Start the embedded `axum` loopback server on `127.0.0.1:0` with a 256-bit per-launch
/// session token. `initial_root` may be `None` (starts in empty folder-picker state).
pub async fn start_server(initial_root: Option<PathBuf>) -> std::io::Result<ServerHandle> {
    let token = generate_session_token();
    let state = AppState::new(token.clone(), initial_root);
    let app = build_router(state.clone());

    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?;
    let addr = listener.local_addr()?;

    tokio::spawn(async move {
        if let Err(err) = axum::serve(listener, app).await {
            tracing::error!("embedded axum server exited: {err}");
        }
    });

    Ok(ServerHandle {
        bootstrap: ServerBootstrap {
            base_url: format!("http://{addr}"),
            token,
        },
        state,
    })
}

/// Legacy Milestone 0 spike helper kept for Gate B smoke tests (`/api/health` + `/m/{token}/media/{*path}`).
pub async fn start_spike_server(media_root: PathBuf) -> std::io::Result<ServerBootstrap> {
    let token = generate_session_token();
    let state = AppState::new(token.clone(), None);
    let media_dir = Arc::new(media_root);

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::OPTIONS])
        .allow_headers([
            header::CONTENT_TYPE,
            header::RANGE,
            header::HeaderName::from_static("x-vsplit-token"),
        ]);

    async fn serve_spike_media(
        State((expected_token, root)): State<(String, Arc<PathBuf>)>,
        Path((tok, rel_path)): Path<(String, String)>,
        req: axum::extract::Request,
    ) -> Response {
        if verify_path_token(&tok, &expected_token).is_err() {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        if rel_path.contains("..") || rel_path.starts_with('/') || rel_path.starts_with('\\') {
            return StatusCode::BAD_REQUEST.into_response();
        }
        let target = root.join(&rel_path);
        let Ok(canon_root) = root.canonicalize() else {
            return StatusCode::NOT_FOUND.into_response();
        };
        let Ok(canon_target) = target.canonicalize() else {
            return StatusCode::NOT_FOUND.into_response();
        };
        if !canon_target.starts_with(&canon_root) || !canon_target.is_file() {
            return StatusCode::NOT_FOUND.into_response();
        }
        match ServeFile::new(canon_target).oneshot(req).await {
            Ok(res) => res.into_response(),
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }

    let spike_media = Router::new()
        .route("/m/{token}/media/{*path}", get(serve_spike_media))
        .with_state((token.clone(), media_dir));

    let app = routes::router(state).merge(spike_media).layer(cors);
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?;
    let addr = listener.local_addr()?;

    tokio::spawn(async move {
        if let Err(err) = axum::serve(listener, app).await {
            tracing::error!("embedded axum spike server exited: {err}");
        }
    });

    Ok(ServerBootstrap {
        base_url: format!("http://{addr}"),
        token,
    })
}
