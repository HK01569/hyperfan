//! Hyperfan GTK Application
//!
//! Main application entry point with theme and CSS initialization.

use gtk4::gio;
use gtk4::gdk::Display;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::CssProvider;
use libadwaita as adw;
use std::time::Duration;
use gtk4::IconTheme;

use crate::window::HyperfanWindow;
use crate::tray;

/// Main Hyperfan GTK application
pub struct HyperfanApp {
    app: adw::Application,
}

impl HyperfanApp {
    /// Create a new application instance
    pub fn new(app_id: &str) -> Self {
        let app = adw::Application::builder()
            .application_id(app_id)
            .flags(gio::ApplicationFlags::FLAGS_NONE)
            .build();

        app.connect_activate(Self::on_activate);
        app.connect_startup(Self::on_startup);

        Self { app }
    }

    /// Called once at application startup - initialize libadwaita
    fn on_startup(_app: &adw::Application) {
        adw::init().expect("FATAL: Failed to initialize libadwaita. Please ensure GTK4 and libadwaita are properly installed on your system.");
        Self::apply_saved_color_scheme();
    }

    /// Apply the saved color scheme from settings
    fn apply_saved_color_scheme() {
        let settings = match hf_core::load_settings() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Failed to load settings for color scheme, using defaults: {}", e);
                hf_core::AppSettings::default()
            }
        };
        let style_manager = adw::StyleManager::default();
        
        let color_scheme = match settings.display.color_scheme.as_str() {
            "light" => adw::ColorScheme::ForceLight,
            "dark" => adw::ColorScheme::ForceDark,
            // PreferLight = use light UNLESS system prefers dark (true system following)
            _ => adw::ColorScheme::PreferLight,
        };
        
        style_manager.set_color_scheme(color_scheme);
    }

    /// Load custom CSS stylesheets from modular files
    /// 
    /// CSS modules are loaded in dependency order:
    /// 1. variables.css - Design tokens (must be first)
    /// 2. Component modules (buttons, navigation, cards, etc.)
    /// 3. accessibility.css - A11y enhancements (must be last for overrides)
    fn load_css() {
        let css_modules = [
            include_str!("styles/variables.css"),
            include_str!("styles/buttons.css"),
            include_str!("styles/window-controls.css"),
            include_str!("styles/navigation.css"),
            include_str!("styles/cards.css"),
            include_str!("styles/graphs.css"),
            include_str!("styles/lists.css"),
            include_str!("styles/forms.css"),
            include_str!("styles/typography.css"),
            include_str!("styles/status.css"),
            include_str!("styles/layout.css"),
            include_str!("styles/utilities.css"),
            include_str!("styles/accessibility.css"),
        ];
        
        // Concatenate all CSS modules into a single stylesheet
        let combined_css = css_modules.join("\n\n");
        
        let provider = CssProvider::new();
        provider.load_from_string(&combined_css);

        if let Some(display) = Display::default() {
            gtk4::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 100,
            );
        }
    }

    /// Register custom icons from the icons directory
    fn register_custom_icons() {
        if let Some(display) = Display::default() {
            let icon_theme = IconTheme::for_display(&display);
            
            // add_search_path expects the PARENT directory containing theme folders (e.g. hicolor)
            // So we add "icons/" not "icons/hicolor/"
            let exe_path = std::env::current_exe().ok();
            let icons_paths = [
                // Development: relative to project root
                std::path::PathBuf::from("icons"),
                // Installed: /usr/share/hyperfan/icons
                std::path::PathBuf::from("/usr/share/hyperfan/icons"),
                // Local install: ~/.local/share/hyperfan/icons
                dirs::data_local_dir().map(|p| p.join("hyperfan/icons")).unwrap_or_default(),
                // Relative to executable
                exe_path.as_ref().and_then(|p| p.parent()).map(|p| p.join("icons")).unwrap_or_default(),
            ];
            
            for path in &icons_paths {
                if path.exists() && path.join("hicolor").exists() {
                    icon_theme.add_search_path(path);
                    tracing::debug!("Added icon search path: {:?}", path);
                }
            }
        }
    }

    /// Called when application is activated - create main window
    fn on_activate(app: &adw::Application) {
        // Load CSS here (not in startup) because Display::default() is None during startup
        Self::load_css();
        Self::register_custom_icons();
        
        let settings = match hf_core::load_settings() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Failed to load settings for tray icon, using defaults: {}", e);
                hf_core::AppSettings::default()
            }
        };
        
        // Start tray icon if enabled
        if settings.display.show_tray_icon {
            tray::start_tray();
        }
        
        let hyperfan_window = HyperfanWindow::new(app);
        let window = hyperfan_window.window.clone();
        
        window.present();
        
        // Setup periodic check for tray commands
        let window_for_tray = window.clone();
        let app_clone = app.clone();
        glib::timeout_add_local(Duration::from_millis(100), move || {
            // Check if tray requested to show window
            if tray::should_show_window() {
                window_for_tray.present();
            }
            
            // Check if tray requested to quit
            if tray::should_quit() {
                app_clone.quit();
                return glib::ControlFlow::Break;
            }
            
            glib::ControlFlow::Continue
        });
    }

    /// Run the application main loop
    /// Filters out custom flags (--perf, --help) so GTK doesn't complain
    pub fn run(&self) -> glib::ExitCode {
        // Filter out our custom arguments that GTK doesn't know about
        let filtered_args: Vec<String> = std::env::args()
            .filter(|arg| !matches!(arg.as_str(), "--perf" | "--help" | "-h"))
            .collect();
        
        self.app.run_with_args(&filtered_args)
    }
}
