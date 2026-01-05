//! Create New Control Dialog
//!
//! Dialog to create a fan control by selecting an existing curve,
//! a temperature source, and one or more fan/PWM controllers.
//! Includes live curve preview.

#![allow(dead_code)]

use gtk4::prelude::*;
use gtk4::{
    cairo, CheckButton, DrawingArea, Entry, Label, ListBox, ListBoxRow, Orientation, ScrolledWindow,
};
use gtk4::glib;
use gtk4::Box as GtkBox;
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use hf_core::PersistedCurve;

/// Data for a created control (supports multiple fans)
#[derive(Clone)]
pub struct PairData {
    pub id: String,
    pub name: String,
    pub curve_id: String,
    pub curve_name: String,
    pub temp_source_path: String,
    pub temp_source_label: String,
    pub fan_path: String,
    pub fan_label: String,
    pub fan_paths: Vec<String>,      // Multiple fan paths
    pub fan_labels: Vec<String>,     // Multiple fan labels
    pub points: Vec<(f32, f32)>,
    pub hysteresis_ms: u32,          // Hysteresis in milliseconds
}

/// Temperature source item
#[derive(Clone)]
struct TempSourceItem {
    path: String,
    chip_name: String,
    sensor_name: String,
    label: Option<String>,
    friendly_name: Option<String>,  // User-defined friendly name
    current_temp: Option<f32>,
}

impl TempSourceItem {
    fn display_name(&self) -> String {
        // Prefer friendly name, then label, then sensor name
        if let Some(ref friendly) = self.friendly_name {
            friendly.clone()
        } else if let Some(ref label) = self.label {
            format!("{} - {}", self.chip_name, label)
        } else {
            format!("{}/{}", self.chip_name, self.sensor_name)
        }
    }
    
    fn display_name_with_temp(&self) -> String {
        let name = self.display_name();
        if let Some(temp) = self.current_temp {
            format!("{} ({})", name, hf_core::display::format_temp_precise(temp))
        } else {
            name
        }
    }
}

/// Fan/PWM item
#[derive(Clone)]
struct FanItem {
    pwm_path: String,
    chip_name: String,
    pwm_name: String,
    pwm_num: String,
    friendly_name: Option<String>,
    paired_fan_name: Option<String>,
    /// The actual fan input path for reading RPM (e.g., /sys/class/hwmon/hwmon0/fan1_input)
    /// This comes from saved pairings, NOT from name-based heuristics
    fan_input_path: Option<String>,
    current_rpm: Option<u32>,
    selected: Rc<RefCell<bool>>,  // For multi-select
    assigned_to_control: Option<String>,  // Name of control this fan is assigned to
}

impl FanItem {
    fn display_name(&self) -> String {
        // Show friendly name first if available, then PWM info
        if let Some(ref friendly) = self.friendly_name {
            format!("{}   PWM{}     {}", friendly, self.pwm_num, self.chip_name)
        } else {
            format!("PWM{}     {}", self.pwm_num, self.chip_name)
        }
    }
    
    fn subtitle(&self) -> String {
        if let Some(ref fan) = self.paired_fan_name {
            format!("Paired with: {}", fan)
        } else {
            "Not paired".to_string()
        }
    }
}

/// Constants for dialog sizing and behavior
mod dialog_constants {
    /// Default dialog width in pixels
    pub const DEFAULT_WIDTH: i32 = 800;
    /// Default dialog height in pixels
    pub const DEFAULT_HEIGHT: i32 = 600;
    /// Minimum fan list height in pixels
    pub const MIN_FAN_LIST_HEIGHT: i32 = 140;
    
    /// PWM override TTL for live preview
    pub const PWM_OVERRIDE_TTL_MS: u32 = 1500;
    
    /// Temperature range for curve display
    pub mod temperature {
        /// Minimum temperature for curve display (°C)
        pub const MIN_TEMP: f32 = 20.0;
        /// Maximum temperature for curve display (°C)
        pub const MAX_TEMP: f32 = 100.0;
        /// Temperature range for calculations
        pub const RANGE: f32 = MAX_TEMP - MIN_TEMP;
    }
    
    /// Fan speed percentage constants
    pub mod fan_speed {
        /// Minimum fan speed percentage
        pub const MIN_PERCENT: f32 = 0.0;
        /// Maximum fan speed percentage
        pub const MAX_PERCENT: f32 = 100.0;
        /// Maximum PWM value (8-bit)
        pub const MAX_PWM: f32 = 255.0;
    }
}

/// Create New Control dialog
pub struct AddPairDialog {
    dialog: adw::Dialog,
    on_create: Rc<RefCell<Option<Box<dyn Fn(PairData)>>>>,
}

impl AddPairDialog {
    pub fn new(curves: &[PersistedCurve]) -> Rc<Self> {
        Self::new_with_existing_pairs(curves, &[])
    }
    
    pub fn new_for_edit(curves: &[PersistedCurve], existing_pair: &PairData) -> Rc<Self> {
        Self::new_internal(curves, &[], Some(existing_pair))
    }
    
    pub fn new_with_existing_pairs(curves: &[PersistedCurve], _existing_pairs: &[PairData]) -> Rc<Self> {
        Self::new_internal(curves, _existing_pairs, None)
    }
    
    fn new_internal(curves: &[PersistedCurve], _existing_pairs: &[PairData], edit_pair: Option<&PairData>) -> Rc<Self> {
        let is_edit_mode = edit_pair.is_some();
        let dialog = adw::Dialog::builder()
            .title(if is_edit_mode { "Edit Control" } else { "Create New Control" })
            .content_width(dialog_constants::DEFAULT_WIDTH)
            .content_height(dialog_constants::DEFAULT_HEIGHT)
            .build();
        
        // Make dialog resizable by the user
        dialog.set_follows_content_size(false);

        let on_create: Rc<RefCell<Option<Box<dyn Fn(PairData)>>>> = Rc::new(RefCell::new(None));

        let selected_curve: Rc<RefCell<Option<PersistedCurve>>> = Rc::new(RefCell::new(None));
        let selected_temp: Rc<RefCell<Option<TempSourceItem>>> = Rc::new(RefCell::new(None));
        let selected_fans: Rc<RefCell<Vec<FanItem>>> = Rc::new(RefCell::new(Vec::new()));
        let target_temp: Rc<RefCell<f32>> = Rc::new(RefCell::new(40.0));
        let display_temp: Rc<RefCell<f32>> = Rc::new(RefCell::new(40.0));
        let has_temp_source: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));
        let poll_interval_ms: Rc<RefCell<u32>> = Rc::new(RefCell::new(1000));
        
        if let Ok(settings) = hf_core::load_settings() {
            *poll_interval_ms.borrow_mut() = settings.general.poll_interval_ms;
        }

        let content = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(12)
            .margin_start(18)
            .margin_end(18)
            .margin_top(12)
            .margin_bottom(18)
            .vexpand(true)
            .hexpand(true)
            .build();

        // Header bar with name entry integrated
        let header = adw::HeaderBar::builder()
            .show_end_title_buttons(false)
            .show_start_title_buttons(false)
            .build();

        let cancel_btn = gtk4::Button::builder().label("Cancel").build();

        let create_btn = gtk4::Button::builder()
            .label(if is_edit_mode { "Save" } else { "Create" })
            .css_classes(["suggested-action"])
            .sensitive(false)
            .build();

        // Name entry in header bar (centered)
        let name_entry = Entry::builder()
            .placeholder_text("e.g., CPU Cooling")
            .hexpand(true)
            .build();
        
        // Validation label for name conflicts
        let name_validation = Label::builder()
            .css_classes(["error", "caption"])
            .halign(gtk4::Align::Start)
            .visible(false)
            .build();
        
        // Pre-fill name if editing
        if let Some(pair) = edit_pair {
            name_entry.set_text(&pair.name);
        }

        header.pack_start(&cancel_btn);
        header.set_title_widget(Some(&name_entry));
        header.pack_end(&create_btn);

        // Name entry change handler with validation
        let create_btn_for_name = create_btn.clone();
        let validation_for_name = name_validation.clone();
        let edit_pair_id_for_name = edit_pair.as_ref().map(|p| p.id.clone());
        name_entry.connect_changed(move |entry| {
            let text = entry.text();
            let is_empty = text.is_empty();
            
            // Check for duplicate names (excluding current pair if editing)
            let is_duplicate = if !is_empty {
                hf_core::load_settings()
                    .ok()
                    .map(|settings| {
                        settings.active_pairs.iter().any(|p| {
                            p.name == text.as_str() && 
                            Some(&p.id) != edit_pair_id_for_name.as_ref()
                        })
                    })
                    .unwrap_or(false)
            } else {
                false
            };
            
            if is_empty {
                validation_for_name.set_text("Name cannot be empty");
                validation_for_name.set_visible(true);
                create_btn_for_name.set_sensitive(false);
            } else if is_duplicate {
                validation_for_name.set_text("A control with this name already exists");
                validation_for_name.set_visible(true);
                create_btn_for_name.set_sensitive(false);
            } else {
                validation_for_name.set_visible(false);
                create_btn_for_name.set_sensitive(true);
            }
        });

        // ================================================================
        // Top Row: Curve Dropdown (LEFT) + Temp Source Dropdown (RIGHT)
        // ================================================================
        let dropdowns_row = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .homogeneous(true)
            .build();

        // --- Curve Dropdown (LEFT) ---
        let curve_dropdown_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(6)
            .build();

        let curve_label = Label::builder()
            .label("Fan Curve")
            .css_classes(["heading"])
            .halign(gtk4::Align::Start)
            .build();

        let curve_names: Vec<String> = curves.iter().map(|c| c.name.clone()).collect();
        let curve_model = gtk4::StringList::new(&curve_names.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        
        let curve_dropdown = gtk4::DropDown::builder()
            .model(&curve_model)
            .enable_search(true)
            .build();
        curve_dropdown.set_selected(gtk4::INVALID_LIST_POSITION);

        let curve_dropdown_frame = adw::Bin::builder()
            .css_classes(["card"])
            .child(&curve_dropdown)
            .build();

        curve_dropdown_box.append(&curve_label);
        curve_dropdown_box.append(&curve_dropdown_frame);

        // --- Temp Source Dropdown (RIGHT) ---
        let temp_dropdown_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(6)
            .build();

        let temp_label = Label::builder()
            .label("Temperature Source")
            .css_classes(["heading"])
            .halign(gtk4::Align::Start)
            .build();

        let temp_sources = Self::load_temp_sources();
        let temp_sources_ref = Rc::new(temp_sources.clone());
        // Show friendly name (or default) with live temperature
        let temp_names: Vec<String> = temp_sources.iter().map(|t| t.display_name_with_temp()).collect();
        let temp_model = gtk4::StringList::new(&temp_names.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        
        let temp_dropdown = gtk4::DropDown::builder()
            .model(&temp_model)
            .enable_search(true)
            .build();
        temp_dropdown.set_selected(gtk4::INVALID_LIST_POSITION);

        let temp_dropdown_frame = adw::Bin::builder()
            .css_classes(["card"])
            .child(&temp_dropdown)
            .build();

        temp_dropdown_box.append(&temp_label);
        temp_dropdown_box.append(&temp_dropdown_frame);

        dropdowns_row.append(&curve_dropdown_box);
        dropdowns_row.append(&temp_dropdown_box);
        content.append(&dropdowns_row);

        // ================================================================
        // Curve Preview Graph
        // ================================================================
        let curves_ref = Rc::new(curves.to_vec());

        let preview_drawing = DrawingArea::builder()
            .height_request(160)
            .hexpand(true)
            .build();

        let preview_frame = adw::Bin::builder()
            .css_classes(["card", "graph-preview-card"])
            .child(&preview_drawing)
            .build();
        
        // Add custom CSS for rounded corners and polished styling
        let css_provider = gtk4::CssProvider::new();
        css_provider.load_from_string(
            ".graph-preview-card { border-radius: 12px; }"
        );
        if let Some(display) = gtk4::gdk::Display::default() {
            gtk4::style_context_add_provider_for_display(
                &display,
                &css_provider,
                gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }

        content.append(&preview_frame);

        // ================================================================
        // Fan Controllers (Multi-select with checkboxes, rounded corners)
        // ================================================================
        let fan_label = Label::builder()
            .label("Fan Controllers")
            .css_classes(["heading"])
            .halign(gtk4::Align::Start)
            .build();
        content.append(&fan_label);

        let fan_scroll = ScrolledWindow::builder()
            .height_request(dialog_constants::MIN_FAN_LIST_HEIGHT)
            .vexpand(true)
            .build();

        let fan_list = ListBox::builder()
            .selection_mode(gtk4::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();

        // Load active pairs from settings to get current state (not stale in-memory state)
        // Exclude the current pair being edited so its fans don't show as "assigned"
        let edit_pair_id = edit_pair.map(|p| p.id.as_str());
        let active_pairs = Self::load_active_pairs_from_settings_excluding(edit_pair_id);
        let fans = Self::load_fans(&active_pairs);
        let _fans_ref = Rc::new(RefCell::new(fans.clone()));

        for fan in &fans {
            let row = Self::create_fan_row_with_checkbox(
                fan, 
                selected_fans.clone(), 
                create_btn.clone(), 
                selected_curve.clone(), 
                selected_temp.clone(),
                edit_pair
            );
            fan_list.append(&row);
        }

        let fan_frame = adw::Bin::builder()
            .css_classes(["card", "fan-list-card"])
            .child(&fan_list)
            .build();

        fan_scroll.set_child(Some(&fan_frame));
        content.append(&fan_scroll);

        // Main container
        let scroll = ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .child(&content)
            .build();

        let main_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .vexpand(true)
            .hexpand(true)
            .build();
        main_box.append(&header);
        main_box.append(&scroll);

        dialog.set_child(Some(&main_box));
        
        // Add keyboard shortcuts
        let key_controller = gtk4::EventControllerKey::new();
        let dialog_for_keys = dialog.clone();
        let create_btn_for_keys = create_btn.clone();
        key_controller.connect_key_pressed(move |_, key, _, modifiers| {
            match key {
                gtk4::gdk::Key::Escape => {
                    dialog_for_keys.close();
                    glib::Propagation::Stop
                }
                gtk4::gdk::Key::Return | gtk4::gdk::Key::KP_Enter => {
                    if create_btn_for_keys.is_sensitive() {
                        create_btn_for_keys.activate();
                    }
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed
            }
        });
        dialog.add_controller(key_controller);

        let this = Rc::new(Self { dialog, on_create });
        
        // Pre-select curve and temp source if editing
        if let Some(pair) = edit_pair {
            // Find and select the curve
            if let Some(curve_idx) = curves.iter().position(|c| c.id == pair.curve_id) {
                curve_dropdown.set_selected(curve_idx as u32);
                *selected_curve.borrow_mut() = Some(curves[curve_idx].clone());
                preview_drawing.queue_draw();
            }
            
            // Find and select the temperature source
            if let Some(temp_idx) = temp_sources.iter().position(|t| t.path == pair.temp_source_path) {
                temp_dropdown.set_selected(temp_idx as u32);
                *selected_temp.borrow_mut() = Some(temp_sources[temp_idx].clone());
                *has_temp_source.borrow_mut() = true;
                if let Some(temp) = temp_sources[temp_idx].current_temp {
                    *target_temp.borrow_mut() = temp;
                    *display_temp.borrow_mut() = temp;
                }
            }
            
            // Pre-select fans that are in this control
            // This will be handled in the fan row creation
        } else if curves.len() == 1 {
            // Auto-select first curve if only one available (create mode)
            curve_dropdown.set_selected(0);
            *selected_curve.borrow_mut() = Some(curves[0].clone());
            name_entry.set_text(&format!("{} Control", curves[0].name));
            preview_drawing.queue_draw();
        }

        // Setup preview drawing - uses display_temp for smooth animation
        let selected_curve_for_draw = selected_curve.clone();
        let display_temp_for_draw = display_temp.clone();
        let has_temp_source_for_draw = has_temp_source.clone();
        preview_drawing.set_draw_func(move |_, cr, width, height| {
            Self::draw_curve_preview(
                cr,
                width,
                height,
                &selected_curve_for_draw.borrow(),
                *display_temp_for_draw.borrow(),
                *has_temp_source_for_draw.borrow(),
            );
        });

        // Curve dropdown selection handler
        let selected_curve_for_dropdown = selected_curve.clone();
        let curves_for_dropdown = curves_ref.clone();
        let preview_for_curve = preview_drawing.clone();
        let name_entry_for_curve = name_entry.clone();
        let create_btn_for_curve = create_btn.clone();
        let selected_temp_for_validate = selected_temp.clone();
        let selected_fans_for_validate = selected_fans.clone();

        curve_dropdown.connect_selected_notify(move |dropdown| {
            let idx = dropdown.selected() as usize;
            if idx < curves_for_dropdown.len() {
                let curve = &curves_for_dropdown[idx];
                *selected_curve_for_dropdown.borrow_mut() = Some(curve.clone());

                // Auto-fill name
                if name_entry_for_curve.text().is_empty() {
                    name_entry_for_curve.set_text(&format!("{} Control", curve.name));
                }

                preview_for_curve.queue_draw();

                // Validate - need curve (just selected), temp source, and at least one fan
                let has_curve = true; // Just selected
                let has_temp = selected_temp_for_validate.borrow().is_some();
                let fans_list = selected_fans_for_validate.borrow();
                let has_fans = !fans_list.is_empty();
                let has_all = has_curve && has_temp && has_fans;
                
                tracing::trace!(curve = has_curve, temp = has_temp, fans = has_fans, 
                               fans_count = fans_list.len(), enabled = has_all, "Curve selected validation");
                
                create_btn_for_curve.set_sensitive(has_all);
            }
        });

        // Temp dropdown selection handler
        let selected_temp_for_dropdown = selected_temp.clone();
        let temp_sources_for_dropdown = temp_sources_ref.clone();
        let target_temp_for_select = target_temp.clone();
        let display_temp_for_select = display_temp.clone();
        let preview_for_temp = preview_drawing.clone();
        let create_btn_for_temp = create_btn.clone();
        let selected_curve_for_validate = selected_curve.clone();
        let selected_fans_for_validate2 = selected_fans.clone();
        let has_temp_source_for_select = has_temp_source.clone();

        temp_dropdown.connect_selected_notify(move |dropdown| {
            let idx = dropdown.selected() as usize;
            if idx < temp_sources_for_dropdown.len() {
                let source = &temp_sources_for_dropdown[idx];
                *selected_temp_for_dropdown.borrow_mut() = Some(source.clone());
                *has_temp_source_for_select.borrow_mut() = true;

                if let Some(temp) = source.current_temp {
                    *target_temp_for_select.borrow_mut() = temp;
                    *display_temp_for_select.borrow_mut() = temp;
                }

                preview_for_temp.queue_draw();

                // Validate
                let has_curve = selected_curve_for_validate.borrow().is_some();
                let has_temp = true; // Just selected
                let fans_list = selected_fans_for_validate2.borrow();
                let has_fans = !fans_list.is_empty();
                let has_all = has_curve && has_temp && has_fans;
                
                tracing::trace!(curve = has_curve, temp = has_temp, fans = has_fans,
                               fans_count = fans_list.len(), enabled = has_all, "Temp selected validation");
                
                create_btn_for_temp.set_sensitive(has_all);
            }
        });

        // Live temperature updates via runtime subscription (world-class approach)
        // Subscribe to sensor updates from the background worker for zero-latency updates
        let selected_temp_for_update = selected_temp.clone();
        let target_temp_for_update = target_temp.clone();
        let selected_curve_for_ramp = selected_curve.clone();
        let selected_fans_for_ramp = selected_fans.clone();
        let dialog_for_cleanup = this.dialog.clone();
        
        if let Some(mut rx) = crate::runtime::subscribe_ui() {
            glib::spawn_future_local(async move {
                while let Ok(update) = rx.recv().await {
                    match update {
                        crate::runtime::UiUpdate::SensorData(_) => {
                            // Fetch latest sensor data from runtime cache (lock-free)
                            if let Some(source) = selected_temp_for_update.borrow().as_ref() {
                                let temp_result = if source.path.starts_with("gpu:") {
                                    // GPU temperature from cache
                                    crate::runtime::get_sensors()
                                        .and_then(|data| {
                                            // Parse gpu:index:name format
                                            let parts: Vec<&str> = source.path.split(':').collect();
                                            if parts.len() >= 3 {
                                                let gpu_idx = parts[1].parse::<u32>().ok()?;
                                                let temp_name = parts[2];
                                                data.gpus.iter()
                                                    .find(|g| g.index == gpu_idx)
                                                    .and_then(|gpu| gpu.temperatures.get(temp_name).copied())
                                            } else {
                                                None
                                            }
                                        })
                                } else {
                                    // hwmon temperature from cache
                                    crate::runtime::get_sensors()
                                        .and_then(|data| {
                                            data.temperatures.iter()
                                                .find(|t| t.path == source.path)
                                                .map(|t| t.temp_celsius)
                                        })
                                };
                                
                                if let Some(temp) = temp_result {
                                    *target_temp_for_update.borrow_mut() = temp;
                                    
                                    // Live fan ramping - apply curve to selected fans
                                    if let Some(curve) = selected_curve_for_ramp.borrow().as_ref() {
                                        let fans = selected_fans_for_ramp.borrow();
                                        if !fans.is_empty() {
                                            let fan_percent = Self::interpolate_percent(&curve.points, temp);
                                            let pwm_value = ((fan_percent / dialog_constants::fan_speed::MAX_PERCENT) * dialog_constants::fan_speed::MAX_PWM).round() as u8;
                                            
                                            for fan in fans.iter() {
                                                // Live preview: use short-lived daemon override
                                                let _ = hf_core::daemon_client::daemon_set_pwm_override(
                                                    &fan.pwm_path, 
                                                    pwm_value, 
                                                    dialog_constants::PWM_OVERRIDE_TTL_MS
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                    
                    // Stop listening if dialog is closed
                    if !dialog_for_cleanup.is_visible() {
                        break;
                    }
                }
            });
        }

        // WORLD-CLASS ANIMATION: Sinusoidal easing with precise FPS throttling
        // Features:
        // - Smooth ease-in-out sine curve for buttery transitions
        // - Respects user's FPS setting from preferences
        // - Frame-perfect timing using GTK frame clock
        // - Zero jitter, zero tearing
        let target_temp_for_anim = target_temp.clone();
        let display_temp_for_anim = display_temp.clone();
        let has_temp_source_for_anim = has_temp_source.clone();
        let last_frame: Rc<RefCell<Option<i64>>> = Rc::new(RefCell::new(None));
        let anim_start: Rc<RefCell<f32>> = Rc::new(RefCell::new(0.0));
        let anim_target: Rc<RefCell<f32>> = Rc::new(RefCell::new(0.0));
        let anim_progress: Rc<RefCell<f32>> = Rc::new(RefCell::new(1.0)); // 1.0 = complete
        let last_render_time: Rc<RefCell<Option<i64>>> = Rc::new(RefCell::new(None));
        
        preview_drawing.add_tick_callback(move |widget, frame_clock| {
            if !*has_temp_source_for_anim.borrow() {
                return glib::ControlFlow::Continue;
            }

            let frame_time = frame_clock.frame_time();
            
            // Get configured frame rate (0 = native refresh rate, otherwise throttle)
            let target_fps = hf_core::get_frame_rate();
            
            // Precise frame pacing: throttle to target FPS if specified
            if target_fps > 0 {
                let frame_interval_us = 1_000_000 / target_fps as i64;
                let mut last_render = last_render_time.borrow_mut();
                
                if let Some(last) = *last_render {
                    let elapsed = frame_time - last;
                    if elapsed < frame_interval_us {
                        // Skip this frame to maintain precise target FPS
                        return glib::ControlFlow::Continue;
                    }
                }
                *last_render = Some(frame_time);
            }

            let target = *target_temp_for_anim.borrow();
            let mut display = display_temp_for_anim.borrow_mut();
            let mut last = last_frame.borrow_mut();
            let mut start = anim_start.borrow_mut();
            let mut anim_tgt = anim_target.borrow_mut();
            let mut progress = anim_progress.borrow_mut();
            
            // Calculate delta time from frame clock (microseconds to seconds)
            // Cap at 50ms to prevent huge jumps if window was hidden
            let dt = match *last {
                Some(prev) => ((frame_time - prev) as f64 / 1_000_000.0).min(0.05) as f32,
                None => 1.0 / 144.0, // Assume 144Hz for first frame
            };
            *last = Some(frame_time);
            
            // Detect target change - start new animation
            if (target - *anim_tgt).abs() > 0.01 {
                *start = *display;
                *anim_tgt = target;
                *progress = 0.0;
            }
            
            // Continue animation if in progress
            let is_animating = *progress < 1.0;
            
            if is_animating {
                // Animation duration: 350ms for smooth, natural feel
                let duration: f32 = 0.35;
                
                // Advance progress based on real elapsed time
                *progress += dt / duration;
                if *progress > 1.0 {
                    *progress = 1.0;
                }
                
                // Sinusoidal ease-in-out: -(cos(π * t) - 1) / 2
                // Creates smooth wave-like acceleration/deceleration
                // This is the secret sauce for buttery smooth animations
                let t = *progress;
                let eased = -(std::f32::consts::PI * t).cos() * 0.5 + 0.5;
                
                // Interpolate between start and target using eased progress
                *display = *start + (*anim_tgt - *start) * eased;
                
                // Snap to target at end to prevent floating point drift
                if *progress >= 1.0 {
                    *display = *anim_tgt;
                }
            }
            
            drop(display);
            drop(last);
            drop(start);
            drop(anim_tgt);
            drop(progress);
            
            // Queue redraw when animating (at configured FPS)
            if is_animating {
                widget.queue_draw();
            }
            
            glib::ControlFlow::Continue
        });

        // Cancel button
        let dialog_for_cancel = this.dialog.clone();
        cancel_btn.connect_clicked(move |_| {
            dialog_for_cancel.close();
        });

        // Create button
        let this_for_create = this.clone();
        let selected_curve_for_create = selected_curve.clone();
        let selected_temp_for_create = selected_temp.clone();
        let selected_fans_for_create = selected_fans.clone();
        let name_entry_for_create = name_entry.clone();
        let edit_pair_id = edit_pair.map(|p| p.id.clone());

        create_btn.connect_clicked(move |_| {
            let name = name_entry_for_create.text().to_string();
            let curve = selected_curve_for_create.borrow().clone();
            let temp = selected_temp_for_create.borrow().clone();
            let fans = selected_fans_for_create.borrow().clone();

            tracing::debug!(name = %name, curve = ?curve.as_ref().map(|c| &c.name), 
                           temp = ?temp.as_ref().map(|t| &t.path), fans_count = fans.len(),
                           "Create/Save button clicked");

            if let (Some(curve), Some(temp)) = (curve, temp) {
                if !name.is_empty() && !fans.is_empty() {
                    // Build fan paths and labels
                    let fan_paths: Vec<String> = fans.iter().map(|f| f.pwm_path.clone()).collect();
                    let fan_labels: Vec<String> = fans.iter().map(|f| f.display_name()).collect();
                    
                    // Use first fan for backward compatibility
                    let first_fan = match fans.first() {
                        Some(f) => f,
                        None => return, // Skip if no fans
                    };
                    
                    let data = PairData {
                        id: edit_pair_id.clone().unwrap_or_else(hf_core::generate_guid),
                        name,
                        curve_id: curve.id.clone(),
                        curve_name: curve.name.clone(),
                        temp_source_path: temp.path.clone(),
                        temp_source_label: temp.display_name(),
                        fan_path: first_fan.pwm_path.clone(),
                        fan_label: first_fan.display_name(),
                        fan_paths,
                        fan_labels,
                        points: curve.points.clone(),
                        hysteresis_ms: 0, // Hysteresis is controlled by fan control loop
                    };

                    if let Some(callback) = this_for_create.on_create.borrow().as_ref() {
                        callback(data);
                    }

                    this_for_create.dialog.close();
                }
            }
        });

        // Initial validation check - run after dialog is fully constructed
        // This ensures the button is enabled if all fields are already valid
        glib::idle_add_local_once({
            let create_btn = create_btn.clone();
            let selected_curve = selected_curve.clone();
            let selected_temp = selected_temp.clone();
            let selected_fans = selected_fans.clone();
            
            move || {
                let has_curve = selected_curve.borrow().is_some();
                let has_temp = selected_temp.borrow().is_some();
                let fans_list = selected_fans.borrow();
                let has_fans = !fans_list.is_empty();
                let has_all = has_curve && has_temp && has_fans;
                
                tracing::trace!(curve = has_curve, temp = has_temp, fans = has_fans,
                               fans_count = fans_list.len(), enabled = has_all, "Initial validation");
                
                create_btn.set_sensitive(has_all);
            }
        });

        this
    }
    
    fn create_fan_row_with_checkbox(
        fan: &FanItem,
        selected_fans: Rc<RefCell<Vec<FanItem>>>,
        create_btn: gtk4::Button,
        selected_curve: Rc<RefCell<Option<PersistedCurve>>>,
        selected_temp: Rc<RefCell<Option<TempSourceItem>>>,
        edit_pair: Option<&PairData>,
    ) -> ListBoxRow {
        let row = ListBoxRow::new();
        let content = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .margin_start(12)
            .margin_end(12)
            .margin_top(8)
            .margin_bottom(8)
            .build();

        let checkbox = CheckButton::new();
        
        // Check if this fan should be pre-selected (when editing)
        let should_preselect = if let Some(pair) = edit_pair {
            pair.fan_paths.contains(&fan.pwm_path)
        } else {
            false
        };
        
        // Pre-select and add to selected_fans if editing
        if should_preselect {
            checkbox.set_active(true);
            let mut fans = selected_fans.borrow_mut();
            if !fans.iter().any(|f| f.pwm_path == fan.pwm_path) {
                fans.push(fan.clone());
                tracing::debug!(fan = %fan.display_name(), path = %fan.pwm_path, "Pre-selected fan for edit");
            }
        }
        
        // Disable checkbox if fan is already assigned to another control (but not this one)
        if let Some(ref control_name) = fan.assigned_to_control {
            // Allow editing fans in the current control
            let is_current_control = edit_pair.map(|p| &p.name == control_name).unwrap_or(false);
            if !is_current_control {
                checkbox.set_sensitive(false);
            }
        }
        
        let info_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(2)
            .hexpand(true)
            .build();

        let label = Label::builder()
            .label(&fan.display_name())
            .halign(gtk4::Align::Start)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .build();

        // Show normal subtitle (no "assigned to" warning in edit screen)
        let subtitle = Label::builder()
            .label(&fan.subtitle())
            .css_classes(["caption", "dim-label"])
            .halign(gtk4::Align::Start)
            .build();

        info_box.append(&label);
        info_box.append(&subtitle);

        let rpm = Label::builder()
            .label(&fan.current_rpm.map(|r| format!("{} RPM", r)).unwrap_or_else(|| "-- RPM".to_string()))
            .css_classes(["dim-label", "numeric"])
            .build();

        content.append(&checkbox);
        content.append(&info_box);
        content.append(&rpm);
        row.set_child(Some(&content));

        // Checkbox toggle handler
        let fan_clone = fan.clone();
        checkbox.connect_toggled(move |cb| {
            let mut fans = selected_fans.borrow_mut();
            if cb.is_active() {
                // Add fan to selection
                if !fans.iter().any(|f| f.pwm_path == fan_clone.pwm_path) {
                    fans.push(fan_clone.clone());
                    tracing::debug!(fan = %fan_clone.display_name(), path = %fan_clone.pwm_path, "Added fan to selection");
                }
            } else {
                // Remove fan from selection
                fans.retain(|f| f.pwm_path != fan_clone.pwm_path);
                tracing::debug!(fan = %fan_clone.display_name(), path = %fan_clone.pwm_path, "Removed fan from selection");
            }
            
            // Validate - need curve, temp source, and at least one fan
            let has_curve = selected_curve.borrow().is_some();
            let has_temp = selected_temp.borrow().is_some();
            let has_fans = !fans.is_empty();
            let has_all = has_curve && has_temp && has_fans;
            
            tracing::trace!(curve = has_curve, temp = has_temp, fans = has_fans,
                           fans_count = fans.len(), enabled = has_all, "Checkbox validation");
            
            create_btn.set_sensitive(has_all);
        });

        row
    }

    fn load_temp_sources() -> Vec<TempSourceItem> {
        tracing::debug!("Loading temperature sources");
        let mut sources = Vec::new();

        // GPU temps via daemon (authoritative)
        if let Ok(daemon_gpus) = hf_core::daemon_list_gpus() {
            tracing::trace!(gpu_count = daemon_gpus.len(), "Found GPUs");
            for gpu in daemon_gpus {
                if let Some(temp) = gpu.temp {
                    let path = format!("gpu:{}:GPU", gpu.index);
                    let friendly_name = hf_core::get_sensor_friendly_name(&path)
                        .ok()
                        .flatten();
                    sources.push(TempSourceItem {
                        path,
                        chip_name: format!("{} ({})", gpu.name, gpu.vendor),
                        sensor_name: "GPU".to_string(),
                        label: Some("GPU".to_string()),
                        friendly_name,
                        current_temp: Some(temp),
                    });
                }
            }
        } else {
            tracing::warn!("Failed to query daemon for GPUs");
        }

        // Hwmon temps via daemon (authoritative)
        if let Ok(hw) = hf_core::daemon_list_hardware() {
            tracing::trace!(chip_count = hw.chips.len(), "Found hwmon chips");
            for chip in hw.chips {
                if chip.name.contains("amdgpu") {
                    continue;
                }
                for temp in chip.temperatures {
                    let path = temp.path.clone();
                    let friendly_name = hf_core::get_sensor_friendly_name(&path)
                        .ok()
                        .flatten();
                    sources.push(TempSourceItem {
                        path,
                        chip_name: chip.name.clone(),
                        sensor_name: temp.name,
                        label: temp.label,
                        friendly_name,
                        current_temp: Some(temp.value),
                    });
                }
            }
        } else {
            tracing::warn!("Failed to query daemon for hardware");
        }

        tracing::debug!(count = sources.len(), "Temperature sources loaded");
        sources
    }

    /// Load active pairs from settings file to ensure we have the latest state
    /// This prevents showing fans as "assigned" when their control has been deleted
    /// Optionally excludes a pair by ID (used when editing to not mark current pair's fans as assigned)
    fn load_active_pairs_from_settings_excluding(exclude_id: Option<&str>) -> Vec<PairData> {
        let Ok(settings) = hf_core::load_settings() else {
            return Vec::new();
        };
        
        let Ok(curves_store) = hf_core::load_curves() else {
            return Vec::new();
        };
        
        settings.active_pairs
            .into_iter()
            .filter(|pair| {
                // Exclude the pair being edited
                if let Some(id) = exclude_id {
                    pair.id != id
                } else {
                    true
                }
            })
            .filter_map(|pair| {
                // Find the curve to get its points
                curves_store.all().iter()
                    .find(|c| c.id == pair.curve_id)
                    .map(|curve| {
                        // Use fan_paths if available, otherwise fall back to single fan_path
                        let fan_paths = if !pair.fan_paths.is_empty() {
                            pair.fan_paths.clone()
                        } else {
                            vec![pair.fan_path.clone()]
                        };
                        
                        PairData {
                            id: pair.id.clone(),
                            name: pair.name.clone(),
                            curve_id: pair.curve_id.clone(),
                            curve_name: curve.name.clone(),
                            temp_source_path: pair.temp_source_path.clone(),
                            temp_source_label: String::new(),
                            fan_path: pair.fan_path.clone(),
                            fan_label: String::new(),
                            fan_paths,
                            fan_labels: vec![],
                            points: curve.points.clone(),
                            hysteresis_ms: pair.hysteresis_ms,
                        }
                    })
            })
            .collect()
    }
    
    fn load_fans(existing_pairs: &[PairData]) -> Vec<FanItem> {
        let mut fans = Vec::new();
        
        // Load settings to get friendly names and pairings
        let settings = hf_core::load_settings().ok();

        // Use daemon for hardware enumeration (authoritative)
        tracing::info!("load_fans: Calling daemon_list_hardware...");
        let hw = match hf_core::daemon_list_hardware() {
            Ok(hw) => {
                tracing::info!("load_fans: Got {} chips from daemon", hw.chips.len());
                for chip in &hw.chips {
                    tracing::debug!("load_fans: Chip '{}' has {} PWMs", chip.name, chip.pwms.len());
                }
                hw
            }
            Err(e) => {
                tracing::error!("load_fans: daemon_list_hardware FAILED: {}", e);
                return fans;
            }
        };
        
        for chip in hw.chips {
            for pwm in &chip.pwms {
                let pwm_path = pwm.path.clone();
                
                // Extract PWM number
                let pwm_num: String = pwm.name.chars().filter(|c| c.is_ascii_digit()).collect();
                let pwm_num = if pwm_num.is_empty() { "?".to_string() } else { pwm_num };
                
                // Look up friendly name and pairing from settings
                let pairing = settings.as_ref()
                    .and_then(|s| s.pwm_fan_pairings.iter().find(|p| p.pwm_path == pwm_path));
                
                let friendly_name = pairing.and_then(|p| p.friendly_name.clone());
                let paired_fan_name = pairing.and_then(|p| p.fan_name.clone());
                
                // Check if this fan is already assigned to an existing control
                let assigned_to_control = existing_pairs.iter()
                    .find(|p| p.fan_path == pwm_path || p.fan_paths.contains(&pwm_path))
                    .map(|p| p.name.clone());
                
                // Get fan input path from saved pairing (authoritative source)
                // This is the CORRECT way to get the fan path - from user-confirmed pairings
                let fan_input_path = pairing.and_then(|p| p.fan_path.clone());
                
                // Read RPM from the CORRECT fan input path (from saved pairing)
                // NOT from a name-based heuristic which is unreliable
                let rpm = if let Some(ref fan_path) = fan_input_path {
                    // Use the saved fan path to read RPM
                    chip.fans.iter()
                        .find(|f| f.path == *fan_path)
                        .and_then(|f| f.rpm)
                } else {
                    // Fallback: try name-based matching only if no saved pairing exists
                    // This is less reliable but better than nothing for unpaired PWMs
                    chip.fans.iter()
                        .find(|f| f.name.replace("fan", "") == pwm.name.replace("pwm", ""))
                        .and_then(|f| f.rpm)
                };

                fans.push(FanItem {
                    pwm_path,
                    chip_name: chip.name.clone(),
                    pwm_name: pwm.name.clone(),
                    pwm_num,
                    friendly_name,
                    paired_fan_name,
                    fan_input_path,
                    current_rpm: rpm,
                    selected: Rc::new(RefCell::new(false)),
                    assigned_to_control,
                });
            }
        }

        // Load GPU fan controllers
        let gpu_controllers = hf_core::enumerate_gpu_pwm_controllers();
        tracing::info!("Enumerated {} GPU PWM controllers", gpu_controllers.len());
        for gpu in &gpu_controllers {
            tracing::debug!("GPU controller: id={}, name={}, vendor={:?}", gpu.id, gpu.name, gpu.vendor);
        }
        
        for gpu in gpu_controllers {
            let pwm_path = gpu.id.clone();
            
            // Extract GPU and fan index from ID (format: "vendor:gpu_index:fan_index")
            let parts: Vec<&str> = pwm_path.split(':').collect();
            let pwm_num = if parts.len() >= 3 {
                format!("GPU{}:{}", parts[1], parts[2])
            } else {
                "?".to_string()
            };
            
            tracing::info!("Adding GPU fan to list: {} ({})", gpu.name, pwm_path);
            
            // Look up friendly name and pairing from settings
            let pairing = settings.as_ref()
                .and_then(|s| s.pwm_fan_pairings.iter().find(|p| p.pwm_path == pwm_path));
            
            let friendly_name = pairing.and_then(|p| p.friendly_name.clone());
            let paired_fan_name = pairing.and_then(|p| p.fan_name.clone());
            
            // Check if this fan is already assigned to an existing control
            let assigned_to_control = existing_pairs.iter()
                .find(|p| p.fan_path == pwm_path || p.fan_paths.contains(&pwm_path))
                .map(|p| p.name.clone());
            
            fans.push(FanItem {
                pwm_path,
                chip_name: format!("{} GPU", gpu.vendor),
                pwm_name: gpu.name.clone(),
                pwm_num,
                friendly_name,
                paired_fan_name,
                fan_input_path: None, // GPU fans don't have separate input paths
                current_rpm: gpu.current_rpm,
                selected: Rc::new(RefCell::new(false)),
                assigned_to_control,
            });
        }
        
        tracing::info!("Total fans loaded: {} (motherboard + GPU)", fans.len());

        fans
    }

    fn create_temp_row(source: &TempSourceItem) -> ListBoxRow {
        let row = ListBoxRow::new();
        let content = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .margin_start(12)
            .margin_end(12)
            .margin_top(6)
            .margin_bottom(6)
            .build();

        let label = Label::builder()
            .label(&source.display_name())
            .hexpand(true)
            .halign(gtk4::Align::Start)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .build();

        let temp = Label::builder()
            .label(&hf_core::display::format_temp_precise(source.current_temp.unwrap_or(0.0)))
            .css_classes(["dim-label", "numeric"])
            .build();

        content.append(&label);
        content.append(&temp);
        row.set_child(Some(&content));
        row
    }

    fn create_fan_row(fan: &FanItem) -> ListBoxRow {
        let row = ListBoxRow::new();
        let content = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(2)
            .margin_start(12)
            .margin_end(12)
            .margin_top(8)
            .margin_bottom(8)
            .build();

        let top_row = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .build();

        let label = Label::builder()
            .label(&fan.display_name())
            .hexpand(true)
            .halign(gtk4::Align::Start)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .build();

        let rpm = Label::builder()
            .label(&fan.current_rpm.map(|r| format!("{} RPM", r)).unwrap_or_else(|| "-- RPM".to_string()))
            .css_classes(["dim-label", "numeric"])
            .build();

        top_row.append(&label);
        top_row.append(&rpm);
        
        let subtitle = Label::builder()
            .label(&fan.subtitle())
            .css_classes(["dim-label", "caption"])
            .halign(gtk4::Align::Start)
            .build();

        content.append(&top_row);
        content.append(&subtitle);
        row.set_child(Some(&content));
        row
    }

    fn draw_curve_preview(
        cr: &cairo::Context,
        width: i32,
        height: i32,
        curve: &Option<PersistedCurve>,
        current_temp: f32,
        has_temp_source: bool,
    ) {
        let w = width as f64;
        let h = height as f64;
        let margin = 20.0;

        // Background - theme-aware for WCAG AA compliance
        let is_dark = super::curve_card::theme_colors::is_dark_mode();
        if is_dark {
            cr.set_source_rgb(0.12, 0.12, 0.14);
        } else {
            cr.set_source_rgb(0.85, 0.85, 0.88);
        }
        cr.rectangle(0.0, 0.0, w, h);
        let _ = cr.fill();

        // Grid - use theme colors
        let grid = super::curve_card::theme_colors::grid_line();
        cr.set_source_rgba(grid.0, grid.1, grid.2, grid.3);
        cr.set_line_width(1.0);

        for temp in (20..=100).step_by(20) {
            let x = margin + ((temp - 20) as f64 / 80.0) * (w - 2.0 * margin);
            cr.move_to(x, margin);
            cr.line_to(x, h - margin);
        }

        for pct in (0..=100).step_by(25) {
            let y = h - margin - (pct as f64 / 100.0) * (h - 2.0 * margin);
            cr.move_to(margin, y);
            cr.line_to(w - margin, y);
        }
        let _ = cr.stroke();

        // Axis labels - theme-aware
        if is_dark {
            cr.set_source_rgb(0.6, 0.6, 0.6);
        } else {
            cr.set_source_rgb(0.15, 0.15, 0.15);
        }
        cr.set_font_size(10.0);

        for temp in (20..=100).step_by(20) {
            let x = margin + ((temp - 20) as f64 / 80.0) * (w - 2.0 * margin);
            cr.move_to(x - 8.0, h - 5.0);
            let _ = cr.show_text(&format!("{}°", temp));
        }

        let Some(curve) = curve else {
            // No curve selected - show placeholder text (theme-aware)
            if is_dark {
                cr.set_source_rgba(0.5, 0.5, 0.5, 0.8);
            } else {
                cr.set_source_rgba(0.3, 0.3, 0.3, 0.9);
            }
            cr.set_font_size(14.0);
            cr.move_to(w / 2.0 - 60.0, h / 2.0);
            let _ = cr.show_text("Select a curve");
            return;
        };

        let points = &curve.points;
        if points.is_empty() {
            return;
        }

        // PERFORMANCE: Use cached settings (no disk I/O in draw function)
        let graph_style = hf_core::get_graph_style();

        let temp_to_x = |t: f32| margin + ((t - dialog_constants::temperature::MIN_TEMP) / dialog_constants::temperature::RANGE) as f64 * (w - 2.0 * margin);
        let pct_to_y = |p: f32| h - margin - (p / dialog_constants::fan_speed::MAX_PERCENT) as f64 * (h - 2.0 * margin);

        // Determine the end point for active curve drawing
        // If we have a temp source, only draw up to current_temp (animated)
        let draw_end_temp = if has_temp_source {
            current_temp.clamp(dialog_constants::temperature::MIN_TEMP, dialog_constants::temperature::MAX_TEMP)
        } else {
            dialog_constants::temperature::MAX_TEMP // Draw full curve when no temp source
        };
        let draw_end_percent = Self::interpolate_percent(points, draw_end_temp);

        // Curve fill (only for "filled" style) - draws up to indicator
        let accent = super::curve_card::theme_colors::curve_line();
        let fill = super::curve_card::theme_colors::curve_fill();
        if graph_style == "filled" {
            cr.set_source_rgba(fill.0, fill.1, fill.2, fill.3);
            cr.move_to(temp_to_x(20.0), pct_to_y(0.0));
            cr.line_to(temp_to_x(20.0), pct_to_y(points[0].1));

            for (t, p) in points {
                if *t > draw_end_temp {
                    // Stop at the indicator position
                    cr.line_to(temp_to_x(draw_end_temp), pct_to_y(draw_end_percent));
                    break;
                }
                cr.line_to(temp_to_x(*t), pct_to_y(*p));
            }

            // If we drew past all points, extend to indicator
            if points.last().map(|(t, _)| *t <= draw_end_temp).unwrap_or(false) {
                cr.line_to(temp_to_x(draw_end_temp), pct_to_y(draw_end_percent));
            }

            cr.line_to(temp_to_x(draw_end_temp), pct_to_y(0.0));
            cr.close_path();
            let _ = cr.fill();
        }

        // Curve line - active portion (up to indicator)
        cr.set_source_rgba(accent.0, accent.1, accent.2, 1.0);
        cr.set_line_width(2.5);

        cr.move_to(temp_to_x(20.0), pct_to_y(points[0].1));
        let mut prev_p = points[0].1;
        for (t, p) in points {
            if *t > draw_end_temp {
                // Draw to indicator position and stop
                match graph_style.as_str() {
                    "stepped" => {
                        cr.line_to(temp_to_x(draw_end_temp), pct_to_y(prev_p));
                    }
                    _ => {
                        cr.line_to(temp_to_x(draw_end_temp), pct_to_y(draw_end_percent));
                    }
                }
                break;
            }
            match graph_style.as_str() {
                "stepped" => {
                    cr.line_to(temp_to_x(*t), pct_to_y(prev_p));
                    cr.line_to(temp_to_x(*t), pct_to_y(*p));
                }
                _ => {
                    cr.line_to(temp_to_x(*t), pct_to_y(*p));
                }
            }
            prev_p = *p;
        }
        // Extend to indicator if past all points
        if points.last().map(|(t, _)| *t <= draw_end_temp).unwrap_or(false) {
            cr.line_to(temp_to_x(draw_end_temp), pct_to_y(draw_end_percent));
        }
        let _ = cr.stroke();

        // Inactive portion of curve (dimmed, from indicator to end)
        if has_temp_source && draw_end_temp < dialog_constants::temperature::MAX_TEMP {
            cr.set_source_rgba(accent.0, accent.1, accent.2, 0.3); // Dimmed
            cr.set_line_width(2.0);

            cr.move_to(temp_to_x(draw_end_temp), pct_to_y(draw_end_percent));
            let mut started = false;
            let mut prev_p = draw_end_percent;
            for (t, p) in points {
                if *t <= draw_end_temp {
                    prev_p = *p;
                    continue;
                }
                if !started {
                    started = true;
                }
                match graph_style.as_str() {
                    "stepped" => {
                        cr.line_to(temp_to_x(*t), pct_to_y(prev_p));
                        cr.line_to(temp_to_x(*t), pct_to_y(*p));
                    }
                    _ => {
                        cr.line_to(temp_to_x(*t), pct_to_y(*p));
                    }
                }
                prev_p = *p;
            }
            if let Some((_, last_p)) = points.last() {
                cr.line_to(temp_to_x(dialog_constants::temperature::MAX_TEMP), pct_to_y(*last_p));
            }
            let _ = cr.stroke();
        }

        // Points - active ones bright, inactive ones dimmed
        for (t, p) in points {
            if *t <= draw_end_temp || !has_temp_source {
                cr.set_source_rgba(accent.0, accent.1, accent.2, 1.0);
            } else {
                cr.set_source_rgba(accent.0, accent.1, accent.2, 0.3);
            }
            cr.arc(temp_to_x(*t), pct_to_y(*p), 4.0, 0.0, 2.0 * std::f64::consts::PI);
            let _ = cr.fill();
        }

        // Current temperature indicator - only show if temp source is selected
        if has_temp_source {
            let indicator_x = temp_to_x(current_temp.clamp(20.0, 100.0));
            let indicator_percent = Self::interpolate_percent(points, current_temp);
            let indicator_y = pct_to_y(indicator_percent);

            // Vertical line - theme-aware
            let ind_line = super::curve_card::theme_colors::indicator_line();
            cr.set_source_rgba(ind_line.0, ind_line.1, ind_line.2, ind_line.3);
            cr.set_line_width(2.0);
            cr.move_to(indicator_x, margin);
            cr.line_to(indicator_x, h - margin);
            let _ = cr.stroke();

            // Indicator dot - theme-aware
            let ind = super::curve_card::theme_colors::indicator();
            cr.set_source_rgba(ind.0, ind.1, ind.2, ind.3);
            cr.arc(indicator_x, indicator_y, 6.0, 0.0, 2.0 * std::f64::consts::PI);
            let _ = cr.fill();

            if is_dark {
                cr.set_source_rgb(1.0, 1.0, 1.0);
            } else {
                cr.set_source_rgb(0.1, 0.1, 0.1);
            }
            cr.set_line_width(2.0);
            cr.arc(indicator_x, indicator_y, 6.0, 0.0, 2.0 * std::f64::consts::PI);
            let _ = cr.stroke();

            // Temperature label - theme-aware
            if is_dark {
                cr.set_source_rgb(1.0, 1.0, 1.0);
            } else {
                cr.set_source_rgb(0.1, 0.1, 0.1);
            }
            cr.set_font_size(12.0);
            cr.move_to(indicator_x + 10.0, indicator_y - 5.0);
            let temp_str = hf_core::display::format_temp_precise(current_temp);
            let fan_str = hf_core::display::format_fan_speed_f32(indicator_percent);
            let _ = cr.show_text(&format!("{} → {}", temp_str, fan_str));
        }
    }

    fn interpolate_percent(points: &[(f32, f32)], temp: f32) -> f32 {
        if points.is_empty() {
            return dialog_constants::fan_speed::MAX_PERCENT;
        }

        if temp <= points[0].0 {
            return points[0].1;
        }

        let last_point = match points.last() {
            Some(p) => p,
            None => return dialog_constants::fan_speed::MAX_PERCENT,
        };
        if temp >= last_point.0 {
            return last_point.1;
        }

        for window in points.windows(2) {
            let (t1, p1) = window[0];
            let (t2, p2) = window[1];
            if temp >= t1 && temp <= t2 {
                let ratio = (temp - t1) / (t2 - t1);
                return p1 + ratio * (p2 - p1);
            }
        }

        100.0
    }

    pub fn connect_create<F: Fn(PairData) + 'static>(&self, callback: F) {
        *self.on_create.borrow_mut() = Some(Box::new(callback));
    }

    pub fn present(&self, parent: &impl gtk4::prelude::IsA<gtk4::Widget>) {
        self.dialog.present(Some(parent));
    }
}

