mod app;
mod cli;
mod daemon_health;
pub mod perf;
pub mod runtime;
pub mod tray;
mod widgets;
mod window;

use clap::Parser;
use gtk4::glib;
use tracing::{debug, info, warn};

const APP_ID: &str = "io.github.hyperfan";

fn main() -> glib::ExitCode {
    // Parse command line arguments with clap
    let cli_args = cli::Cli::parse();
    
    // Enable perf overlay if requested
    if cli_args.perf {
        perf::enable();
    }
    
    // Initialize tracing only if RUST_LOG is set (for debugging)
    if std::env::var("RUST_LOG").is_ok() {
        tracing_subscriber::fmt::init();
    }
    
    // Handle CLI commands (non-GUI)
    match cli::run_cli(&cli_args) {
        Ok(true) => return glib::ExitCode::SUCCESS, // CLI handled
        Ok(false) => {} // Continue to GUI
        Err(e) => {
            eprintln!("Error: {}", e);
            return glib::ExitCode::FAILURE;
        }
    }
    
    // ========================================================================
    // Window Manager Selection (must be checked FIRST before loading any GUI)
    // ========================================================================
    let effective_wm = hf_core::get_effective_window_manager();
    
    match effective_wm {
        hf_core::WindowManager::Kde => {
            warn!(
                "KDE frontend is not implemented; falling back to GTK frontend (effective WM: {})",
                effective_wm
            );
        }
        hf_core::WindowManager::Gnome | hf_core::WindowManager::Unknown => {
            // Continue with GTK frontend
            debug!("Using GTK frontend (effective WM: {})", effective_wm);
        }
    }
    
    // Apply display backend setting (must be done before GTK init)
    // This sets GDK_BACKEND which controls Wayland vs X11
    if let Ok(settings) = hf_core::load_settings() {
        match settings.display.display_backend.as_str() {
            "wayland" => {
                std::env::set_var("GDK_BACKEND", "wayland");
            }
            "x11" => {
                std::env::set_var("GDK_BACKEND", "x11");
            }
            _ => {
                // "auto" - let GTK decide based on environment.
                // IMPORTANT: ensure we don't inherit a previously forced backend across restarts.
                std::env::remove_var("GDK_BACKEND");
            }
        }
    }
    
    // Configure GPU-accelerated rendering for Wayland/X11
    // GSK (GTK Scene Kit) handles rendering - use GL for best compatibility
    if std::env::var("GSK_RENDERER").is_err() {
        // Detect display backend and set appropriate GPU renderer
        let is_wayland = std::env::var("WAYLAND_DISPLAY").is_ok();
        let is_x11 = std::env::var("DISPLAY").is_ok();
        
        if is_wayland || is_x11 {
            // Use GL renderer for GPU acceleration (more stable than Vulkan)
            // ngl = new GL renderer in GTK4, falls back to gl if unavailable
            std::env::set_var("GSK_RENDERER", "ngl");
        }
    }
    
    // Initialize the Tokio worker runtime before GTK
    runtime::init_runtime();
    
    // Load and apply settings on startup
    apply_startup_settings();
    
    let app = app::HyperfanApp::new(APP_ID);
    app.run()
}

/// Apply settings and fan curves on startup
fn apply_startup_settings() {
    // Load settings
    let settings = match hf_core::load_settings() {
        Ok(s) => s,
        Err(e) => {
            warn!("Failed to load settings: {}", e);
            return;
        }
    };
    
    info!("Settings loaded from {:?}", hf_core::get_settings_path().ok());
    
    // Check if we should apply curves on startup
    if !settings.general.apply_curves_on_startup {
        debug!("apply_curves_on_startup is disabled, skipping curve application");
        return;
    }

    // Daemon authoritative: signal daemon to reload and apply config/curves.
    // The GUI should not touch sysfs or directly apply curves.
    if let Err(e) = hf_core::daemon_reload_config() {
        debug!("Failed to signal daemon reload on startup: {}", e);
    }
}
