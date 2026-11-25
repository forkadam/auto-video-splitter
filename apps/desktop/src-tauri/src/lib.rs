pub mod cli_shim;
pub mod commands;
pub mod updater;

use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use tauri::{Manager, RunEvent};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;
use vsplit_core::config::load_app_config;
use vsplit_core::paths::app_logs_dir;
use vsplit_server::session::AppState as ServerAppState;
use vsplit_server::{start_server, ServerBootstrap};

pub struct DesktopState {
    pub bootstrap: ServerBootstrap,
    pub server_state: ServerAppState,
    pub boot_ms: u128,
}

fn init_logging() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let logs_dir = app_logs_dir();
    let _ = fs::create_dir_all(&logs_dir);
    let file_appender = tracing_appender::rolling::daily(&logs_dir, "vsplit-desktop.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::registry()
        .with(env_filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(non_blocking),
        )
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .try_init();

    Some(guard)
}

pub fn run() {
    let t0 = Instant::now();
    let _log_guard = init_logging();

    let app_cfg = load_app_config().unwrap_or_default();
    let initial_root: Option<PathBuf> = std::env::var("VSPLIT_INITIAL_DIR")
        .ok()
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .or_else(|| app_cfg.last_folder.filter(|p| p.is_dir()));

    let rt = tokio::runtime::Runtime::new().expect("failed to start tokio runtime");
    let handle = rt
        .block_on(start_server(initial_root))
        .expect("failed to start embedded vsplit-server");
    // Keep Tokio runtime alive for the lifetime of the desktop process.
    std::mem::forget(rt);

    let boot_ms = t0.elapsed().as_millis();
    tracing::info!(
        base_url = %handle.bootstrap.base_url,
        boot_ms,
        "vsplit-desktop embedded loopback server ready"
    );

    let server_state_for_exit = handle.state.clone();
    let desktop_state = DesktopState {
        bootstrap: handle.bootstrap,
        server_state: handle.state,
        boot_ms,
    };

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(desktop_state)
        .setup(|app| {
            // On Linux AppImage startup, keep `~/.local/bin/vsplit` version-locked to the GUI.
            if let Err(err) = cli_shim::sync_if_needed(app.handle()) {
                tracing::warn!(error = %err, "failed to sync Linux CLI shim on startup");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_server_bootstrap,
            commands::get_app_config,
            commands::set_app_config,
            commands::pick_folder,
            commands::pick_file,
            commands::repair_cli_shim,
            updater::check_update_available,
            updater::check_and_install_update,
        ])
        .build(tauri::generate_context!())
        .expect("error while building vsplit-desktop application");

    app.run(move |app_handle, event| {
        if let RunEvent::ExitRequested { .. } | RunEvent::Exit = event {
            // Cancel any in-flight pipeline jobs and allow bounded child ffmpeg teardown.
            let active = server_state_for_exit.coordinator.active_count();
            if active > 0 {
                tracing::info!(active, "cancelling active jobs before exit");
                server_state_for_exit.coordinator.cancel_all();
                let deadline = Instant::now() + std::time::Duration::from_secs(2);
                while server_state_for_exit.coordinator.active_count() > 0
                    && Instant::now() < deadline
                {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
            let _ = app_handle.state::<DesktopState>();
        }
    });
}
