//! Window module - Main application window with navigation and page management.
//!
//! This module provides the primary application window including:
//!
//! - **Titlebar**: Custom header bar with compact window controls
//! - **Navigation**: Sidebar with page switching and keyboard shortcuts
//! - **Pages**: Dashboard, curves, fan pairing, sensors, graphs, and settings
//! - **Dialogs**: Unsaved changes prompts, daemon status, EC control
//!
//! # Architecture
//!
//! The window is structured as:
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │ Titlebar (custom window controls)           │
//! ├─────────────────────────────────────────────┤
//! │ Daemon Warning Banner (conditional)         │
//! ├──────┬──────────────────────────────────────┤
//! │ Nav  │                                      │
//! │ Bar  │         Page Content                 │
//! │      │         (Stack)                      │
//! │      │                                      │
//! ├──────┴──────────────────────────────────────┤
//! │ Performance Bar (optional, --perf flag)     │
//! └─────────────────────────────────────────────┘
//! ```
//!
//! # Submodules
//!
//! - [`titlebar`]: Custom header bar with window controls
//! - [`navigation`]: Sidebar navigation with state management
//! - [`dialogs`]: Modal dialogs for user interactions

mod titlebar;
mod navigation;
mod dialogs;

// Re-export public API for external consumers
pub use titlebar::Titlebar;

// Future public API (currently unused externally)
#[allow(unused_imports)]
pub use navigation::{Navigator, NavButton, AnimationHandle};

// Internal use
use navigation::{create_nav_button, create_animated_nav_button, setup_icon_spin_animation, set_active_button};
use dialogs::show_unsaved_changes_dialog;

use gtk4::prelude::*;
use gtk4::glib;
use gtk4::{Button, Label, Separator};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use crate::perf::{self, PerfCollector};
use crate::runtime;
use crate::widgets::{CurvesPage, Dashboard, DetectionDialog, FanPairingPage, GraphsPage, NavPage, SensorsPage, SettingsPage};

// ============================================================================
// Constants
// ============================================================================

/// Default window width in pixels
const DEFAULT_WINDOW_WIDTH: i32 = 900;

/// Default window height in pixels
const DEFAULT_WINDOW_HEIGHT: i32 = 650;

/// Daemon status check interval in seconds
const DAEMON_CHECK_INTERVAL_SECS: u64 = 2;

/// Minimum poll interval for dashboard refresh in milliseconds
const MIN_POLL_INTERVAL_MS: u64 = 50;

// ============================================================================
// HyperfanWindow
// ============================================================================

/// The main application window.
///
/// Contains the titlebar, navigation sidebar, page stack, and optional
/// performance metrics bar. Manages window lifecycle, geometry persistence,
/// and daemon connectivity.
pub struct HyperfanWindow {
    /// The underlying GTK application window
    pub window: adw::ApplicationWindow,
    /// Dashboard page reference for refresh callbacks
    dashboard: Rc<RefCell<Option<Rc<Dashboard>>>>,
    /// Performance metrics collector (active when --perf flag is used)
    perf_collector: Rc<RefCell<PerfCollector>>,
    /// Settings page reference for unsaved changes detection
    settings_page: Rc<SettingsPage>,
}

impl HyperfanWindow {
    /// Create a new application window.
    ///
    /// Loads saved geometry from settings, creates all UI components,
    /// and sets up event handlers.
    ///
    /// # Arguments
    ///
    /// * `app` - The libadwaita application instance
    pub fn new(app: &adw::Application) -> Self {
        // Load saved window geometry
        let saved_settings = hf_core::get_cached_settings();
        let default_width = saved_settings.display.window_width.unwrap_or(DEFAULT_WINDOW_WIDTH);
        let default_height = saved_settings.display.window_height.unwrap_or(DEFAULT_WINDOW_HEIGHT);
        let was_maximized = saved_settings.display.window_maximized.unwrap_or(false);

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Hyperfan")
            .default_width(default_width)
            .default_height(default_height)
            .maximized(was_maximized)
            .build();

        let dashboard: Rc<RefCell<Option<Rc<Dashboard>>>> = Rc::new(RefCell::new(None));
        let perf_collector = Rc::new(RefCell::new(PerfCollector::new()));

        // Build UI hierarchy using ToolbarView for proper titlebar integration
        let toolbar_view = adw::ToolbarView::new();

        // Create titlebar with custom window controls
        let titlebar = Titlebar::builder(&window).build();
        let daemon_indicator = titlebar.daemon_indicator().clone();
        let support_button = titlebar.support_button().clone();
        toolbar_view.add_top_bar(titlebar.header());
        
        // Connect support button click handler
        support_button.connect_clicked(|btn| {
            dialogs::show_support_dialog(btn);
        });
        
        // Daemon health warning banner (below header)
        let daemon_banner = crate::daemon_health::create_daemon_warning_banner();
        toolbar_view.add_top_bar(&daemon_banner);

        // Main content box
        let root_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);

        // Content area: nav on left, pages on right
        let content_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        content_box.set_vexpand(true);

        // Navigation bar (vertical, LEFT side) - HIG: 6px spacing, 12px margins
        let nav_bar = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .spacing(6)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();

        // Create nav buttons with proper sizing
        let dash_btn = create_nav_button("go-home-symbolic", "Dashboard");
        let curves_btn = create_nav_button("document-edit-symbolic", "Fan Curves");
        let (fan_pairing_btn, fan_icon, fan_rotation) = create_animated_nav_button("fan-symbolic", "Fan Pairing");
        let sensors_btn = create_nav_button("temp-symbolic", "Sensors");
        let graphs_btn = create_nav_button("org.gnome.SystemMonitor-symbolic", "Graphs");
        let ec_btn = create_nav_button("utilities-terminal-symbolic", "EC Control");

        nav_bar.append(&dash_btn);
        nav_bar.append(&curves_btn);
        nav_bar.append(&fan_pairing_btn);
        nav_bar.append(&sensors_btn);
        nav_bar.append(&graphs_btn);
        
        // EC button only visible when EC control is enabled
        let settings = hf_core::get_cached_settings();
        let ec_enabled = settings.advanced.ec_direct_control_enabled && settings.advanced.ec_danger_acknowledged;
        ec_btn.set_visible(ec_enabled);
        nav_bar.append(&ec_btn);

        // Spacer to push settings button to bottom
        let spacer = gtk4::Box::builder()
            .vexpand(true)
            .build();
        nav_bar.append(&spacer);

        // Settings button at bottom of nav bar
        let settings_btn = create_nav_button("emblem-system-symbolic", "Settings");
        nav_bar.append(&settings_btn);

        content_box.append(&nav_bar);

        // Setup smooth spinning animation for fan pairing button icon
        setup_icon_spin_animation(fan_icon, fan_rotation);

        // Separator between nav and content
        let separator = Separator::new(gtk4::Orientation::Vertical);
        content_box.append(&separator);

        // Stack for pages (RIGHT side, expands)
        let stack = gtk4::Stack::builder()
            .transition_type(gtk4::StackTransitionType::Crossfade)
            .transition_duration(150)
            .vexpand(true)
            .hexpand(true)
            .build();

        // Create pages
        let dash = Rc::new(Dashboard::new());
        stack.add_named(dash.widget(), Some("dashboard"));

        let curves = CurvesPage::new();
        stack.add_named(curves.widget(), Some("curves"));

        let fan_pairing = Rc::new(FanPairingPage::new());
        stack.add_named(fan_pairing.widget(), Some("fan_pairing"));

        let sensors = SensorsPage::new();
        stack.add_named(sensors.widget(), Some("sensors"));

        let graphs = GraphsPage::new();
        stack.add_named(graphs.widget(), Some("graphs"));

        let settings = Rc::new(SettingsPage::new());
        stack.add_named(settings.widget(), Some("settings"));

        // Set initial page based on user's default_page setting
        let app_settings = hf_core::get_cached_settings();
        let default_page = app_settings.general.default_page.as_str();
        let initial_page = match default_page {
            "dashboard" => "dashboard",
            "curves" => "curves",
            "fan_pairing" => "fan_pairing",
            "sensors" => "sensors",
            "graphs" => "graphs",
            _ => "dashboard",
        };
        stack.set_visible_child_name(initial_page);

        content_box.append(&stack);
        root_box.append(&content_box);

        // Performance metrics bar (only shown with --perf flag)
        let perf_bar = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(24)
            .margin_start(12)
            .margin_end(12)
            .margin_top(4)
            .margin_bottom(4)
            .halign(gtk4::Align::Center)
            .build();

        let fps_label = Label::builder()
            .label("FPS: --")
            .css_classes(["caption", "numeric"])
            .build();

        let cpu_label = Label::builder()
            .label("CPU: --%")
            .css_classes(["caption", "numeric"])
            .build();

        let mem_label = Label::builder()
            .label("Mem: --/-- MB")
            .css_classes(["caption", "numeric"])
            .build();

        perf_bar.append(&fps_label);
        perf_bar.append(&cpu_label);
        perf_bar.append(&mem_label);

        if perf::is_enabled() {
            let perf_separator = Separator::new(gtk4::Orientation::Horizontal);
            root_box.append(&perf_separator);
            root_box.append(&perf_bar);
        }

        toolbar_view.set_content(Some(&root_box));
        window.set_content(Some(&toolbar_view));
        
        // Start daemon health monitoring
        let health_monitor = crate::daemon_health::DaemonHealthMonitor::new();
        health_monitor.check_health();
        health_monitor.start_monitoring(10);
        
        let health_for_retry = health_monitor.clone();
        daemon_banner.connect_button_clicked(move |_| {
            health_for_retry.check_health();
        });

        *dashboard.borrow_mut() = Some(dash.clone());

        // Store buttons for active state management
        let buttons = [
            (NavPage::Dashboard, dash_btn.clone()),
            (NavPage::Curves, curves_btn.clone()),
            (NavPage::FanPairing, fan_pairing_btn.clone()),
            (NavPage::Sensors, sensors_btn.clone()),
            (NavPage::Graphs, graphs_btn.clone()),
            (NavPage::EcControl, ec_btn.clone()),
        ];

        // Setup keyboard shortcuts for navigation
        let key_controller = gtk4::EventControllerKey::new();
        let stack_for_keys = stack.clone();
        let buttons_for_keys: Vec<_> = buttons.iter().map(|(p, b)| (*p, b.clone())).collect();
        let settings_btn_for_keys = settings_btn.clone();
        let settings_for_keys = settings.clone();
        let dashboard_for_keys = dashboard.clone();
        
        key_controller.connect_key_pressed(move |_, key, _, modifiers| {
            if !modifiers.contains(gtk4::gdk::ModifierType::CONTROL_MASK) {
                return glib::Propagation::Proceed;
            }
            
            let current_page = stack_for_keys.visible_child_name();
            let settings_page = settings_for_keys.clone();
            
            let navigate = |page_name: &str, nav_page: NavPage| {
                if current_page.as_deref() == Some("settings") && settings_page.has_unsaved_changes() {
                    return;
                }
                stack_for_keys.set_visible_child_name(page_name);
                set_active_button(&buttons_for_keys, nav_page);
                settings_btn_for_keys.remove_css_class("suggested-action");
                
                if page_name == "dashboard" {
                    if let Some(ref dash) = *dashboard_for_keys.borrow() {
                        dash.refresh();
                    }
                }
            };
            
            match key {
                gtk4::gdk::Key::_1 | gtk4::gdk::Key::KP_1 => {
                    navigate("dashboard", NavPage::Dashboard);
                    glib::Propagation::Stop
                }
                gtk4::gdk::Key::_2 | gtk4::gdk::Key::KP_2 => {
                    navigate("curves", NavPage::Curves);
                    glib::Propagation::Stop
                }
                gtk4::gdk::Key::_3 | gtk4::gdk::Key::KP_3 => {
                    navigate("fan_pairing", NavPage::FanPairing);
                    glib::Propagation::Stop
                }
                gtk4::gdk::Key::_4 | gtk4::gdk::Key::KP_4 => {
                    navigate("sensors", NavPage::Sensors);
                    glib::Propagation::Stop
                }
                gtk4::gdk::Key::_5 | gtk4::gdk::Key::KP_5 => {
                    navigate("graphs", NavPage::Graphs);
                    glib::Propagation::Stop
                }
                gtk4::gdk::Key::comma => {
                    stack_for_keys.set_visible_child_name("settings");
                    for (_, btn) in &buttons_for_keys {
                        btn.remove_css_class("suggested-action");
                    }
                    settings_btn_for_keys.add_css_class("suggested-action");
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed
            }
        });
        window.add_controller(key_controller);
        
        // Connect dashboard "Go to Fan Curves" navigation
        let stack_for_dash_nav = stack.clone();
        let buttons_for_dash_nav: Vec<_> = buttons.iter().map(|(p, b)| (*p, b.clone())).collect();
        let settings_btn_for_dash_nav = settings_btn.clone();
        dash.connect_navigate_curves(move || {
            stack_for_dash_nav.set_visible_child_name("curves");
            set_active_button(&buttons_for_dash_nav, NavPage::Curves);
            settings_btn_for_dash_nav.remove_css_class("suggested-action");
        });

        // Set initial active state
        let initial_nav_page = match initial_page {
            "dashboard" => NavPage::Dashboard,
            "curves" => NavPage::Curves,
            "fan_pairing" => NavPage::FanPairing,
            "sensors" => NavPage::Sensors,
            "graphs" => NavPage::Graphs,
            _ => NavPage::Dashboard,
        };
        set_active_button(&buttons.iter().map(|(p, b)| (*p, b.clone())).collect::<Vec<_>>(), initial_nav_page);

        // Navigation handlers
        Self::setup_nav_handlers(
            &stack,
            &buttons,
            &settings_btn,
            &dashboard,
            &settings,
            &dash,
            &ec_btn,
            &daemon_indicator,
            &fan_pairing,
        );

        let this = Self { window, dashboard, perf_collector, settings_page: settings.clone() };

        // Setup periodic daemon status check
        let daemon_indicator_for_check = daemon_indicator.clone();
        let fan_pairing_for_check = fan_pairing.clone();
        let dashboard_for_check = dash.clone();
        let was_available = Rc::new(RefCell::new(hf_core::is_daemon_available()));
        glib::timeout_add_local(Duration::from_secs(DAEMON_CHECK_INTERVAL_SECS), move || {
            let is_available = hf_core::is_daemon_available();
            daemon_indicator_for_check.set_visible(is_available);
            
            let prev_available = *was_available.borrow();
            if is_available && !prev_available {
                tracing::info!("Daemon became available - refreshing pages");
                fan_pairing_for_check.refresh();
                dashboard_for_check.refresh();
            }
            *was_available.borrow_mut() = is_available;
            
            glib::ControlFlow::Continue
        });

        daemon_indicator.set_visible(hf_core::is_daemon_available());

        // Handle window close
        let settings_for_close = settings.clone();
        this.window.connect_close_request(move |window| {
            Self::save_window_geometry(window);
            
            if settings_for_close.has_unsaved_changes() {
                let dialog = adw::AlertDialog::builder()
                    .heading("Unsaved Changes")
                    .body("You have unsaved settings changes. Do you want to save them before closing?")
                    .build();

                dialog.add_response("cancel", "Cancel");
                dialog.add_response("discard", "Discard");
                dialog.add_response("save", "Save");
                dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
                dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
                dialog.set_default_response(Some("save"));
                dialog.set_close_response("cancel");

                let window_for_response = window.clone();
                let settings_for_response = settings_for_close.clone();
                dialog.connect_response(None, move |_dialog, response| {
                    match response {
                        "save" => {
                            settings_for_response.apply_settings();
                            window_for_response.close();
                        }
                        "discard" => {
                            settings_for_response.reset_dirty();
                            window_for_response.close();
                        }
                        _ => {}
                    }
                });

                dialog.present(Some(window));
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });

        this.setup_refresh_tick(stack.clone());

        if perf::is_enabled() {
            let perf_collector_for_tick = this.perf_collector.clone();
            let fps_label_clone = fps_label.clone();
            let cpu_label_clone = cpu_label.clone();
            let mem_label_clone = mem_label.clone();
            
            this.window.add_tick_callback(move |_widget, _frame_clock| {
                perf_collector_for_tick.borrow_mut().tick_frame();
                let metrics = perf_collector_for_tick.borrow().get_metrics();
                
                let frame_count = perf_collector_for_tick.borrow().frame_count();
                if frame_count % 8 == 0 {
                    fps_label_clone.set_label(&format!("FPS: {:.0}", metrics.fps));
                    cpu_label_clone.set_label(&format!("CPU: {:.1}%", metrics.cpu_percent));
                    mem_label_clone.set_label(&format!("Mem: {}", metrics.memory_str()));
                }
                
                glib::ControlFlow::Continue
            });
        }

        this
    }

    #[allow(clippy::too_many_arguments)]
    fn setup_nav_handlers(
        stack: &gtk4::Stack,
        buttons: &[(NavPage, Button)],
        settings_btn: &Button,
        dashboard: &Rc<RefCell<Option<Rc<Dashboard>>>>,
        settings: &Rc<SettingsPage>,
        dash: &Rc<Dashboard>,
        ec_btn: &Button,
        daemon_indicator: &Button,
        fan_pairing: &Rc<FanPairingPage>,
    ) {
        // Dashboard button
        let stack_clone = stack.clone();
        let buttons_clone: Vec<_> = buttons.iter().map(|(p, b)| (*p, b.clone())).collect();
        let dashboard_for_nav = dashboard.clone();
        let settings_for_dash = settings.clone();
        let settings_btn_for_dash = settings_btn.clone();
        let dash_btn = buttons.iter().find(|(p, _)| *p == NavPage::Dashboard).map(|(_, b)| b.clone()).unwrap();
        dash_btn.connect_clicked(move |btn| {
            let stack = stack_clone.clone();
            let buttons = buttons_clone.clone();
            let dashboard = dashboard_for_nav.clone();
            let settings_page = settings_for_dash.clone();
            let settings_btn = settings_btn_for_dash.clone();
            
            if stack.visible_child_name().as_deref() == Some("settings") && settings_page.has_unsaved_changes() {
                show_unsaved_changes_dialog(btn, settings_page, move || {
                    stack.set_visible_child_name("dashboard");
                    set_active_button(&buttons, NavPage::Dashboard);
                    settings_btn.remove_css_class("suggested-action");
                    if let Some(ref dash) = *dashboard.borrow() {
                        dash.refresh();
                    }
                });
            } else {
                stack.set_visible_child_name("dashboard");
                set_active_button(&buttons, NavPage::Dashboard);
                settings_btn_for_dash.remove_css_class("suggested-action");
                if let Some(ref dash) = *dashboard.borrow() {
                    dash.refresh();
                }
            }
        });

        // Curves button
        let stack_clone = stack.clone();
        let buttons_clone: Vec<_> = buttons.iter().map(|(p, b)| (*p, b.clone())).collect();
        let settings_for_curves = settings.clone();
        let settings_btn_for_curves = settings_btn.clone();
        let curves_btn = buttons.iter().find(|(p, _)| *p == NavPage::Curves).map(|(_, b)| b.clone()).unwrap();
        curves_btn.connect_clicked(move |btn| {
            let stack = stack_clone.clone();
            let buttons = buttons_clone.clone();
            let settings_page = settings_for_curves.clone();
            let settings_btn = settings_btn_for_curves.clone();
            
            if stack.visible_child_name().as_deref() == Some("settings") && settings_page.has_unsaved_changes() {
                show_unsaved_changes_dialog(btn, settings_page, move || {
                    stack.set_visible_child_name("curves");
                    set_active_button(&buttons, NavPage::Curves);
                    settings_btn.remove_css_class("suggested-action");
                });
            } else {
                stack.set_visible_child_name("curves");
                set_active_button(&buttons, NavPage::Curves);
                settings_btn_for_curves.remove_css_class("suggested-action");
            }
        });

        // Fan pairing button
        let stack_clone = stack.clone();
        let buttons_clone: Vec<_> = buttons.iter().map(|(p, b)| (*p, b.clone())).collect();
        let settings_for_fan = settings.clone();
        let settings_btn_for_fan = settings_btn.clone();
        let fan_pairing_btn = buttons.iter().find(|(p, _)| *p == NavPage::FanPairing).map(|(_, b)| b.clone()).unwrap();
        fan_pairing_btn.connect_clicked(move |btn| {
            let stack = stack_clone.clone();
            let buttons = buttons_clone.clone();
            let settings_page = settings_for_fan.clone();
            let settings_btn = settings_btn_for_fan.clone();
            
            if stack.visible_child_name().as_deref() == Some("settings") && settings_page.has_unsaved_changes() {
                show_unsaved_changes_dialog(btn, settings_page, move || {
                    stack.set_visible_child_name("fan_pairing");
                    set_active_button(&buttons, NavPage::FanPairing);
                    settings_btn.remove_css_class("suggested-action");
                });
            } else {
                stack.set_visible_child_name("fan_pairing");
                set_active_button(&buttons, NavPage::FanPairing);
                settings_btn_for_fan.remove_css_class("suggested-action");
            }
        });

        // Sensors button
        let stack_clone = stack.clone();
        let buttons_clone: Vec<_> = buttons.iter().map(|(p, b)| (*p, b.clone())).collect();
        let settings_for_sensors = settings.clone();
        let settings_btn_for_sensors = settings_btn.clone();
        let sensors_btn = buttons.iter().find(|(p, _)| *p == NavPage::Sensors).map(|(_, b)| b.clone()).unwrap();
        sensors_btn.connect_clicked(move |btn| {
            let stack = stack_clone.clone();
            let buttons = buttons_clone.clone();
            let settings_page = settings_for_sensors.clone();
            let settings_btn = settings_btn_for_sensors.clone();
            
            if stack.visible_child_name().as_deref() == Some("settings") && settings_page.has_unsaved_changes() {
                show_unsaved_changes_dialog(btn, settings_page, move || {
                    stack.set_visible_child_name("sensors");
                    set_active_button(&buttons, NavPage::Sensors);
                    settings_btn.remove_css_class("suggested-action");
                });
            } else {
                stack.set_visible_child_name("sensors");
                set_active_button(&buttons, NavPage::Sensors);
                settings_btn_for_sensors.remove_css_class("suggested-action");
            }
        });

        // Graphs button
        let stack_clone = stack.clone();
        let buttons_clone: Vec<_> = buttons.iter().map(|(p, b)| (*p, b.clone())).collect();
        let settings_for_graphs = settings.clone();
        let settings_btn_for_graphs = settings_btn.clone();
        let graphs_btn = buttons.iter().find(|(p, _)| *p == NavPage::Graphs).map(|(_, b)| b.clone()).unwrap();
        graphs_btn.connect_clicked(move |btn| {
            let stack = stack_clone.clone();
            let buttons = buttons_clone.clone();
            let settings_page = settings_for_graphs.clone();
            let settings_btn = settings_btn_for_graphs.clone();
            
            if stack.visible_child_name().as_deref() == Some("settings") && settings_page.has_unsaved_changes() {
                show_unsaved_changes_dialog(btn, settings_page, move || {
                    stack.set_visible_child_name("graphs");
                    set_active_button(&buttons, NavPage::Graphs);
                    settings_btn.remove_css_class("suggested-action");
                });
            } else {
                stack.set_visible_child_name("graphs");
                set_active_button(&buttons, NavPage::Graphs);
                settings_btn_for_graphs.remove_css_class("suggested-action");
            }
        });

        // EC Control button
        ec_btn.connect_clicked(move |btn| {
            dialogs::show_ec_control_dialog(btn);
        });

        // Settings button
        let stack_for_settings = stack.clone();
        let buttons_for_settings: Vec<_> = buttons.iter().map(|(p, b)| (*p, b.clone())).collect();
        let settings_btn_for_click = settings_btn.clone();
        settings_btn.connect_clicked(move |_| {
            stack_for_settings.set_visible_child_name("settings");
            for (_, btn) in &buttons_for_settings {
                btn.remove_css_class("suggested-action");
            }
            settings_btn_for_click.add_css_class("suggested-action");
        });

        // Daemon indicator click
        let stack_for_daemon = stack.clone();
        let buttons_for_daemon: Vec<_> = buttons.iter().map(|(p, b)| (*p, b.clone())).collect();
        let settings_for_daemon = settings.clone();
        let settings_btn_for_daemon = settings_btn.clone();
        daemon_indicator.connect_clicked(move |btn| {
            let stack = stack_for_daemon.clone();
            let buttons = buttons_for_daemon.clone();
            let settings_page = settings_for_daemon.clone();
            let settings_btn = settings_btn_for_daemon.clone();
            
            if stack.visible_child_name().as_deref() == Some("settings") && settings_page.has_unsaved_changes() {
                show_unsaved_changes_dialog(btn, settings_page, move || {
                    stack.set_visible_child_name("settings");
                    for (_, btn) in &buttons {
                        btn.remove_css_class("suggested-action");
                    }
                    settings_btn.add_css_class("suggested-action");
                });
            } else {
                stack.set_visible_child_name("settings");
                for (_, btn) in &buttons {
                    btn.remove_css_class("suggested-action");
                }
                settings_btn_for_daemon.add_css_class("suggested-action");
            }
        });
    }

    fn setup_refresh_tick(&self, stack: gtk4::Stack) {
        let dashboard = self.dashboard.clone();
        
        let poll_interval_ms = hf_core::get_cached_settings().general.poll_interval_ms as u64;
        let poll_interval_ms = poll_interval_ms.max(MIN_POLL_INTERVAL_MS);

        glib::timeout_add_local(Duration::from_millis(poll_interval_ms), move || {
            if stack.visible_child_name().as_deref() != Some("dashboard") {
                return glib::ControlFlow::Continue;
            }
            
            if runtime::get_sensors().is_some() {
                if let Some(ref dash) = *dashboard.borrow() {
                    dash.refresh_temps_only();
                }
            }
            glib::ControlFlow::Continue
        });
    }

    fn save_window_geometry(window: &adw::ApplicationWindow) {
        let (width, height) = window.default_size();
        let is_maximized = window.is_maximized();
        
        if let Ok(mut settings) = hf_core::load_settings() {
            if width > 0 && height > 0 {
                settings.display.window_width = Some(width);
                settings.display.window_height = Some(height);
            }
            settings.display.window_maximized = Some(is_maximized);
            
            if let Err(e) = hf_core::save_settings(&settings) {
                tracing::warn!("Failed to save window geometry: {}", e);
            }
        }
    }

    pub fn present(&self) {
        self.window.present();
        self.check_daemon_connectivity();
        self.check_first_run_detection();
    }

    fn check_daemon_connectivity(&self) {
        if hf_core::daemon_list_hardware().is_err() {
            dialogs::show_daemon_not_running_dialog(&self.window, self.settings_page.clone());
        }
    }

    fn check_first_run_detection(&self) {
        let detection_completed = hf_core::is_detection_completed().unwrap_or(false);
        
        if !detection_completed {
            let dialog = DetectionDialog::new();
            dialog.connect_complete(|mappings| {
                tracing::info!("PWM-fan detection complete: {} mappings found", mappings.len());
            });
            dialog.present();
        }
    }
}
