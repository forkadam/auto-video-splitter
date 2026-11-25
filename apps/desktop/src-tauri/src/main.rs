// Prevents additional console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Apply persisted Wayland WebKitGTK DMABUF compatibility toggle before GTK initializes.
    let env_disable = std::env::var("VSPLIT_DISABLE_DMABUF").as_deref() == Ok("1");
    let cfg_disable = vsplit_core::config::load_app_config()
        .map(|c| c.wayland_disable_dmabuf)
        .unwrap_or(false);
    if env_disable || cfg_disable {
        // SAFETY: invoked at process entry before spawning threads or initializing GTK.
        unsafe {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }

    vsplit_desktop_lib::run();
}
