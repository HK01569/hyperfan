//! Settings Page
//!
//! Application settings with General, Display, and About sections.
//! Settings are staged in memory and saved when Apply is clicked.
//! Tracks dirty state and prompts user on navigation if unsaved.

#![allow(dead_code)]
#![allow(deprecated)]

use gtk4::prelude::*;
use gtk4::glib;
use gtk4::Box as GtkBox;
use gtk4::{Button, Label, Orientation, ScrolledWindow};
use gtk4::gio;
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use tracing::{debug, error, info, warn};

/// Settings page with sections
pub struct SettingsPage {
    container: GtkBox,
    pending_settings: Rc<RefCell<hf_core::AppSettings>>,
    is_dirty: Rc<RefCell<bool>>,
    apply_btn: Button,
    daemon_service_row: Rc<RefCell<Option<adw::ActionRow>>>,
    on_daemon_installed: Rc<RefCell<Option<Box<dyn Fn()>>>>,
}

impl SettingsPage {
    pub fn new() -> Self {
        let container = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .build();

        // Load current settings
        let settings = hf_core::load_settings().unwrap_or_else(|e| {
            warn!("Failed to load settings, using defaults: {}", e);
            hf_core::AppSettings::default()
        });
        
        // Pending settings (staged changes, not yet saved)
        let pending_settings = Rc::new(RefCell::new(settings.clone()));
        let is_dirty = Rc::new(RefCell::new(false));

        // Header row with title and Apply button
        let header_box = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .margin_start(24)
            .margin_end(24)
            .margin_top(24)
            .margin_bottom(12)
            .build();

        let page_title = Label::builder()
            .label("Settings")
            .css_classes(["title-1"])
            .halign(gtk4::Align::Start)
            .hexpand(true)
            .build();

        let apply_btn = Button::builder()
            .label("Apply")
            .css_classes(["suggested-action"])
            .sensitive(false)
            .build();

        header_box.append(&page_title);
        header_box.append(&apply_btn);
        
        container.append(&header_box);
        
        // Add keyboard shortcut for Ctrl+S to apply settings
        let key_controller = gtk4::EventControllerKey::new();
        let apply_btn_for_keys = apply_btn.clone();
        key_controller.connect_key_pressed(move |_, key, _, modifiers| {
            if modifiers.contains(gtk4::gdk::ModifierType::CONTROL_MASK) {
                if matches!(key, gtk4::gdk::Key::s | gtk4::gdk::Key::S) {
                    if apply_btn_for_keys.is_sensitive() {
                        apply_btn_for_keys.activate();
                    }
                    return glib::Propagation::Stop;
                }
            }
            glib::Propagation::Proceed
        });
        container.add_controller(key_controller);

        let scroll = ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .build();

        let content = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(24)
            .margin_start(24)
            .margin_end(24)
            .margin_top(12)
            .margin_bottom(24)
            .build();

        // ================================================================
        // About Section (TOP)
        // ================================================================
        let about_group = adw::PreferencesGroup::builder()
            .title("About")
            .build();

        // Program name and description
        let app_row = adw::ActionRow::builder()
            .title("Hyperfan")
            .subtitle("A modern, GPU-accelerated fan control application for Linux. \
                       Control your system's cooling with custom fan curves, \
                       real-time temperature monitoring, and intelligent hardware detection.")
            .build();
        about_group.add(&app_row);

        // Version
        let version_row = adw::ActionRow::builder()
            .title("Version")
            .subtitle(env!("CARGO_PKG_VERSION"))
            .build();
        about_group.add(&version_row);

        // Author
        let author_row = adw::ActionRow::builder()
            .title("Author")
            .subtitle("Henry Kleyn")
            .build();
        about_group.add(&author_row);

        content.append(&about_group);

        // ================================================================
        // Licensing Section
        // ================================================================
        let licensing_group = adw::PreferencesGroup::builder()
            .title("Licensing")
            .build();

        // License - clickable to show full license text
        let license_row = adw::ActionRow::builder()
            .title("License")
            .subtitle("GPL-3.0-or-later")
            .activatable(true)
            .build();

        let license_btn = Button::builder()
            .icon_name("go-next-symbolic")
            .valign(gtk4::Align::Center)
            .tooltip_text("View full license")
            .css_classes(["flat"])
            .build();
        license_row.add_suffix(&license_btn);
        license_row.set_activatable_widget(Some(&license_btn));

        license_btn.connect_clicked(move |btn| {
            Self::show_license_dialog(btn);
        });

        licensing_group.add(&license_row);

        // Free, Open Source Software - clickable to show dependencies
        let foss_row = adw::ActionRow::builder()
            .title("Free, Open Source Software")
            .subtitle("View all open source dependencies used by Hyperfan")
            .activatable(true)
            .build();

        let foss_btn = Button::builder()
            .icon_name("go-next-symbolic")
            .valign(gtk4::Align::Center)
            .tooltip_text("View dependencies")
            .css_classes(["flat"])
            .build();
        foss_row.add_suffix(&foss_btn);
        foss_row.set_activatable_widget(Some(&foss_btn));

        foss_btn.connect_clicked(move |btn| {
            Self::show_foss_dialog(btn);
        });

        licensing_group.add(&foss_row);
        content.append(&licensing_group);

        // ================================================================
        // System Section
        // ================================================================
        let general_group = adw::PreferencesGroup::builder()
            .title("System")
            .build();

        // Daemon installation and version
        let service_installed = hf_core::service::is_service_installed();
        
        // Get daemon version
        let daemon_version = hf_core::get_daemon_version().ok();
        let daemon_running = daemon_version.is_some();
        
        // Build subtitle with version info
        let subtitle = if daemon_running {
            if let Some(ref dv) = daemon_version {
                format!("Running v{}", dv)
            } else {
                "Running".to_string()
            }
        } else if service_installed {
            "Installed but not running".to_string()
        } else {
            "Not installed".to_string()
        };
        
        let service_row = adw::ActionRow::builder()
            .title("Daemon")
            .subtitle(&subtitle)
            .build();
        
        // Store reference for flash animation
        let daemon_service_row = Rc::new(RefCell::new(Some(service_row.clone())));
        
        // Button box for multiple buttons
        let btn_box = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(6)
            .valign(gtk4::Align::Center)
            .build();
        
        // Install/Uninstall button
        let service_btn = if service_installed {
            Button::builder()
                .label("Uninstall")
                .css_classes(["destructive-action"])
                .build()
        } else {
            Button::builder()
                .label("Install")
                .css_classes(["suggested-action"])
                .build()
        };
        btn_box.append(&service_btn);
        
        service_row.add_suffix(&btn_box);
        general_group.add(&service_row);

        // Apply curves on startup - only visible when service is installed
        let apply_row = adw::SwitchRow::builder()
            .title("Apply Curves on Startup")
            .subtitle("Automatically apply fan curves when the service starts")
            .sensitive(service_installed)
            .build();
        apply_row.set_active(settings.general.apply_curves_on_startup && service_installed);
        apply_row.set_visible(service_installed);
        
        let pending_for_apply = pending_settings.clone();
        let dirty_for_apply = is_dirty.clone();
        let apply_btn_for_apply = apply_btn.clone();
        apply_row.connect_active_notify(move |row| {
            let enabled = row.is_active();
            pending_for_apply.borrow_mut().general.apply_curves_on_startup = enabled;
            *dirty_for_apply.borrow_mut() = true;
            apply_btn_for_apply.set_sensitive(true);
        });
        general_group.add(&apply_row);
        
        // Service button click handler - updates apply_row visibility
        // Uses async polling to avoid blocking GTK main thread during install/uninstall
        let service_row_clone = service_row.clone();
        let apply_row_clone = apply_row.clone();
        service_btn.connect_clicked(move |btn| {
            let is_installed = hf_core::service::is_service_installed();
            
            // Disable button during operation to prevent double-clicks
            btn.set_sensitive(false);
            btn.set_label(if is_installed { "Uninstalling..." } else { "Installing..." });

            let btn_clone = btn.clone();
            let service_row_for_poll = service_row_clone.clone();
            let apply_row_for_poll = apply_row_clone.clone();

            let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
            std::thread::spawn(move || {
                let result = if is_installed {
                    hf_core::service::uninstall_service()
                } else {
                    hf_core::service::install_service()
                };
                let _ = tx.send(result);
            });

            // Poll the receiver on the GTK main thread.
            glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
                match rx.try_recv() {
                    Ok(result) => {
                        match result {
                            Ok(_) => {
                                // Use async polling instead of blocking sleep
                                // This keeps the GTK main loop responsive
                                let retry_count = Rc::new(RefCell::new(0u32));

                                let btn_for_inner = btn_clone.clone();
                                let service_row_for_inner = service_row_for_poll.clone();
                                let apply_row_for_inner = apply_row_for_poll.clone();

                                glib::timeout_add_local(std::time::Duration::from_millis(300), move || {
                                    let mut count = retry_count.borrow_mut();
                                    *count += 1;

                                    // Try to get daemon version AND verify hardware data is available
                                    let daemon_version = hf_core::get_daemon_version().ok();
                                    let now_installed = hf_core::service::is_service_installed();
                                    
                                    // CRITICAL: Wait until daemon can actually serve hardware data
                                    // This ensures sensor data is available immediately after install
                                    let hardware_ready = hf_core::daemon_list_hardware()
                                        .map(|hw| !hw.chips.is_empty())
                                        .unwrap_or(false);

                                    // Stop polling after 10 attempts or if daemon is fully ready
                                    let should_stop = *count >= 10
                                        || (daemon_version.is_some() && hardware_ready)
                                        || (!now_installed && *count >= 2);

                                    if should_stop {
                                        // Update UI with final state
                                        let subtitle = if let Some(ref dv) = daemon_version {
                                            format!("Running v{}", dv)
                                        } else if now_installed {
                                            "Installed but not running".to_string()
                                        } else {
                                            "Not installed".to_string()
                                        };
                                        service_row_for_inner.set_subtitle(&subtitle);

                                        if now_installed {
                                            btn_for_inner.set_label("Uninstall");
                                            btn_for_inner.remove_css_class("suggested-action");
                                            btn_for_inner.add_css_class("destructive-action");
                                            apply_row_for_inner.set_visible(true);
                                            apply_row_for_inner.set_sensitive(true);
                                            
                                            // Notify that daemon was just installed - trigger sensor refresh
                                            // The runtime sensor worker will now be able to get data
                                            info!("Daemon installed successfully, sensor data should now be available");
                                        } else {
                                            btn_for_inner.set_label("Install");
                                            btn_for_inner.remove_css_class("destructive-action");
                                            btn_for_inner.add_css_class("suggested-action");
                                            apply_row_for_inner.set_visible(false);
                                            apply_row_for_inner.set_active(false);
                                            apply_row_for_inner.set_sensitive(false);
                                            // Save the disabled state
                                            if let Err(e) = hf_core::update_setting(|s| {
                                                s.general.apply_curves_on_startup = false;
                                            }) {
                                                error!(
                                                    "Failed to disable apply_curves_on_startup: {}",
                                                    e
                                                );
                                            }
                                        }

                                        // Re-enable button
                                        btn_for_inner.set_sensitive(true);
                                        return glib::ControlFlow::Break;
                                    }

                                    glib::ControlFlow::Continue
                                });
                            }
                            Err(e) => {
                                error!("Service operation failed: {}", e);
                                // Re-enable button on error
                                btn_clone.set_sensitive(true);
                                btn_clone.set_label(if is_installed { "Uninstall" } else { "Install" });
                            }
                        }

                        glib::ControlFlow::Break
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        error!("Service operation failed: worker disconnected");
                        btn_clone.set_sensitive(true);
                        btn_clone.set_label(if is_installed { "Uninstall" } else { "Install" });
                        glib::ControlFlow::Break
                    }
                }
            });
        });

        // Poll interval
        let poll_row = adw::ComboRow::builder()
            .title("Sensor Poll Interval")
            .subtitle("How often to read temperature sensors")
            .build();

        let poll_options = gtk4::StringList::new(&["50ms (High CPU)", "100ms (Recommended)", "200ms", "500ms", "1000ms (Low CPU)"]);
        poll_row.set_model(Some(&poll_options));
        
        // Map current value to index with validation
        let poll_idx = match settings.general.poll_interval_ms {
            50 => 0,
            100 => 1,
            200 => 2,
            500 => 3,
            1000 => 4,
            _ => {
                warn!("Invalid poll_interval_ms: {}, defaulting to 100ms", settings.general.poll_interval_ms);
                1
            }
        };
        poll_row.set_selected(poll_idx);
        
        let pending_for_poll = pending_settings.clone();
        let dirty_for_poll = is_dirty.clone();
        let apply_btn_for_poll = apply_btn.clone();
        poll_row.connect_selected_notify(move |row| {
            let ms = match row.selected() {
                0 => 50,
                1 => 100,
                2 => 200,
                3 => 500,
                4 => 1000,
                _ => {
                    warn!("Invalid poll interval selection, defaulting to 100ms");
                    100
                }
            };
            // Validate range (50ms - 1000ms)
            if ms < 50 || ms > 1000 {
                error!("Poll interval out of valid range: {}ms", ms);
                return;
            }
            pending_for_poll.borrow_mut().general.poll_interval_ms = ms;
            *dirty_for_poll.borrow_mut() = true;
            apply_btn_for_poll.set_sensitive(true);
        });
        general_group.add(&poll_row);

        // Default page on startup
        let default_page_row = adw::ComboRow::builder()
            .title("Default Page")
            .subtitle("Page to show when application starts")
            .build();

        let page_options = gtk4::StringList::new(&[
            "Dashboard",
            "Fan Curves",
            "Fan Pairing",
            "Sensors",
            "Graphs",
        ]);
        default_page_row.set_model(Some(&page_options));
        
        // Map current value to index
        let page_idx = match settings.general.default_page.as_str() {
            "dashboard" => 0,
            "curves" => 1,
            "fan_pairing" => 2,
            "sensors" => 3,
            "graphs" => 4,
            _ => 0,
        };
        default_page_row.set_selected(page_idx);
        
        let pending_for_page = pending_settings.clone();
        let dirty_for_page = is_dirty.clone();
        let apply_btn_for_page = apply_btn.clone();
        default_page_row.connect_selected_notify(move |row| {
            let page = match row.selected() {
                0 => "dashboard",
                1 => "curves",
                2 => "fan_pairing",
                3 => "sensors",
                4 => "graphs",
                _ => "dashboard",
            };
            pending_for_page.borrow_mut().general.default_page = page.to_string();
            *dirty_for_page.borrow_mut() = true;
            apply_btn_for_page.set_sensitive(true);
        });
        general_group.add(&default_page_row);

        // Rate limit setting (1500-9999 requests per 10s window)
        let rate_limit_row = adw::SpinRow::builder()
            .title("Request Rate Limit")
            .subtitle("Max requests per 10 seconds (applies to client and daemon)")
            .adjustment(&gtk4::Adjustment::new(
                settings.general.rate_limit as f64,
                hf_core::MIN_RATE_LIMIT as f64,
                hf_core::MAX_RATE_LIMIT as f64,
                100.0,  // step increment
                500.0,  // page increment
                0.0,    // page size (unused for spin)
            ))
            .climb_rate(100.0)
            .digits(0)
            .numeric(true)
            .build();
        
        let pending_for_rate = pending_settings.clone();
        let dirty_for_rate = is_dirty.clone();
        let apply_btn_for_rate = apply_btn.clone();
        rate_limit_row.connect_value_notify(move |row| {
            let limit = row.value() as u32;
            let clamped = limit.clamp(hf_core::MIN_RATE_LIMIT, hf_core::MAX_RATE_LIMIT);
            pending_for_rate.borrow_mut().general.rate_limit = clamped;
            *dirty_for_rate.borrow_mut() = true;
            apply_btn_for_rate.set_sensitive(true);
        });
        general_group.add(&rate_limit_row);

        content.append(&general_group);

        // ================================================================
        // Display Section
        // ================================================================
        let display_group = adw::PreferencesGroup::builder()
            .title("Display")
            .build();

        // Temperature unit
        let unit_row = adw::ComboRow::builder()
            .title("Temperature Unit")
            .subtitle("Unit for displaying temperatures")
            .build();

        let unit_options = gtk4::StringList::new(&["Celsius (°C)", "Fahrenheit (°F)"]);
        unit_row.set_model(Some(&unit_options));
        unit_row.set_selected(if settings.display.temperature_unit == "fahrenheit" { 1 } else { 0 });
        
        let pending_for_unit = pending_settings.clone();
        let dirty_for_unit = is_dirty.clone();
        let apply_btn_for_unit = apply_btn.clone();
        unit_row.connect_selected_notify(move |row| {
            let unit = if row.selected() == 1 { "fahrenheit" } else { "celsius" };
            pending_for_unit.borrow_mut().display.temperature_unit = unit.to_string();
            *dirty_for_unit.borrow_mut() = true;
            apply_btn_for_unit.set_sensitive(true);
        });
        display_group.add(&unit_row);

        // Fan control metric
        let fan_metric_row = adw::ComboRow::builder()
            .title("Fan Control Metric")
            .subtitle("Unit for displaying fan speeds")
            .build();

        let fan_metric_options = gtk4::StringList::new(&["Percentage (%)", "PWM (0-255)"]);
        fan_metric_row.set_model(Some(&fan_metric_options));
        fan_metric_row.set_selected(if settings.display.fan_control_metric == "pwm" { 1 } else { 0 });
        
        let pending_for_metric = pending_settings.clone();
        let dirty_for_metric = is_dirty.clone();
        let apply_btn_for_metric = apply_btn.clone();
        fan_metric_row.connect_selected_notify(move |row| {
            let metric = if row.selected() == 1 { "pwm" } else { "percent" };
            pending_for_metric.borrow_mut().display.fan_control_metric = metric.to_string();
            *dirty_for_metric.borrow_mut() = true;
            apply_btn_for_metric.set_sensitive(true);
        });
        display_group.add(&fan_metric_row);

        // Show in system tray
        let tray_row = adw::SwitchRow::builder()
            .title("System Tray Icon")
            .subtitle("Show icon in system tray when running")
            .build();
        tray_row.set_active(crate::tray::is_tray_running() || settings.display.show_tray_icon);
        let pending_for_tray = pending_settings.clone();
        let dirty_for_tray = is_dirty.clone();
        let apply_btn_for_tray = apply_btn.clone();
        tray_row.connect_active_notify(move |row| {
            let enabled = row.is_active();
            
            // Actually start/stop the tray icon immediately (this is a live action)
            if enabled {
                crate::tray::start_tray();
            } else {
                crate::tray::stop_tray();
            }
            
            pending_for_tray.borrow_mut().display.show_tray_icon = enabled;
            *dirty_for_tray.borrow_mut() = true;
            apply_btn_for_tray.set_sensitive(true);
        });
        display_group.add(&tray_row);

        // Graph style
        let graph_row = adw::ComboRow::builder()
            .title("Graph Style")
            .subtitle("Visual style for temperature graphs")
            .build();

        let graph_options = gtk4::StringList::new(&["Line", "Filled", "Stepped"]);
        graph_row.set_model(Some(&graph_options));
        
        let graph_idx = match settings.display.graph_style.as_str() {
            "line" => 0,
            "filled" => 1,
            "stepped" => 2,
            _ => 1,
        };
        graph_row.set_selected(graph_idx);
        
        let pending_for_graph = pending_settings.clone();
        let dirty_for_graph = is_dirty.clone();
        let apply_btn_for_graph = apply_btn.clone();
        graph_row.connect_selected_notify(move |row| {
            let style = match row.selected() {
                0 => "line",
                1 => "filled",
                2 => "stepped",
                _ => "filled",
            };
            pending_for_graph.borrow_mut().display.graph_style = style.to_string();
            *dirty_for_graph.borrow_mut() = true;
            apply_btn_for_graph.set_sensitive(true);
        });
        display_group.add(&graph_row);

        // Graph smoothing (Direct/Smoothed)
        let smoothing_row = adw::ComboRow::builder()
            .title("Graph Smoothing")
            .subtitle("How lines are drawn between data points")
            .build();

        let smoothing_options = gtk4::StringList::new(&["Direct", "Smoothed"]);
        smoothing_row.set_model(Some(&smoothing_options));
        
        let smoothing_idx = match settings.display.graph_smoothing.as_str() {
            "direct" => 0,
            "smoothed" => 1,
            _ => 0,
        };
        smoothing_row.set_selected(smoothing_idx);
        
        let pending_for_smoothing = pending_settings.clone();
        let dirty_for_smoothing = is_dirty.clone();
        let apply_btn_for_smoothing = apply_btn.clone();
        smoothing_row.connect_selected_notify(move |row| {
            let smoothing = match row.selected() {
                0 => "direct",
                1 => "smoothed",
                _ => "direct",
            };
            pending_for_smoothing.borrow_mut().display.graph_smoothing = smoothing.to_string();
            *dirty_for_smoothing.borrow_mut() = true;
            apply_btn_for_smoothing.set_sensitive(true);
        });
        display_group.add(&smoothing_row);

        // Frame rate slider
        let frame_rate_row = adw::ActionRow::builder()
            .title("Animation Frame Rate")
            .subtitle("Higher rates = smoother animations, more CPU usage")
            .build();

        let frame_rate_box = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .valign(gtk4::Align::Center)
            .build();

        let frame_rate_label = Label::builder()
            .label(&Self::format_frame_rate(settings.display.frame_rate))
            .width_chars(12)
            .halign(gtk4::Align::End)
            .css_classes(["dim-label"])
            .build();

        // Scale with marks at: 24, 30, 60, 90, 120, 0 (native)
        // Map to positions: 0=24, 1=30, 2=60, 3=90, 4=120, 5=native(0)
        let frame_rate_scale = gtk4::Scale::builder()
            .orientation(Orientation::Horizontal)
            .draw_value(false)
            .hexpand(true)
            .width_request(200)
            .build();
        
        frame_rate_scale.set_range(0.0, 5.0);
        frame_rate_scale.set_increments(1.0, 1.0);
        frame_rate_scale.set_round_digits(0);
        
        // Add marks
        frame_rate_scale.add_mark(0.0, gtk4::PositionType::Bottom, Some("24"));
        frame_rate_scale.add_mark(1.0, gtk4::PositionType::Bottom, Some("30"));
        frame_rate_scale.add_mark(2.0, gtk4::PositionType::Bottom, Some("60"));
        frame_rate_scale.add_mark(3.0, gtk4::PositionType::Bottom, Some("90"));
        frame_rate_scale.add_mark(4.0, gtk4::PositionType::Bottom, Some("120"));
        frame_rate_scale.add_mark(5.0, gtk4::PositionType::Bottom, Some("Native"));
        
        // Set initial value based on current setting
        let initial_pos = match settings.display.frame_rate {
            24 => 0.0,
            30 => 1.0,
            60 => 2.0,
            90 => 3.0,
            120 => 4.0,
            0 => 5.0, // Native
            _ => 2.0, // Default to 60
        };
        frame_rate_scale.set_value(initial_pos);

        frame_rate_box.append(&frame_rate_scale);
        frame_rate_box.append(&frame_rate_label);
        frame_rate_row.add_suffix(&frame_rate_box);

        let pending_for_fps = pending_settings.clone();
        let dirty_for_fps = is_dirty.clone();
        let apply_btn_for_fps = apply_btn.clone();
        let label_for_fps = frame_rate_label.clone();
        
        frame_rate_scale.connect_value_changed(move |scale| {
            let pos = scale.value().round() as u32;
            let fps = match pos {
                0 => 24,
                1 => 30,
                2 => 60,
                3 => 90,
                4 => 120,
                5 => 0, // Native
                _ => 60,
            };
            
            label_for_fps.set_label(&Self::format_frame_rate(fps));
            pending_for_fps.borrow_mut().display.frame_rate = fps;
            *dirty_for_fps.borrow_mut() = true;
            apply_btn_for_fps.set_sensitive(true);
        });
        display_group.add(&frame_rate_row);

        // Color scheme (Dark/Light/System)
        let color_row = adw::ComboRow::builder()
            .title("Color Scheme")
            .subtitle("Application color theme")
            .build();

        let color_options = gtk4::StringList::new(&["Follow System", "Light", "Dark"]);
        color_row.set_model(Some(&color_options));
        
        let color_idx = match settings.display.color_scheme.as_str() {
            "system" => 0,
            "light" => 1,
            "dark" => 2,
            _ => 0,
        };
        color_row.set_selected(color_idx);
        
        let pending_for_color = pending_settings.clone();
        let dirty_for_color = is_dirty.clone();
        let apply_btn_for_color = apply_btn.clone();
        color_row.connect_selected_notify(move |row| {
            let scheme = match row.selected() {
                0 => "system",
                1 => "light",
                2 => "dark",
                _ => "system",
            };
            
            // Apply theme immediately (live preview)
            Self::apply_color_scheme(scheme);
            
            pending_for_color.borrow_mut().display.color_scheme = scheme.to_string();
            *dirty_for_color.borrow_mut() = true;
            apply_btn_for_color.set_sensitive(true);
        });
        display_group.add(&color_row);

        // Display backend (Wayland/X11)
        let backend_row = adw::ComboRow::builder()
            .title("Display Backend")
            .subtitle("Requires restart to take effect")
            .build();

        // Detect current backend for the "Auto" label
        let current_backend = Self::detect_current_backend();
        let auto_label = format!("Auto ({})", current_backend);
        
        let backend_options = gtk4::StringList::new(&[&auto_label, "Wayland", "X11"]);
        backend_row.set_model(Some(&backend_options));
        
        let backend_idx = match settings.display.display_backend.as_str() {
            "auto" => 0,
            "wayland" => 1,
            "x11" => 2,
            _ => 0,
        };
        backend_row.set_selected(backend_idx);
        
        let pending_for_backend = pending_settings.clone();
        let dirty_for_backend = is_dirty.clone();
        let apply_btn_for_backend = apply_btn.clone();
        backend_row.connect_selected_notify(move |row| {
            let backend = match row.selected() {
                0 => "auto",
                1 => "wayland",
                2 => "x11",
                _ => "auto",
            };
            pending_for_backend.borrow_mut().display.display_backend = backend.to_string();
            *dirty_for_backend.borrow_mut() = true;
            apply_btn_for_backend.set_sensitive(true);
        });
        display_group.add(&backend_row);

        // Window Manager / Desktop Environment
        let wm_row = adw::ComboRow::builder()
            .title("Window Manager")
            .subtitle("Changing this will restart Hyperfan")
            .build();

        // Detect current WM for the "Auto" label
        let detected_wm = hf_core::detect_desktop_environment();
        let auto_wm_label = format!("Auto ({})", detected_wm);
        
        let wm_options = gtk4::StringList::new(&[&auto_wm_label, "GNOME", "KDE"]);
        wm_row.set_model(Some(&wm_options));
        
        let wm_idx = match settings.display.window_manager.as_str() {
            "auto" => 0,
            "gnome" => 1,
            "kde" => 2,
            _ => 0,
        };
        wm_row.set_selected(wm_idx);
        
        let pending_for_wm = pending_settings.clone();
        let dirty_for_wm = is_dirty.clone();
        let apply_btn_for_wm = apply_btn.clone();
        wm_row.connect_selected_notify(move |row| {
            let wm = match row.selected() {
                0 => "auto",
                1 => "gnome",
                2 => "kde",
                _ => "auto",
            };
            pending_for_wm.borrow_mut().display.window_manager = wm.to_string();
            *dirty_for_wm.borrow_mut() = true;
            apply_btn_for_wm.set_sensitive(true);
        });
        display_group.add(&wm_row);

        content.append(&display_group);

        // ================================================================
        // Export/Import Section
        // ================================================================
        let export_group = adw::PreferencesGroup::builder()
            .title("Export and Import")
            .description("Export all settings, fan curves, and pairs to a file")
            .build();

        // Export button row
        let export_row = adw::ActionRow::builder()
            .title("Export All Settings")
            .subtitle("Save all configuration to a JSON file")
            .activatable(true)
            .build();

        let export_btn = Button::builder()
            .icon_name("document-save-symbolic")
            .valign(gtk4::Align::Center)
            .tooltip_text("Export settings")
            .css_classes(["flat"])
            .build();

        export_row.add_suffix(&export_btn);
        export_row.set_activatable_widget(Some(&export_btn));

        export_btn.connect_clicked(|btn| {
            Self::show_export_dialog(btn);
        });

        export_group.add(&export_row);

        // Import button row
        let import_row = adw::ActionRow::builder()
            .title("Import Settings")
            .subtitle("Load configuration from a JSON file")
            .activatable(true)
            .build();

        let import_btn = Button::builder()
            .icon_name("document-open-symbolic")
            .valign(gtk4::Align::Center)
            .tooltip_text("Import settings")
            .css_classes(["flat"])
            .build();

        import_row.add_suffix(&import_btn);
        import_row.set_activatable_widget(Some(&import_btn));

        import_btn.connect_clicked(|btn| {
            Self::show_import_dialog(btn);
        });

        export_group.add(&import_row);

        content.append(&export_group);

        // ================================================================
        // Advanced Section (DANGEROUS)
        // ================================================================
        let advanced_group = adw::PreferencesGroup::builder()
            .title("Advanced")
            .description("Dangerous features - use with extreme caution")
            .build();

        // EC Direct Control toggle
        let ec_row = adw::SwitchRow::builder()
            .title("Enable EC Direct Control")
            .subtitle("EXTREMELY DANGEROUS - Can permanently damage hardware")
            .build();
        ec_row.set_active(settings.advanced.ec_direct_control_enabled && settings.advanced.ec_danger_acknowledged);

        let ec_row_clone = ec_row.clone();
        ec_row.connect_active_notify(move |row| {
            let enabled = row.is_active();
            
            if enabled {
                // Show danger warning dialog
                Self::show_ec_danger_warning(&ec_row_clone);
            } else {
                // Disable EC control
                if let Err(e) = hf_core::update_setting(|s| {
                    s.advanced.ec_direct_control_enabled = false;
                    s.advanced.ec_danger_acknowledged = false;
                    s.advanced.ec_enabled_at = None;
                }) {
                    error!("Failed to save EC setting: {}", e);
                } else {
                    info!("EC direct control disabled");
                }
            }
        });

        advanced_group.add(&ec_row);
        content.append(&advanced_group);

        // ================================================================
        // Configuration Section
        // ================================================================
        let config_group = adw::PreferencesGroup::builder()
            .title("Configuration")
            .build();

        // Settings path - clickable to open file explorer
        let settings_path = hf_core::get_settings_path().ok();
        let path_str = settings_path.as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unknown".to_string());
        
        let path_row = adw::ActionRow::builder()
            .title("Settings File")
            .subtitle(&path_str)
            .activatable(true)
            .build();
        
        // Add folder icon button
        let open_folder_btn = Button::builder()
            .icon_name("folder-open-symbolic")
            .valign(gtk4::Align::Center)
            .tooltip_text("Open in file manager")
            .css_classes(["flat"])
            .build();
        
        path_row.add_suffix(&open_folder_btn);
        path_row.set_activatable_widget(Some(&open_folder_btn));
        
        // Open file manager to settings directory
        if let Some(path) = settings_path {
            let parent_dir = path.parent().map(|p| p.to_path_buf());
            open_folder_btn.connect_clicked(move |_| {
                if let Some(ref dir) = parent_dir {
                    if let Err(e) = open::that(dir) {
                        error!("Failed to open file manager: {}", e);
                    }
                }
            });
        }
        
        config_group.add(&path_row);

        content.append(&config_group);

        scroll.set_child(Some(&content));
        container.append(&scroll);

        // Apply button click handler - saves all pending settings
        let pending_for_save = pending_settings.clone();
        let dirty_for_save = is_dirty.clone();
        let apply_btn_for_save = apply_btn.clone();
        apply_btn.connect_clicked(move |btn| {
            // Show loading state
            btn.set_sensitive(false);
            btn.set_label("Applying...");
            let settings_to_save = pending_for_save.borrow().clone();
            
            // Check if window_manager changed - need to restart
            let current_wm = hf_core::load_settings()
                .map(|s| s.display.window_manager)
                .unwrap_or_else(|_| "auto".to_string());
            let new_wm = settings_to_save.display.window_manager.clone();
            let wm_changed = current_wm != new_wm;

            // Check if display_backend changed - need to restart (GDK_BACKEND read at startup)
            let current_backend = hf_core::load_settings()
                .map(|s| s.display.display_backend)
                .unwrap_or_else(|_| "auto".to_string());
            let new_backend = settings_to_save.display.display_backend.clone();
            let backend_changed = current_backend != new_backend;
            
            // Check if graph_style changed - need to reload daemon for stepped mode
            let current_graph_style = hf_core::load_settings()
                .map(|s| s.display.graph_style)
                .unwrap_or_else(|_| "filled".to_string());
            let new_graph_style = settings_to_save.display.graph_style.clone();
            let graph_style_changed = current_graph_style != new_graph_style;
            
            // Save all settings at once
            if let Err(e) = hf_core::save_settings(&settings_to_save) {
                error!("Failed to save settings: {}", e);
                btn.set_label("Apply");
                btn.set_sensitive(true);
                
                // Show error toast
                if let Some(window) = btn.root().and_then(|r| r.downcast::<gtk4::Window>().ok()) {
                    let toast = adw::Toast::new(&format!("Failed to save settings: {}", e));
                    toast.set_timeout(3);
                    if let Some(toast_overlay) = window.child()
                        .and_then(|c| c.downcast::<adw::ToastOverlay>().ok())
                    {
                        toast_overlay.add_toast(toast);
                    }
                }
                return;
            }
            
            // Apply rate limit immediately to both client and daemon
            let rate_limit = settings_to_save.general.rate_limit;
            match hf_core::set_rate_limits(rate_limit) {
                Ok((client_limit, daemon_limit)) => {
                    info!("Rate limit applied: client={}, daemon={}", client_limit, daemon_limit);
                }
                Err(e) => {
                    // Client-side is already set, daemon might not be running
                    hf_core::set_client_rate_limit(rate_limit);
                    warn!("Could not set daemon rate limit (daemon may not be running): {}", e);
                }
            }
                
            // Restart if window manager or display backend changed
            if wm_changed || backend_changed {
                if wm_changed {
                    info!("Window manager changed to {}, restarting...", new_wm);
                }
                if backend_changed {
                    info!("Display backend changed to {}, restarting...", new_backend);
                }
                Self::restart_application();
            }
            
            // Reload daemon if graph_style changed (affects stepped fan control mode)
            if graph_style_changed {
                info!("Graph style changed to {}, reloading daemon config...", new_graph_style);
                if let Err(e) = hf_core::daemon_reload_config() {
                    warn!("Could not reload daemon config (daemon may not be running): {}", e);
                }
            }
            
            // Mark as clean and reset button
            *dirty_for_save.borrow_mut() = false;
            apply_btn_for_save.set_sensitive(false);
            apply_btn_for_save.set_label("Apply");
        });

        Self { 
            container, 
            pending_settings, 
            is_dirty, 
            apply_btn,
            daemon_service_row,
            on_daemon_installed: Rc::new(RefCell::new(None)),
        }
    }
    
    /// Set callback to be called when daemon is installed
    pub fn connect_daemon_installed<F: Fn() + 'static>(&self, callback: F) {
        *self.on_daemon_installed.borrow_mut() = Some(Box::new(callback));
    }

    pub fn widget(&self) -> &GtkBox {
        &self.container
    }

    /// Check if there are unsaved changes
    pub fn has_unsaved_changes(&self) -> bool {
        *self.is_dirty.borrow()
    }

    /// Reset dirty state (e.g., after discarding changes)
    pub fn reset_dirty(&self) {
        *self.is_dirty.borrow_mut() = false;
        self.apply_btn.set_sensitive(false);
    }

    /// Apply pending settings (save to disk)
    pub fn apply_settings(&self) {
        let settings_to_save = self.pending_settings.borrow().clone();
        
        // Check if window_manager changed - need to restart
        let current_wm = hf_core::load_settings()
            .map(|s| s.display.window_manager)
            .unwrap_or_else(|_| "auto".to_string());
        let new_wm = settings_to_save.display.window_manager.clone();
        let wm_changed = current_wm != new_wm;

        // Check if display_backend changed - need to restart (GDK_BACKEND read at startup)
        let current_backend = hf_core::load_settings()
            .map(|s| s.display.display_backend)
            .unwrap_or_else(|_| "auto".to_string());
        let new_backend = settings_to_save.display.display_backend.clone();
        let backend_changed = current_backend != new_backend;
        
        // Save all settings at once
        if let Err(e) = hf_core::save_settings(&settings_to_save) {
            error!("Failed to save settings: {}", e);
        } else {
            info!("Settings saved successfully");
            *self.is_dirty.borrow_mut() = false;
            self.apply_btn.set_sensitive(false);
            
            // Signal daemon to reload config
            if let Err(e) = hf_core::daemon_reload_config() {
                debug!("Failed to signal daemon reload: {}", e);
            }
            
            // Restart if window manager or display backend changed
            if wm_changed || backend_changed {
                if wm_changed {
                    info!("Window manager changed to {}, restarting...", new_wm);
                }
                if backend_changed {
                    info!("Display backend changed to {}, restarting...", new_backend);
                }
                Self::restart_application();
            }
        }
    }

    /// Flash the daemon install card with ease-in-out animation (3 cycles)
    pub fn flash_daemon_install_card(&self) {
        if let Some(ref row) = *self.daemon_service_row.borrow() {
            let row_clone = row.clone();
            
            // Animation parameters
            let duration_ms = 400;
            let cycles = 3;
            
            // Start animation sequence
            Self::animate_flash_cycle(row_clone, 0, cycles, duration_ms);
        }
    }
    
    /// Recursive function to animate flash cycles
    fn animate_flash_cycle(row: adw::ActionRow, current_cycle: u32, total_cycles: u32, duration_ms: u64) {
        if current_cycle >= total_cycles {
            return;
        }
        
        // Ease-in-out animation using opacity
        let steps = 20; // Number of steps for smooth animation
        let step_duration = duration_ms / steps;
        
        // Animate to highlighted state (ease-in)
        Self::animate_opacity_steps(row.clone(), 0, (steps / 2) as u32, step_duration, true, move |row_inner| {
            // Then animate back to normal (ease-out)
            Self::animate_opacity_steps(row_inner.clone(), 0, (steps / 2) as u32, step_duration, false, move |row_final| {
                // Continue to next cycle
                Self::animate_flash_cycle(row_final, current_cycle + 1, total_cycles, duration_ms);
            });
        });
    }
    
    /// Animate opacity steps with ease-in-out
    fn animate_opacity_steps<F>(row: adw::ActionRow, current_step: u32, total_steps: u32, step_duration: u64, fade_in: bool, on_complete: F)
    where
        F: Fn(adw::ActionRow) + 'static,
    {
        if current_step >= total_steps {
            on_complete(row);
            return;
        }
        
        // Calculate progress with ease-in-out
        let progress = current_step as f64 / total_steps as f64;
        let eased = if fade_in {
            // Ease-in: slow start, fast end
            progress * progress
        } else {
            // Ease-out: fast start, slow end
            1.0 - (1.0 - progress) * (1.0 - progress)
        };
        
        // Apply highlight using CSS class and opacity
        let opacity = if fade_in {
            0.3 + (0.7 * eased) // Fade from 30% to 100%
        } else {
            1.0 - (0.7 * eased) // Fade from 100% to 30%
        };
        
        row.set_opacity(opacity);
        
        // Add accent color highlight at peak
        if fade_in && current_step == total_steps - 1 {
            row.add_css_class("accent");
        } else if !fade_in && current_step == 0 {
            row.remove_css_class("accent");
        }
        
        // Schedule next step
        let row_clone = row.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(step_duration), move || {
            Self::animate_opacity_steps(row_clone, current_step + 1, total_steps, step_duration, fade_in, on_complete);
        });
    }

    /// Format frame rate value for display
    fn format_frame_rate(fps: u32) -> String {
        if fps == 0 {
            "Native".to_string()
        } else {
            format!("{} FPS", fps)
        }
    }

    /// Detect which display backend is currently in use
    fn detect_current_backend() -> &'static str {
        if std::env::var("WAYLAND_DISPLAY").is_ok() {
            "Wayland"
        } else if std::env::var("DISPLAY").is_ok() {
            "X11"
        } else {
            "Unknown"
        }
    }

    /// Apply color scheme to the application
    /// Uses libadwaita's StyleManager for proper HDR and accessibility support
    pub fn apply_color_scheme(scheme: &str) {
        let style_manager = libadwaita::StyleManager::default();
        
        let color_scheme = match scheme {
            "light" => libadwaita::ColorScheme::ForceLight,
            "dark" => libadwaita::ColorScheme::ForceDark,
            // PreferLight = use light UNLESS system prefers dark (true system following)
            _ => libadwaita::ColorScheme::PreferLight,
        };
        
        style_manager.set_color_scheme(color_scheme);
        debug!("Applied color scheme: {}", scheme);
    }

    fn show_export_dialog(btn: &Button) {
        let window = btn.root().and_downcast::<gtk4::Window>();
        
        let dialog = gtk4::FileDialog::builder()
            .title("Export Hyperfan Settings")
            .initial_name("hyperfan-export.json")
            .build();

        let filter = gtk4::FileFilter::new();
        filter.add_pattern("*.json");
        filter.set_name(Some("JSON files"));
        
        let filters = gio::ListStore::new::<gtk4::FileFilter>();
        filters.append(&filter);
        dialog.set_filters(Some(&filters));

        dialog.save(window.as_ref(), gio::Cancellable::NONE, move |result| {
            if let Ok(file) = result {
                if let Some(path) = file.path() {
                    match Self::export_all_settings(&path) {
                        Ok(_) => info!("Exported settings to {:?}", path),
                        Err(e) => error!("Failed to export settings: {}", e),
                    }
                }
            }
        });
    }

    fn show_import_dialog(btn: &Button) {
        let window = btn.root().and_downcast::<gtk4::Window>();
        
        let dialog = gtk4::FileDialog::builder()
            .title("Import Hyperfan Settings")
            .build();

        let filter = gtk4::FileFilter::new();
        filter.add_pattern("*.json");
        filter.set_name(Some("JSON files"));
        
        let filters = gio::ListStore::new::<gtk4::FileFilter>();
        filters.append(&filter);
        dialog.set_filters(Some(&filters));

        dialog.open(window.as_ref(), gio::Cancellable::NONE, move |result| {
            if let Ok(file) = result {
                if let Some(path) = file.path() {
                    match Self::import_all_settings(&path) {
                        Ok(_) => info!("Imported settings from {:?}", path),
                        Err(e) => error!("Failed to import settings: {}", e),
                    }
                }
            }
        });
    }

    fn export_all_settings(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        // Copy settings file
        let settings_path = hf_core::get_settings_path()?;
        let curves_path = hf_core::get_curves_path();
        
        // Read both files and combine into export
        let settings_json = if settings_path.exists() {
            std::fs::read_to_string(&settings_path)?
        } else {
            "{}".to_string()
        };
        
        let curves_json = if curves_path.exists() {
            std::fs::read_to_string(&curves_path)?
        } else {
            "{}".to_string()
        };
        
        // Create combined export JSON
        let export = format!(
            r#"{{
  "version": "{}",
  "exported_at": {},
  "settings": {},
  "curves": {}
}}"#,
            env!("CARGO_PKG_VERSION"),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            settings_json.trim(),
            curves_json.trim()
        );

        std::fs::write(path, export)?;
        Ok(())
    }

    fn import_all_settings(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        // ================================================================
        // STRICT INPUT VALIDATION
        // ================================================================
        
        // Check file size (max 1MB to prevent DoS)
        let metadata = std::fs::metadata(path)?;
        if metadata.len() > 1_048_576 {
            return Err("Import file too large (max 1MB)".into());
        }
        
        let content = std::fs::read_to_string(path)?;
        
        // Validate JSON structure
        let export: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| format!("Invalid JSON: {}", e))?;
        
        // Must be an object
        let export_obj = export.as_object()
            .ok_or("Import file must be a JSON object")?;
        
        // Validate top-level structure
        Self::validate_export_structure(export_obj)?;
        
        // Extract and validate settings if present
        if let Some(settings_value) = export.get("settings") {
            // Validate settings structure strictly
            Self::validate_settings_structure(settings_value)?;
            
            let settings_path = hf_core::get_settings_path()?;
            if let Some(parent) = settings_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let settings_json = serde_json::to_string_pretty(settings_value)?;
            std::fs::write(&settings_path, settings_json)?;
            info!("Imported settings to {:?}", settings_path);
        }
        
        // Extract and validate curves if present
        if let Some(curves_value) = export.get("curves") {
            // Validate curves structure strictly
            Self::validate_curves_structure(curves_value)?;
            
            let curves_path = hf_core::get_curves_path();
            if let Some(parent) = curves_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let curves_json = serde_json::to_string_pretty(curves_value)?;
            std::fs::write(&curves_path, curves_json)?;
            info!("Imported curves to {:?}", curves_path);
        }
        
        info!("Import complete - restart application to apply all settings");
        Ok(())
    }
    
    /// Validate export file top-level structure
    fn validate_export_structure(obj: &serde_json::Map<String, serde_json::Value>) -> Result<(), Box<dyn std::error::Error>> {
        // Only allow known keys
        const ALLOWED_KEYS: &[&str] = &["version", "exported_at", "settings", "curves"];
        
        for key in obj.keys() {
            if !ALLOWED_KEYS.contains(&key.as_str()) {
                return Err(format!("Unknown key in export file: '{}'", key).into());
            }
        }
        
        // Validate version if present
        if let Some(version) = obj.get("version") {
            if !version.is_string() {
                return Err("'version' must be a string".into());
            }
        }
        
        // Validate exported_at if present
        if let Some(exported_at) = obj.get("exported_at") {
            if !exported_at.is_u64() && !exported_at.is_i64() {
                return Err("'exported_at' must be a timestamp number".into());
            }
        }
        
        Ok(())
    }
    
    /// Strictly validate settings structure
    fn validate_settings_structure(settings: &serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
        let obj = settings.as_object()
            .ok_or("'settings' must be a JSON object")?;
        
        // Validate 'general' section if present
        if let Some(general) = obj.get("general") {
            Self::validate_general_settings(general)?;
        }
        
        // Validate 'display' section if present
        if let Some(display) = obj.get("display") {
            Self::validate_display_settings(display)?;
        }
        
        // Validate 'active_pairs' if present
        if let Some(pairs) = obj.get("active_pairs") {
            Self::validate_active_pairs(pairs)?;
        }
        
        // Validate 'pwm_fan_mappings' if present
        if let Some(mappings) = obj.get("pwm_fan_mappings") {
            if !mappings.is_array() {
                return Err("'pwm_fan_mappings' must be an array".into());
            }
        }
        
        Ok(())
    }
    
    /// Validate general settings
    fn validate_general_settings(general: &serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
        let obj = general.as_object()
            .ok_or("'general' must be a JSON object")?;
        
        // Boolean fields
        for field in &["start_at_boot", "start_minimized", "apply_curves_on_startup"] {
            if let Some(val) = obj.get(*field) {
                if !val.is_boolean() {
                    return Err(format!("'general.{}' must be a boolean", field).into());
                }
            }
        }
        
        // poll_interval_ms must be a reasonable number
        if let Some(poll) = obj.get("poll_interval_ms") {
            let val = poll.as_u64()
                .ok_or("'general.poll_interval_ms' must be a positive number")?;
            if val < 10 || val > 10000 {
                return Err("'general.poll_interval_ms' must be between 10 and 10000".into());
            }
        }
        
        Ok(())
    }
    
    /// Validate display settings
    fn validate_display_settings(display: &serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
        let obj = display.as_object()
            .ok_or("'display' must be a JSON object")?;
        
        // Boolean fields
        if let Some(val) = obj.get("show_tray_icon") {
            if !val.is_boolean() {
                return Err("'display.show_tray_icon' must be a boolean".into());
            }
        }
        
        // Enum-like string fields with strict validation
        if let Some(val) = obj.get("temperature_unit") {
            let s = val.as_str().ok_or("'display.temperature_unit' must be a string")?;
            if !["celsius", "fahrenheit"].contains(&s) {
                return Err("'display.temperature_unit' must be 'celsius' or 'fahrenheit'".into());
            }
        }
        
        if let Some(val) = obj.get("graph_style") {
            let s = val.as_str().ok_or("'display.graph_style' must be a string")?;
            if !["line", "filled", "stepped"].contains(&s) {
                return Err("'display.graph_style' must be 'line', 'filled', or 'stepped'".into());
            }
        }
        
        if let Some(val) = obj.get("color_scheme") {
            let s = val.as_str().ok_or("'display.color_scheme' must be a string")?;
            if !["system", "light", "dark"].contains(&s) {
                return Err("'display.color_scheme' must be 'system', 'light', or 'dark'".into());
            }
        }
        
        if let Some(val) = obj.get("display_backend") {
            let s = val.as_str().ok_or("'display.display_backend' must be a string")?;
            if !["auto", "wayland", "x11"].contains(&s) {
                return Err("'display.display_backend' must be 'auto', 'wayland', or 'x11'".into());
            }
        }
        
        Ok(())
    }
    
    /// Validate active pairs array
    fn validate_active_pairs(pairs: &serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
        let arr = pairs.as_array()
            .ok_or("'active_pairs' must be an array")?;
        
        for (i, pair) in arr.iter().enumerate() {
            let obj = pair.as_object()
                .ok_or(format!("active_pairs[{}] must be an object", i))?;
            
            // Required string fields
            for field in &["id", "name", "curve_id", "temp_source_path", "fan_path"] {
                let val = obj.get(*field)
                    .ok_or(format!("active_pairs[{}].{} is required", i, field))?;
                let s = val.as_str()
                    .ok_or(format!("active_pairs[{}].{} must be a string", i, field))?;
                
                // Validate string length
                if s.is_empty() || s.len() > 1024 {
                    return Err(format!("active_pairs[{}].{} must be 1-1024 characters", i, field).into());
                }
                
                // Validate paths don't contain dangerous sequences
                if *field == "temp_source_path" || *field == "fan_path" {
                    Self::validate_path_string(s, &format!("active_pairs[{}].{}", i, field))?;
                }
            }
            
            // Optional boolean
            if let Some(active) = obj.get("active") {
                if !active.is_boolean() {
                    return Err(format!("active_pairs[{}].active must be a boolean", i).into());
                }
            }
        }
        
        Ok(())
    }
    
    /// Validate curves structure
    fn validate_curves_structure(curves: &serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
        let obj = curves.as_object()
            .ok_or("'curves' must be a JSON object")?;
        
        // Validate each curve
        for (name, curve) in obj {
            // Name validation
            if name.is_empty() || name.len() > 256 {
                return Err(format!("Curve name must be 1-256 characters: '{}'", name).into());
            }
            
            let curve_obj = curve.as_object()
                .ok_or(format!("Curve '{}' must be an object", name))?;
            
            // Must have points array
            let points = curve_obj.get("points")
                .ok_or(format!("Curve '{}' must have 'points'", name))?;
            
            let points_arr = points.as_array()
                .ok_or(format!("Curve '{}' points must be an array", name))?;
            
            // Validate each point
            for (i, point) in points_arr.iter().enumerate() {
                let arr = point.as_array()
                    .ok_or(format!("Curve '{}' point {} must be [temp, percent]", name, i))?;
                
                if arr.len() != 2 {
                    return Err(format!("Curve '{}' point {} must have exactly 2 values", name, i).into());
                }
                
                // Temperature: 0-150
                let temp = arr[0].as_f64()
                    .ok_or(format!("Curve '{}' point {} temperature must be a number", name, i))?;
                if temp < 0.0 || temp > 150.0 {
                    return Err(format!("Curve '{}' point {} temperature must be 0-150", name, i).into());
                }
                
                // Percent: 0-100
                let percent = arr[1].as_f64()
                    .ok_or(format!("Curve '{}' point {} percent must be a number", name, i))?;
                if percent < 0.0 || percent > 100.0 {
                    return Err(format!("Curve '{}' point {} percent must be 0-100", name, i).into());
                }
            }
        }
        
        Ok(())
    }
    
    /// Show full GPL-3.0+ license dialog
    fn show_license_dialog(btn: &Button) {
        let dialog = adw::Dialog::builder()
            .title("GNU General Public License v3.0")
            .content_width(700)
            .content_height(600)
            .build();

        let content = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .build();

        let header = adw::HeaderBar::builder()
            .show_end_title_buttons(true)
            .build();

        // Copy license button
        let license_text_static: &'static str = include_str!("../../../LICENSE.md");
        let copy_btn = Button::builder()
            .icon_name("edit-copy-symbolic")
            .tooltip_text("Copy license text")
            .build();
        copy_btn.connect_clicked(move |btn| {
            let clipboard = btn.clipboard();
            clipboard.set_text(license_text_static);
        });
        header.pack_start(&copy_btn);

        // FSF website button
        let fsf_btn = Button::builder()
            .icon_name("external-link-symbolic")
            .tooltip_text("Visit FSF website")
            .build();
        let dialog_weak = dialog.downgrade();
        fsf_btn.connect_clicked(move |_| {
            Self::show_fsf_warning(&dialog_weak);
        });
        header.pack_start(&fsf_btn);

        content.append(&header);

        // Scroll window flush to edges (no margins)
        let scroll = ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .build();

        // Inner box for padding on the text content only
        let text_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .margin_start(18)
            .margin_end(18)
            .margin_top(12)
            .margin_bottom(18)
            .build();

        let label = Label::builder()
            .label(license_text_static)
            .selectable(true)
            .wrap(true)
            .xalign(0.0)
            .css_classes(["monospace"])
            .build();

        // Prevent auto-selection on dialog open
        label.select_region(0, 0);

        text_box.append(&label);
        scroll.set_child(Some(&text_box));
        content.append(&scroll);

        dialog.set_child(Some(&content));
        dialog.present(Some(btn));
    }

    /// Show FSF website warning dialog
    fn show_fsf_warning(parent: &glib::WeakRef<adw::Dialog>) {
        let warning = adw::AlertDialog::builder()
            .heading("Open External Website")
            .body("You are about to be taken to the Free Software Foundation website:\n\nhttps://www.fsf.org\n\nDo you want to continue?")
            .build();

        warning.add_response("cancel", "Cancel");
        warning.add_response("open", "Open Website");
        warning.set_response_appearance("open", adw::ResponseAppearance::Suggested);
        warning.set_default_response(Some("cancel"));
        warning.set_close_response("cancel");

        warning.connect_response(None, |_, response| {
            if response == "open" {
                if let Err(e) = open::that("https://www.fsf.org") {
                    error!("Failed to open FSF website: {}", e);
                }
            }
        });

        if let Some(dialog) = parent.upgrade() {
            warning.present(Some(&dialog));
        }
    }

    /// Show FOSS dependencies dialog
    fn show_foss_dialog(btn: &Button) {
        let dialog = adw::Dialog::builder()
            .title("Open Source Dependencies")
            .content_width(700)
            .content_height(500)
            .build();

        let content = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .build();

        // Header bar with export menu
        let header = adw::HeaderBar::builder()
            .show_end_title_buttons(true)
            .build();

        // Export menu button (top left)
        let export_menu = gio::Menu::new();
        export_menu.append(Some("Export as CSV"), Some("foss.export-csv"));
        export_menu.append(Some("Export as JSON"), Some("foss.export-json"));

        let export_btn = gtk4::MenuButton::builder()
            .icon_name("document-save-symbolic")
            .tooltip_text("Export dependencies")
            .menu_model(&export_menu)
            .build();
        header.pack_start(&export_btn);

        // Create action group for export actions
        let action_group = gio::SimpleActionGroup::new();

        // CSV export action
        let csv_action = gio::SimpleAction::new("export-csv", None);
        csv_action.connect_activate(|_, _| {
            Self::export_foss_csv();
        });
        action_group.add_action(&csv_action);

        // JSON export action
        let json_action = gio::SimpleAction::new("export-json", None);
        json_action.connect_activate(|_, _| {
            Self::export_foss_json();
        });
        action_group.add_action(&json_action);

        content.insert_action_group("foss", Some(&action_group));
        content.append(&header);

        // Description
        let desc = Label::builder()
            .label("Hyperfan is built with these amazing open source projects. Click any row to visit the project page.")
            .wrap(true)
            .margin_start(18)
            .margin_end(18)
            .margin_top(12)
            .margin_bottom(12)
            .css_classes(["dim-label"])
            .build();
        content.append(&desc);

        // Scrollable list of dependencies
        let scroll = ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .build();

        let list = gtk4::ListBox::builder()
            .selection_mode(gtk4::SelectionMode::None)
            .css_classes(["boxed-list"])
            .margin_start(18)
            .margin_end(18)
            .margin_bottom(18)
            .build();

        // All dependencies with their info
        let dependencies = Self::get_foss_dependencies();

        for (name, license, author, url) in dependencies {
            let row = adw::ActionRow::builder()
                .title(name)
                .subtitle(&format!("{} • {}", license, author))
                .activatable(true)
                .build();

            let link_btn = Button::builder()
                .icon_name("external-link-symbolic")
                .valign(gtk4::Align::Center)
                .tooltip_text("Open project page")
                .css_classes(["flat"])
                .build();

            row.add_suffix(&link_btn);
            row.set_activatable_widget(Some(&link_btn));

            let url_owned = url.to_string();
            link_btn.connect_clicked(move |_| {
                if let Err(e) = open::that(&url_owned) {
                    error!("Failed to open URL: {}", e);
                }
            });

            list.append(&row);
        }

        scroll.set_child(Some(&list));
        content.append(&scroll);

        dialog.set_child(Some(&content));
        dialog.present(Some(btn));
    }

    /// Get FOSS dependencies data
    fn get_foss_dependencies() -> Vec<(&'static str, &'static str, &'static str, &'static str)> {
        vec![
            // (name, license, author, url)
            ("gtk4-rs", "MIT", "gtk-rs contributors", "https://github.com/gtk-rs/gtk4-rs"),
            ("libadwaita-rs", "MIT", "gtk-rs contributors", "https://github.com/gtk-rs/gtk4-rs"),
            ("tokio", "MIT", "Tokio Contributors", "https://github.com/tokio-rs/tokio"),
            ("serde", "MIT OR Apache-2.0", "Erick Tryzelaar, David Tolnay", "https://github.com/serde-rs/serde"),
            ("serde_json", "MIT OR Apache-2.0", "Erick Tryzelaar, David Tolnay", "https://github.com/serde-rs/json"),
            ("anyhow", "MIT OR Apache-2.0", "David Tolnay", "https://github.com/dtolnay/anyhow"),
            ("thiserror", "MIT OR Apache-2.0", "David Tolnay", "https://github.com/dtolnay/thiserror"),
            ("tracing", "MIT", "Tokio Contributors", "https://github.com/tokio-rs/tracing"),
            ("tracing-subscriber", "MIT", "Tokio Contributors", "https://github.com/tokio-rs/tracing"),
            ("tracing-journald", "MIT", "Tokio Contributors", "https://github.com/tokio-rs/tracing"),
            ("parking_lot", "MIT OR Apache-2.0", "Amanieu d'Antras", "https://github.com/Amanieu/parking_lot"),
            ("regex", "MIT OR Apache-2.0", "The Rust Project Developers", "https://github.com/rust-lang/regex"),
            ("dirs", "MIT OR Apache-2.0", "Simon Ochsenreither", "https://github.com/dirs-dev/dirs-rs"),
            ("libc", "MIT OR Apache-2.0", "The Rust Project Developers", "https://github.com/rust-lang/libc"),
            ("open", "MIT", "Byron Rakitzis", "https://github.com/Byron/open-rs"),
            ("ksni", "Apache-2.0", "ksni contributors", "https://github.com/ksni-rs/ksni"),
            ("ctrlc", "MIT OR Apache-2.0", "Antti Keränen", "https://github.com/Detegr/rust-ctrlc"),
            ("clap", "MIT OR Apache-2.0", "Kevin K., Ed Page", "https://github.com/clap-rs/clap"),
            ("uuid", "MIT OR Apache-2.0", "Ashley Mannix, Dylan DPC, Hunar Roop Kahlon", "https://github.com/uuid-rs/uuid"),
            ("chrono", "MIT OR Apache-2.0", "Kang Seonghoon, Brandon W Maister", "https://github.com/chronotope/chrono"),
            ("sha2", "MIT OR Apache-2.0", "RustCrypto Developers", "https://github.com/RustCrypto/hashes"),
        ]
    }

    /// Export FOSS dependencies as CSV
    fn export_foss_csv() {
        let deps = Self::get_foss_dependencies();
        let mut csv = String::from("Name,License,Author,URL\n");
        for (name, license, author, url) in deps {
            csv.push_str(&format!("\"{}\",\"{}\",\"{}\",\"{}\"\n", name, license, author, url));
        }

        if let Some(downloads) = dirs::download_dir() {
            let path = downloads.join("hyperfan-dependencies.csv");
            if let Err(e) = std::fs::write(&path, csv) {
                error!("Failed to export CSV: {}", e);
            } else {
                info!("Exported dependencies to {}", path.display());
                let _ = open::that(&path);
            }
        }
    }

    /// Export FOSS dependencies as JSON
    fn export_foss_json() {
        let deps = Self::get_foss_dependencies();
        let json_deps: Vec<serde_json::Value> = deps
            .iter()
            .map(|(name, license, author, url)| {
                serde_json::json!({
                    "name": name,
                    "license": license,
                    "author": author,
                    "url": url
                })
            })
            .collect();

        let json = serde_json::json!({
            "project": "Hyperfan",
            "dependencies": json_deps
        });

        if let Some(downloads) = dirs::download_dir() {
            let path = downloads.join("hyperfan-dependencies.json");
            if let Err(e) = std::fs::write(&path, serde_json::to_string_pretty(&json).unwrap_or_default()) {
                error!("Failed to export JSON: {}", e);
            } else {
                info!("Exported dependencies to {}", path.display());
                let _ = open::that(&path);
            }
        }
    }

    /// Show EC danger warning dialog
    fn show_ec_danger_warning(ec_row: &adw::SwitchRow) {
        let dialog = adw::AlertDialog::builder()
            .heading("EXTREME DANGER WARNING")
            .body(
                "You are about to enable DIRECT EC (Embedded Controller) ACCESS.\n\n\
                 This feature allows you to directly read and write to your system's \
                 Embedded Controller registers.\n\n\
                 RISKS INCLUDE:\n\
                 - PERMANENT HARDWARE DAMAGE\n\
                 - SYSTEM INSTABILITY OR CRASHES\n\
                 - BRICKING YOUR MOTHERBOARD\n\
                 - VOIDING YOUR WARRANTY\n\
                 - DATA LOSS\n\n\
                 This feature is intended ONLY for advanced users who understand \
                 their system's EC register layout.\n\n\
                 INCORRECT VALUES CAN DESTROY YOUR HARDWARE.\n\n\
                 The developers of Hyperfan accept NO RESPONSIBILITY for any \
                 damage caused by using this feature.\n\n\
                 Do you understand and accept these risks?"
            )
            .build();

        dialog.add_response("cancel", "Cancel");
        dialog.add_response("accept", "I UNDERSTAND THE RISKS");
        dialog.set_response_appearance("accept", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");

        let ec_row_for_response = ec_row.clone();
        dialog.connect_response(None, move |_dialog, response| {
            if response == "accept" {
                // User accepted - enable EC control
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);

                if let Err(e) = hf_core::update_setting(|s| {
                    s.advanced.ec_direct_control_enabled = true;
                    s.advanced.ec_danger_acknowledged = true;
                    s.advanced.ec_enabled_at = Some(now);
                }) {
                    error!("Failed to save EC setting: {}", e);
                    ec_row_for_response.set_active(false);
                } else {
                    warn!("EC direct control ENABLED by user at timestamp {}", now);
                }
            } else {
                // User cancelled - keep switch off
                ec_row_for_response.set_active(false);
            }
        });

        // Present dialog
        if let Some(root) = ec_row.root() {
            if let Some(window) = root.downcast_ref::<gtk4::Window>() {
                dialog.present(Some(window));
            }
        }
    }

    /// Validate a path string for dangerous sequences
    fn validate_path_string(path: &str, field_name: &str) -> Result<(), Box<dyn std::error::Error>> {
        // Check for dangerous sequences
        const FORBIDDEN: &[&str] = &[
            "..",       // Path traversal
            "\0",       // Null byte
            "\n", "\r", // Newlines
            "$(",       // Command substitution
            "`",        // Backtick
            ";",        // Command chaining
            "|",        // Pipe
            "&",        // Background/chain
            ">", "<",   // Redirect
        ];
        
        for seq in FORBIDDEN {
            if path.contains(seq) {
                return Err(format!("{} contains forbidden sequence: {:?}", field_name, seq).into());
            }
        }
        
        Ok(())
    }
    
    /// Restart the application by re-executing the current binary
    fn restart_application() {
        use std::process::Command;
        
        // Get current executable path
        if let Ok(exe) = std::env::current_exe() {
            // Collect current args (skip the program name)
            let args: Vec<String> = std::env::args().skip(1).collect();
            
            info!("Restarting application: {:?} {:?}", exe, args);
            
            // Spawn new process
            match Command::new(&exe).args(&args).spawn() {
                Ok(_) => {
                    // Exit current process
                    std::process::exit(0);
                }
                Err(e) => {
                    error!("Failed to restart application: {}", e);
                }
            }
        } else {
            error!("Failed to get current executable path for restart");
        }
    }
}

impl Default for SettingsPage {
    fn default() -> Self {
        Self::new()
    }
}
