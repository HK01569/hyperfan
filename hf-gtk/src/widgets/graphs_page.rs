//! Temperature Graphs Page
//!
//! Allows users to create custom temperature monitoring graphs.
//! Each graph tracks a single temperature source with a rolling history.

#![allow(dead_code)]

use gtk4::prelude::*;
use gtk4::{
    cairo, Box as GtkBox, Button, DrawingArea, Entry, Label, ListBox, Orientation, ScrolledWindow,
};
use gtk4::glib;
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::{Duration, Instant};
use tracing::{debug, error, warn};

// ============================================================================
// Constants
// ============================================================================

/// Graph configuration
mod config {
    pub const HISTORY_SIZE: usize = 120;      // 60 seconds at 500ms updates
    pub const UPDATE_INTERVAL_MS: u64 = 500;  // Sensor polling rate
    pub const GRAPH_HEIGHT: i32 = 100;        // Graph widget height
    pub const MARGIN: f64 = 4.0;              // Graph edge margin
    pub const LINE_WIDTH: f64 = 2.0;          // Graph line thickness
    pub const POINT_RADIUS: f64 = 5.0;        // Current value indicator
    pub const AUTO_SCALE_PADDING: f32 = 5.0;  // Extra padding for Y-axis auto-scale (°C)
}

/// Smoothing filter for temperature to eliminate jitter
mod smoothing {
    /// Exponential moving average smoothing factor (0.0-1.0)
    /// Lower = more smoothing, higher = more responsive
    pub const EMA_ALPHA: f32 = 0.3;
    
    /// Apply exponential moving average smoothing
    pub fn ema(current: f32, previous: f32) -> f32 {
        EMA_ALPHA * current + (1.0 - EMA_ALPHA) * previous
    }
}

/// Spline interpolation for smooth curves
mod spline {
    /// Tension parameter for Cardinal spline (0.0 = Catmull-Rom, 1.0 = linear)
    /// Higher tension = less overshoot, more controlled curves
    const TENSION: f64 = 0.5;
    
    /// Compute Cardinal spline interpolation between p1 and p2
    /// using p0 and p3 as control points with tension control.
    /// t is in range [0, 1] where 0 = p1, 1 = p2
    /// Tension of 0.5 prevents overshooting while maintaining smooth curves.
    pub fn cardinal(p0: f64, p1: f64, p2: f64, p3: f64, t: f64) -> f64 {
        let t2 = t * t;
        let t3 = t2 * t;
        
        // Cardinal spline with tension parameter
        // s = (1 - tension) / 2
        let s = (1.0 - TENSION) / 2.0;
        
        // Hermite basis functions with Cardinal tangents
        let h1 = 2.0 * t3 - 3.0 * t2 + 1.0;
        let h2 = -2.0 * t3 + 3.0 * t2;
        let h3 = t3 - 2.0 * t2 + t;
        let h4 = t3 - t2;
        
        // Tangents at p1 and p2
        let m1 = s * (p2 - p0);
        let m2 = s * (p3 - p1);
        
        // Interpolated value
        let result = h1 * p1 + h2 * p2 + h3 * m1 + h4 * m2;
        
        // Clamp to prevent any overshooting beyond the segment endpoints
        let min_val = p1.min(p2);
        let max_val = p1.max(p2);
        result.clamp(min_val, max_val)
    }
    
    /// Compute smooth ease-in-out interpolation (sinusoidal feel)
    pub fn ease_in_out(t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }
}

/// Data for a custom graph
#[derive(Clone)]
pub struct GraphData {
    pub id: String,
    pub name: String,
    pub temp_source_path: String,
    pub temp_source_label: String,
    pub history: VecDeque<f32>,
    pub min_temp: f32,
    pub max_temp: f32,
    /// Smoothed display temperature (for jitter-free animation)
    pub display_temp: f32,
    /// Animation state: previous history snapshot for interpolation
    prev_history: VecDeque<f32>,
    /// Timestamp when animation started (for time-based interpolation)
    anim_start: Instant,
    /// Animation duration in milliseconds
    anim_duration_ms: u64,
    /// Previous display temp for label interpolation
    prev_display_temp: f32,
}

impl GraphData {
    /// Create a new graph with the given configuration
    fn new(id: String, name: String, path: String, label: String) -> Self {
        Self {
            id,
            name,
            temp_source_path: path,
            temp_source_label: label,
            history: VecDeque::with_capacity(config::HISTORY_SIZE),
            min_temp: 20.0,
            max_temp: 100.0,
            display_temp: 0.0,
            prev_history: VecDeque::with_capacity(config::HISTORY_SIZE),
            anim_start: Instant::now(),
            anim_duration_ms: 500, // Default, will be set by poll interval
            prev_display_temp: 0.0,
        }
    }

    /// Add a temperature reading to the history with smoothing
    fn push_temp(&mut self, temp: f32, anim_duration_ms: u64) {
        // Snapshot current state for animation interpolation
        self.prev_history = self.history.clone();
        self.prev_display_temp = self.display_temp;
        self.anim_start = Instant::now(); // Start animation from now
        self.anim_duration_ms = anim_duration_ms;
        
        // Apply EMA smoothing to eliminate jitter
        let smoothed = if self.display_temp == 0.0 {
            temp // First reading, no smoothing
        } else {
            smoothing::ema(temp, self.display_temp)
        };
        self.display_temp = smoothed;
        
        // Remove oldest entry if at capacity
        if self.history.len() >= config::HISTORY_SIZE {
            self.history.pop_front();
        }
        self.history.push_back(smoothed);
        
        // Auto-scale Y axis based on data range
        self.update_scale();
    }
    
    /// Get animation progress based on wall-clock time (0.0 to 1.0)
    fn anim_progress(&self) -> f32 {
        let elapsed = self.anim_start.elapsed().as_millis() as f32;
        let duration = self.anim_duration_ms as f32;
        (elapsed / duration).min(1.0)
    }
    
    /// Check if animation is still in progress
    fn is_animating(&self) -> bool {
        self.anim_start.elapsed().as_millis() < self.anim_duration_ms as u128
    }
    
    /// Get interpolated display temperature for smooth label updates
    fn interpolated_display_temp(&self) -> f32 {
        let t = spline::ease_in_out(self.anim_progress() as f64) as f32;
        self.prev_display_temp + (self.display_temp - self.prev_display_temp) * t
    }
    
    /// Get interpolated temperature at history index for graph rendering
    /// Interpolation style follows graph_style setting:
    /// - "stepped": No interpolation (instant jump at end)
    /// - "line"/"filled": Linear interpolation
    /// - "smoothed": Ease-in-out interpolation (when graph_smoothing == "smoothed")
    fn interpolated_temp_at(&self, index: usize, _graph_style: &str, graph_smoothing: &str) -> Option<f32> {
        let current = self.history.get(index).copied();
        
        // If animation complete or no previous data, return current
        let progress = self.anim_progress();
        if progress >= 1.0 || self.prev_history.is_empty() {
            return current;
        }
        
        // Map current index to previous history (accounting for shift)
        // When new data arrives, history shifts left by 1
        let prev_index = if self.prev_history.len() == self.history.len() {
            // Same size: previous[i+1] maps to current[i] (data shifted left)
            index.checked_add(1).filter(|&i| i < self.prev_history.len())
        } else if self.prev_history.len() < self.history.len() {
            // Growing: direct mapping
            Some(index).filter(|&i| i < self.prev_history.len())
        } else {
            Some(index)
        };
        
        let prev_val = prev_index.and_then(|i| self.prev_history.get(i).copied());
        
        match (current, prev_val) {
            (Some(curr), Some(prev)) => {
                // Animation interpolation style based on settings
                // Note: graph_style affects RENDERING (line vs stepped drawing)
                // graph_smoothing affects ANIMATION (linear vs ease-in-out transition)
                let t = if graph_smoothing == "smoothed" {
                    // Ease-in-out for smooth sinusoidal feel
                    spline::ease_in_out(progress as f64) as f32
                } else {
                    // Linear interpolation
                    progress
                };
                Some(prev + (curr - prev) * t)
            }
            (Some(curr), None) => Some(curr),
            (None, _) => None,
        }
    }

    /// Update min/max scale based on current history
    fn update_scale(&mut self) {
        let min = self.history.iter().copied().fold(f32::INFINITY, f32::min);
        let max = self.history.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        
        if min.is_finite() && max.is_finite() {
            self.min_temp = (min - 5.0).max(0.0);
            self.max_temp = (max + 5.0).min(120.0);
        }
    }
}

/// A single graph card widget
struct GraphCard {
    card: adw::Bin,
    drawing_area: DrawingArea,
    data: Rc<RefCell<GraphData>>,
    current_temp_label: Label,
    range_label: Label,
}

impl GraphCard {
    fn new_with_delete<F>(data: GraphData, on_delete: F) -> Self 
    where
        F: Fn(String) + 'static,
    {
        let on_delete = Rc::new(on_delete);
        let data = Rc::new(RefCell::new(data));

        let card = adw::Bin::builder()
            .css_classes(["card"])
            .build();

        let content = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(6)
            .margin_start(12)
            .margin_end(12)
            .margin_top(12)
            .margin_bottom(12)
            .build();

        // Header
        let header = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .build();

        let name_label = Label::builder()
            .label(&data.borrow().name)
            .css_classes(["title-3"])
            .hexpand(true)
            .halign(gtk4::Align::Start)
            .build();

        let temp_placeholder = format!("--{}", hf_core::display::temp_unit_suffix());
        let current_temp_label = Label::builder()
            .label(&temp_placeholder)
            .css_classes(["title-2", "numeric"])
            .build();
        
        // Temperature range legend (min/max)
        let range_label = Label::builder()
            .label("--")
            .css_classes(["caption", "dim-label"])
            .tooltip_text("Temperature range (min-max)")
            .build();

        // Export menu button
        let export_btn = gtk4::MenuButton::builder()
            .icon_name("document-save-symbolic")
            .css_classes(["flat", "circular"])
            .tooltip_text("Export data")
            .valign(gtk4::Align::Center)
            .build();

        let export_menu = gtk4::gio::Menu::new();
        export_menu.append(Some("Export as JSON..."), Some("graph.export-json"));
        export_menu.append(Some("Copy last 60s as JSON"), Some("graph.copy-json"));
        export_menu.append(Some("Copy last 60s as CSV"), Some("graph.copy-csv"));
        export_btn.set_menu_model(Some(&export_menu));

        // Delete button
        let delete_btn = Button::builder()
            .icon_name("user-trash-symbolic")
            .css_classes(["flat", "circular"])
            .tooltip_text("Remove graph")
            .valign(gtk4::Align::Center)
            .build();

        let graph_id = data.borrow().id.clone();
        let graph_name = data.borrow().name.clone();
        delete_btn.connect_clicked(move |btn| {
            let confirm = libadwaita::AlertDialog::builder()
                .heading("Remove Graph?")
                .body(&format!("Are you sure you want to remove \"{}\"?", graph_name))
                .build();

            confirm.add_response("cancel", "Cancel");
            confirm.add_response("remove", "Remove");
            confirm.set_response_appearance("remove", libadwaita::ResponseAppearance::Destructive);
            confirm.set_default_response(Some("cancel"));
            confirm.set_close_response("cancel");

            let graph_id = graph_id.clone();
            let on_delete = on_delete.clone();

            if let Some(root) = btn.root() {
                if let Some(window) = root.downcast_ref::<gtk4::Window>() {
                    confirm.choose(window, None::<&gtk4::gio::Cancellable>, move |response| {
                        if response == "remove" {
                            on_delete(graph_id.clone());
                        }
                    });
                }
            }
        });

        // Setup export actions
        let action_group = gtk4::gio::SimpleActionGroup::new();
        
        let data_for_json = data.clone();
        let export_json_action = gtk4::gio::SimpleAction::new("export-json", None);
        let export_btn_for_json = export_btn.clone();
        export_json_action.connect_activate(move |_, _| {
            Self::export_json_file(&data_for_json.borrow(), &export_btn_for_json);
        });
        action_group.add_action(&export_json_action);

        let data_for_copy_json = data.clone();
        let copy_json_action = gtk4::gio::SimpleAction::new("copy-json", None);
        let export_btn_for_copy = export_btn.clone();
        copy_json_action.connect_activate(move |_, _| {
            Self::copy_json_to_clipboard(&data_for_copy_json.borrow(), &export_btn_for_copy);
        });
        action_group.add_action(&copy_json_action);

        let data_for_csv = data.clone();
        let copy_csv_action = gtk4::gio::SimpleAction::new("copy-csv", None);
        let export_btn_for_csv = export_btn.clone();
        copy_csv_action.connect_activate(move |_, _| {
            Self::copy_csv_to_clipboard(&data_for_csv.borrow(), &export_btn_for_csv);
        });
        action_group.add_action(&copy_csv_action);

        card.insert_action_group("graph", Some(&action_group));

        header.append(&name_label);
        header.append(&range_label);
        header.append(&current_temp_label);
        header.append(&export_btn);
        header.append(&delete_btn);

        // Subtitle
        let subtitle = Label::builder()
            .label(&data.borrow().temp_source_label)
            .css_classes(["dim-label", "caption"])
            .halign(gtk4::Align::Start)
            .build();

        // Graph
        let drawing_area = DrawingArea::builder()
            .height_request(120)
            .hexpand(true)
            .build();

        let data_for_draw = data.clone();
        let paused_for_draw = Rc::new(RefCell::new(false));
        drawing_area.set_draw_func(move |_, cr, width, height| {
            let is_paused = *paused_for_draw.borrow();
            Self::draw_graph(cr, width, height, &data_for_draw.borrow(), is_paused);
        });

        content.append(&header);
        content.append(&subtitle);
        content.append(&drawing_area);

        card.set_child(Some(&content));

        Self {
            card,
            data,
            drawing_area,
            current_temp_label,
            range_label,
        }
    }

    fn new(data: GraphData) -> Self {
        Self::new_with_delete(data, |_| {})
    }

    /// Update sensor data only (phase 1 of synchronized update)
    fn update_data(&self, cached: Option<&crate::runtime::SensorData>, anim_duration_ms: u64) {
        let path = self.data.borrow().temp_source_path.clone();
        
        let temp_result: Option<f32> = if path.starts_with("gpu:") {
            // GPU temperature - check cache first
            Self::read_gpu_temp_cached(&path, cached)
                .or_else(|| Self::read_gpu_temp(&path).ok())
        } else {
            // hwmon temperature - check cache first
            cached
                .and_then(|data| {
                    data.temperatures.iter()
                        .find(|t| t.path == path)
                        .map(|t| t.temp_celsius)
                })
                .or_else(|| hf_core::daemon_client::daemon_read_temperature(&path).ok())
        };

        if let Some(temp) = temp_result {
            let mut data = self.data.borrow_mut();
            data.push_temp(temp, anim_duration_ms);
        }
    }

    /// Refresh UI elements (phase 2 of synchronized update)
    fn refresh_ui(&self) {
        let data = self.data.borrow();
        let display_temp = data.interpolated_display_temp();
        
        // Update range label with min-max
        if !data.history.is_empty() {
            let min_temp = data.history.iter().copied().fold(f32::INFINITY, f32::min);
            let max_temp = data.history.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            let range_text = format!("{:.0}-{:.0}°C", min_temp, max_temp);
            if self.range_label.text() != range_text {
                self.range_label.set_label(&range_text);
            }
        }
        
        drop(data);
        
        if display_temp > 0.0 {
            self.current_temp_label.set_label(&hf_core::display::format_temp_precise(display_temp));
        }
        self.drawing_area.queue_draw();
    }
    
    /// Tick animation forward with frame time for precise interpolation
    /// Returns true if still animating (for optimization)
    fn tick_animation(&self, frame_time: i64) -> bool {
        let data = self.data.borrow();
        let is_animating = data.is_animating();
        let display_temp = data.interpolated_display_temp();
        drop(data);
        
        // Only redraw if animation is in progress
        if is_animating {
            if display_temp > 0.0 {
                self.current_temp_label.set_label(&hf_core::display::format_temp_precise(display_temp));
            }
            self.drawing_area.queue_draw();
        }
        
        is_animating
    }

    /// Read GPU temperature from cached runtime data (non-blocking)
    fn read_gpu_temp_cached(path: &str, cached: Option<&crate::runtime::SensorData>) -> Option<f32> {
        let parts: Vec<&str> = path.split(':').collect();
        if parts.len() < 3 {
            // Fallback: if only gpu:<index>, use default temp
            if parts.len() == 2 {
                let gpu_index: u32 = parts[1].parse().ok()?;
                return cached?.gpus.iter()
                    .find(|g| g.index == gpu_index)
                    .and_then(|g| g.temp);
            }
            return None;
        }
        
        let gpu_index: u32 = parts[1].parse().ok()?;
        let temp_name = parts[2];
        
        cached?.gpus.iter()
            .find(|g| g.index == gpu_index)
            .and_then(|g| g.temperatures.get(temp_name).copied())
    }

    /// Read temperature from a GPU source path (blocking fallback)
    fn read_gpu_temp(path: &str) -> anyhow::Result<f32> {
        // Parse "gpu:<index>:<temp_name>" format
        let parts: Vec<&str> = path.split(':').collect();
        if parts.len() != 3 {
            anyhow::bail!("Invalid GPU path format");
        }

        let gpu_index: u32 = parts[1].parse()?;
        let temp_name = parts[2];

        // Daemon authoritative: GPU fallback uses daemon list (limited fields).
        let gpus = hf_core::daemon_list_gpus().map_err(|e| anyhow::anyhow!(e))?;
        let gpu = gpus
            .iter()
            .find(|g| g.index == gpu_index)
            .ok_or_else(|| anyhow::anyhow!("GPU {} not found", gpu_index))?;

        if temp_name == "GPU" {
            gpu.temp.ok_or_else(|| anyhow::anyhow!("Temperature not available"))
        } else {
            anyhow::bail!("Temperature sensor not available via daemon: {}", temp_name);
        }
    }

    fn draw_graph(cr: &cairo::Context, width: i32, height: i32, data: &GraphData, is_paused: bool) {
        let w = width as f64;
        let h = height as f64;
        let margin = 4.0;

        // PERFORMANCE: Use cached settings (no disk I/O in draw function)
        let graph_style = hf_core::get_graph_style();
        let graph_smoothing = hf_core::get_graph_smoothing();

        // Background
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
        cr.rectangle(0.0, 0.0, w, h);
        let _ = cr.fill();

        // Grid - use theme colors
        let grid = super::curve_card::theme_colors::grid_line();
        cr.set_source_rgba(grid.0, grid.1, grid.2, grid.3);
        cr.set_line_width(1.0);
        for i in 0..=4 {
            let y = margin + (i as f64 / 4.0) * (h - 2.0 * margin);
            cr.move_to(margin, y);
            cr.line_to(w - margin, y);
        }
        let _ = cr.stroke();

        if data.history.is_empty() {
            return;
        }

        // Auto-scale Y-axis based on actual data range with padding
        let data_min = data.history.iter().copied().fold(f32::INFINITY, f32::min);
        let data_max = data.history.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        
        let scale_min = (data_min - config::AUTO_SCALE_PADDING).max(0.0);
        let scale_max = (data_max + config::AUTO_SCALE_PADDING).min(150.0);
        let temp_range = scale_max - scale_min;
        
        if temp_range < 0.1 {
            return;
        }

        let temp_to_y = |t: f32| {
            h - margin - ((t - scale_min) / temp_range) as f64 * (h - 2.0 * margin)
        };

        let len = data.history.len();
        let x_step = (w - 2.0 * margin) / (config::HISTORY_SIZE - 1) as f64;
        let last_x = margin + (config::HISTORY_SIZE - 1) as f64 * x_step;
        
        // Collect interpolated Y values for all points (respects graph style)
        let y_values: Vec<f64> = (0..len)
            .map(|i| {
                let temp = data.interpolated_temp_at(i, &graph_style, &graph_smoothing)
                    .unwrap_or_else(|| data.history[i]);
                temp_to_y(temp)
            })
            .collect();

        // Fill (only for "filled" style) - use accent color
        let accent = super::curve_card::theme_colors::curve_line();
        if graph_style == "filled" {
            cr.set_source_rgba(accent.0, accent.1, accent.2, 0.2);
            cr.move_to(margin, h - margin);

            for (i, &y) in y_values.iter().enumerate() {
                let x = margin + (config::HISTORY_SIZE - len + i) as f64 * x_step;
                cr.line_to(x, y);
            }

            cr.line_to(last_x, h - margin);
            cr.close_path();
            let _ = cr.fill();
        }

        // Line - use accent color
        cr.set_source_rgba(accent.0, accent.1, accent.2, 1.0);
        cr.set_line_width(2.0);

        if graph_smoothing == "smoothed" && len >= 2 {
            // Catmull-Rom spline for sinusoidal curves through all points
            Self::draw_catmull_rom_spline(cr, margin, x_step, len, &y_values);
        } else {
            // Non-smoothed rendering
            let mut first = true;
            let mut prev_y = 0.0;
            
            for (i, &y) in y_values.iter().enumerate() {
                let x = margin + (config::HISTORY_SIZE - len + i) as f64 * x_step;

                if first {
                    cr.move_to(x, y);
                    first = false;
                } else {
                    match graph_style.as_str() {
                        "stepped" => {
                            // Step function: horizontal then vertical
                            cr.line_to(x, prev_y);
                            cr.line_to(x, y);
                        }
                        _ => {
                            // Direct straight line
                            cr.line_to(x, y);
                        }
                    }
                }
                prev_y = y;
            }
        }
        let _ = cr.stroke();

        // Current value dot (use interpolated temp)
        let last_temp = data.interpolated_display_temp();
        if last_temp > 0.0 {
            let x = last_x;
            let y = temp_to_y(last_temp);

            cr.set_source_rgba(accent.0, accent.1, accent.2, 1.0);
            cr.arc(x, y, 5.0, 0.0, 2.0 * std::f64::consts::PI);
            let _ = cr.fill();

            cr.set_source_rgba(1.0, 1.0, 1.0, 1.0);
            cr.set_line_width(2.0);
            cr.arc(x, y, 5.0, 0.0, 2.0 * std::f64::consts::PI);
            let _ = cr.stroke();

            // Current value indicator
            let indicator = super::curve_card::theme_colors::indicator();
            cr.set_source_rgba(indicator.0, indicator.1, indicator.2, indicator.3);
            cr.arc(last_x, y, config::POINT_RADIUS, 0.0, 2.0 * std::f64::consts::PI);
            let _ = cr.fill();

            // Draw PAUSED overlay if paused
            if is_paused {
                // Semi-transparent overlay
                cr.set_source_rgba(0.0, 0.0, 0.0, 0.3);
                cr.rectangle(0.0, 0.0, w, h);
                let _ = cr.fill();
                
                // PAUSED text
                cr.set_source_rgba(1.0, 1.0, 1.0, 0.9);
                cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
                cr.set_font_size(14.0);
                
                let text = "PAUSED";
                let extents = cr.text_extents(text).unwrap();
                let text_x = (w - extents.width()) / 2.0;
                let text_y = h / 2.0;
                
                cr.move_to(text_x, text_y);
                let _ = cr.show_text(text);
            }
        }
    }
    
    /// Draw a Catmull-Rom spline through all data points for smooth sinusoidal curves
    fn draw_catmull_rom_spline(
        cr: &cairo::Context,
        margin: f64,
        x_step: f64,
        len: usize,
        y_values: &[f64],
    ) {
        if y_values.is_empty() {
            return;
        }
        
        // Number of interpolation segments between each pair of points
        const SEGMENTS_PER_POINT: usize = 8;
        
        let x_offset = config::HISTORY_SIZE - len;
        
        // Start at first point
        let first_x = margin + x_offset as f64 * x_step;
        cr.move_to(first_x, y_values[0]);
        
        // For each segment between points
        for i in 0..len.saturating_sub(1) {
            // Get 4 control points for Catmull-Rom (p0, p1, p2, p3)
            // p1 and p2 are the segment endpoints, p0 and p3 are neighbors
            let p0 = if i == 0 { y_values[0] } else { y_values[i - 1] };
            let p1 = y_values[i];
            let p2 = y_values[i + 1];
            let p3 = if i + 2 < len { y_values[i + 2] } else { y_values[len - 1] };
            
            let x1 = margin + (x_offset + i) as f64 * x_step;
            let x2 = margin + (x_offset + i + 1) as f64 * x_step;
            
            // Interpolate between p1 and p2
            for seg in 1..=SEGMENTS_PER_POINT {
                let t = seg as f64 / SEGMENTS_PER_POINT as f64;
                let x = x1 + (x2 - x1) * t;
                let y = spline::cardinal(p0, p1, p2, p3, t);
                cr.line_to(x, y);
            }
        }
    }

    fn widget(&self) -> &adw::Bin {
        &self.card
    }

    /// Export graph data to JSON file
    fn export_json_file(data: &GraphData, btn: &gtk4::MenuButton) {
        let json = Self::generate_json(data);
        
        let dialog = gtk4::FileDialog::builder()
            .title("Export Graph Data")
            .initial_name(&format!("{}.json", data.name.replace(' ', "_")))
            .build();

        if let Some(window) = btn.root().and_then(|r| r.downcast::<gtk4::Window>().ok()) {
            dialog.save(Some(&window), gtk4::gio::Cancellable::NONE, move |result| {
                if let Ok(file) = result {
                    if let Some(path) = file.path() {
                        if let Err(e) = std::fs::write(&path, &json) {
                            error!("Failed to write JSON file: {}", e);
                        } else {
                            debug!("Exported graph data to {:?}", path);
                        }
                    }
                }
            });
        }
    }

    /// Copy graph data as JSON to clipboard
    fn copy_json_to_clipboard(data: &GraphData, btn: &gtk4::MenuButton) {
        let json = Self::generate_json(data);
        
        let display = btn.display();
        let clipboard = display.clipboard();
        clipboard.set_text(&json);
        debug!("Copied {} bytes of JSON to clipboard", json.len());
    }

    /// Copy graph data as CSV to clipboard
    fn copy_csv_to_clipboard(data: &GraphData, btn: &gtk4::MenuButton) {
        let csv = Self::generate_csv(data);
        
        let display = btn.display();
        let clipboard = display.clipboard();
        clipboard.set_text(&csv);
        debug!("Copied {} bytes of CSV to clipboard", csv.len());
    }

    /// Generate JSON representation of graph data
    fn generate_json(data: &GraphData) -> String {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        
        let interval_ms = config::UPDATE_INTERVAL_MS;
        let points: Vec<_> = data.history.iter().enumerate().map(|(i, &temp)| {
            let offset_ms = (data.history.len() - 1 - i) as u64 * interval_ms;
            serde_json::json!({
                "offset_ms": -(offset_ms as i64),
                "temperature": temp
            })
        }).collect();

        let json_obj = serde_json::json!({
            "name": data.name,
            "source": data.temp_source_label,
            "source_path": data.temp_source_path,
            "exported_at": now,
            "interval_ms": interval_ms,
            "min_temp": data.min_temp,
            "max_temp": data.max_temp,
            "current_temp": data.display_temp,
            "points": points
        });

        serde_json::to_string_pretty(&json_obj).unwrap_or_else(|_| "{}".to_string())
    }

    /// Generate CSV representation of graph data
    fn generate_csv(data: &GraphData) -> String {
        let mut csv = String::from("offset_ms,temperature\n");
        let interval_ms = config::UPDATE_INTERVAL_MS;
        
        for (i, &temp) in data.history.iter().enumerate() {
            let offset_ms = (data.history.len() - 1 - i) as u64 * interval_ms;
            csv.push_str(&format!("-{},{:.2}\n", offset_ms, temp));
        }
        
        csv
    }
}

pub struct GraphsPage {
    container: GtkBox,
    graphs: Rc<RefCell<Vec<GraphCard>>>,
    cards_box: GtkBox,
    empty_state: adw::StatusPage,
    stack: gtk4::Stack,
    paused: Rc<RefCell<bool>>,
}

impl GraphsPage {
    pub fn new() -> Self {
        let container = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .build();

        // Header - HIG: consistent 24px margins
        let header_box = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .margin_start(24)
            .margin_end(24)
            .margin_top(24)
            .margin_bottom(12)
            .build();

        let title = Label::builder()
            .label("Temperature Graphs")
            .css_classes(["title-1"])
            .hexpand(true)
            .halign(gtk4::Align::Start)
            .build();

        let pause_btn = Button::builder()
            .icon_name("media-playback-pause-symbolic")
            .css_classes(["circular", "flat"])
            .tooltip_text("Pause graph updates")
            .build();

        let add_button = Button::builder()
            .icon_name("list-add-symbolic")
            .css_classes(["fab", "suggested-action"])
            .tooltip_text("Add Graph")
            .build();

        header_box.append(&title);
        header_box.append(&pause_btn);
        header_box.append(&add_button);
        container.append(&header_box);

        // Scrollable cards
        let scroll = ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .build();

        let cards_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(12)
            .margin_start(24)
            .margin_end(24)
            .margin_top(0)
            .margin_bottom(24)
            .build();

        scroll.set_child(Some(&cards_box));

        // Stack for empty vs list state
        let stack = gtk4::Stack::builder()
            .transition_type(gtk4::StackTransitionType::Crossfade)
            .transition_duration(150)
            .build();

        // Empty state
        let empty_state = adw::StatusPage::builder()
            .icon_name("utilities-system-monitor-symbolic")
            .title("No Temperature Graphs")
            .description("Create custom graphs to monitor temperature trends over time.")
            .build();

        let empty_add_btn = Button::builder()
            .label("Create Your First Graph")
            .css_classes(["suggested-action", "pill"])
            .halign(gtk4::Align::Center)
            .build();
        empty_state.set_child(Some(&empty_add_btn));

        stack.add_named(&empty_state, Some("empty"));
        stack.add_named(&scroll, Some("list"));
        container.append(&stack);

        // Add Ctrl+N keyboard shortcut to add graph
        let key_controller = gtk4::EventControllerKey::new();
        let cards_for_keys = cards_box.clone();
        let graphs_for_keys = Rc::new(RefCell::new(Vec::new()));
        let stack_for_keys = stack.clone();
        key_controller.connect_key_pressed(move |_, key, _, modifiers| {
            if modifiers.contains(gtk4::gdk::ModifierType::CONTROL_MASK) {
                if matches!(key, gtk4::gdk::Key::n | gtk4::gdk::Key::N) {
                    Self::show_add_dialog(&cards_for_keys, &graphs_for_keys, &stack_for_keys);
                    return glib::Propagation::Stop;
                }
            }
            glib::Propagation::Proceed
        });
        container.add_controller(key_controller);

        let graphs: Rc<RefCell<Vec<GraphCard>>> = Rc::new(RefCell::new(Vec::new()));

        let paused = Rc::new(RefCell::new(false));
        
        // Clone graphs before moving into Self
        let graphs_for_empty = graphs.clone();
        
        let this = Self {
            container,
            graphs,
            cards_box: cards_box.clone(),
            empty_state,
            stack: stack.clone(),
            paused: paused.clone(),
        };

        // Wire up empty state add button
        let cards_box_for_empty = cards_box.clone();
        let stack_for_empty = stack.clone();
        empty_add_btn.connect_clicked(move |_| {
            Self::show_add_dialog(&cards_box_for_empty, &graphs_for_empty, &stack_for_empty);
        });

        // Wire up pause/play button
        let paused_for_btn = paused.clone();
        pause_btn.connect_clicked(move |btn| {
            let mut is_paused = paused_for_btn.borrow_mut();
            *is_paused = !*is_paused;
            
            if *is_paused {
                btn.set_icon_name("media-playback-start-symbolic");
                btn.set_tooltip_text(Some("Resume graph updates"));
            } else {
                btn.set_icon_name("media-playback-pause-symbolic");
                btn.set_tooltip_text(Some("Pause graph updates"));
            }
        });

        // Add button handler
        let cards_box_for_add = this.cards_box.clone();
        let graphs_for_add = this.graphs.clone();
        let stack_for_add = this.stack.clone();

        add_button.connect_clicked(move |_| {
            Self::show_add_dialog(&cards_box_for_add, &graphs_for_add, &stack_for_add);
        });

        // Setup live updates
        this.setup_live_updates();

        this
    }

    /// Update stack visibility based on whether graphs exist
    fn update_stack_visibility(&self) {
        if self.graphs.borrow().is_empty() {
            self.stack.set_visible_child_name("empty");
        } else {
            self.stack.set_visible_child_name("list");
        }
    }

    /// Load saved graphs from temp_graphs.json
    fn load_saved_graphs(&self) {
        match hf_core::load_temp_graphs() {
            Ok(saved_graphs) => {
                for saved in saved_graphs {
                    let data = GraphData::new(
                        saved.id,
                        saved.name,
                        saved.temp_source_path,
                        saved.temp_source_label,
                    );
                    let cards_box = self.cards_box.clone();
                    let graphs = self.graphs.clone();
                    let stack = self.stack.clone();
                    let card = GraphCard::new_with_delete(data, move |graph_id| {
                        Self::delete_graph(&cards_box, &graphs, &stack, &graph_id);
                    });
                    self.cards_box.append(card.widget());
                    self.graphs.borrow_mut().push(card);
                }
                debug!("Loaded {} saved temperature graphs", self.graphs.borrow().len());
            }
            Err(e) => {
                warn!("Failed to load saved graphs: {}", e);
            }
        }
    }

    /// Delete a graph by ID (removes from UI, list, and disk)
    fn delete_graph(cards_box: &GtkBox, graphs: &Rc<RefCell<Vec<GraphCard>>>, stack: &gtk4::Stack, graph_id: &str) {
        // Remove from disk
        if let Err(e) = hf_core::remove_temp_graph(graph_id) {
            error!("Failed to remove graph from disk: {}", e);
        }

        // Find and remove from UI and list
        let mut graphs_mut = graphs.borrow_mut();
        if let Some(idx) = graphs_mut.iter().position(|c| c.data.borrow().id == graph_id) {
            let card = graphs_mut.remove(idx);
            cards_box.remove(card.widget());
            debug!("Deleted graph: {}", graph_id);
        }

        // Update stack visibility
        if graphs_mut.is_empty() {
            stack.set_visible_child_name("empty");
        }
    }

    fn show_add_dialog(cards_box: &GtkBox, graphs: &Rc<RefCell<Vec<GraphCard>>>, stack: &gtk4::Stack) {
        let dialog = adw::Window::builder()
            .title("Add Temperature Graph")
            .default_width(400)
            .default_height(400)
            .modal(true)
            .build();

        let content = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(18)
            .margin_start(24)
            .margin_end(24)
            .margin_top(18)
            .margin_bottom(24)
            .build();

        let header = adw::HeaderBar::builder()
            .show_end_title_buttons(false)
            .show_start_title_buttons(false)
            .build();

        let cancel_btn = Button::builder()
            .label("Cancel")
            .build();

        let add_btn = Button::builder()
            .label("Add")
            .css_classes(["suggested-action"])
            .build();

        header.pack_start(&cancel_btn);
        header.pack_end(&add_btn);

        // Name entry
        let name_group = adw::PreferencesGroup::builder()
            .title("Graph Name")
            .build();

        let name_entry = Entry::builder()
            .placeholder_text("My Temperature Graph")
            .activates_default(true)
            .build();

        let name_row = adw::ActionRow::builder()
            .title("Name")
            .build();
        name_row.add_suffix(&name_entry);
        name_group.add(&name_row);

        content.append(&name_group);

        // Temperature source list
        let source_group = adw::PreferencesGroup::builder()
            .title("Temperature Source")
            .build();

        let source_scroll = ScrolledWindow::builder()
            .height_request(200)
            .build();

        let source_list = ListBox::builder()
            .css_classes(["boxed-list"])
            .build();

        // Populate sources
        let selected_source: Rc<RefCell<Option<(String, String)>>> = Rc::new(RefCell::new(None));

        // Add GPU temperature sources first via daemon (authoritative)
        if let Ok(daemon_gpus) = hf_core::daemon_list_gpus() {
            for gpu in daemon_gpus {
                // Use GPU temp from daemon response
                if let Some(temp) = gpu.temp {
                    let display = format!("{} - GPU", gpu.name);
                    let path = format!("gpu:{}:GPU", gpu.index);
                    let temp_str = format!("{:.1}°C", temp);

                    let row = adw::ActionRow::builder()
                        .title(&display)
                        .subtitle(&format!("{} GPU", gpu.vendor))
                        .activatable(true)
                        .build();

                    let path_clone = path.clone();
                    let label_clone = display.clone();
                    let selected = selected_source.clone();

                    row.connect_activated(move |r| {
                        *selected.borrow_mut() = Some((path_clone.clone(), label_clone.clone()));
                        r.add_css_class("selected");
                    });

                    source_list.append(&row);
                }
            }
        }

        // Add hwmon temperature sources via daemon (authoritative)
        if let Ok(hw) = hf_core::daemon_list_hardware() {
            for chip in hw.chips {
                // Skip amdgpu chips as they're in GPU section
                if chip.name.contains("amdgpu") {
                    continue;
                }

                for temp in &chip.temperatures {
                    let label = temp.label.clone().unwrap_or_else(|| temp.name.clone());
                    let display = format!("{} - {}", chip.name, label);
                    let path = temp.path.clone();

                    let row = adw::ActionRow::builder()
                        .title(&display)
                        .subtitle(&path)
                        .activatable(true)
                        .build();

                    let path_clone = path.clone();
                    let label_clone = display.clone();
                    let selected = selected_source.clone();

                    row.connect_activated(move |r| {
                        *selected.borrow_mut() = Some((path_clone.clone(), label_clone.clone()));
                        r.add_css_class("selected");
                    });

                    source_list.append(&row);
                }
            }
        }

        source_scroll.set_child(Some(&source_list));
        source_group.add(&source_scroll);
        content.append(&source_group);

        let main_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .build();
        main_box.append(&header);
        main_box.append(&content);

        dialog.set_content(Some(&main_box));

        // Cancel
        let dialog_for_cancel = dialog.clone();
        cancel_btn.connect_clicked(move |_| {
            dialog_for_cancel.close();
        });

        // Add
        let dialog_for_add = dialog.clone();
        let cards_box_clone = cards_box.clone();
        let graphs_clone = graphs.clone();
        let stack_clone = stack.clone();

        add_btn.connect_clicked(move |_| {
            let name = name_entry.text().to_string();
            let name = if name.is_empty() { "Temperature Graph".to_string() } else { name };

            if let Some((path, label)) = selected_source.borrow().clone() {
                let id = format!("graph_{}", std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis());

                // Save to disk
                let persisted = hf_core::PersistedGraph {
                    id: id.clone(),
                    name: name.clone(),
                    temp_source_path: path.clone(),
                    temp_source_label: label.clone(),
                };
                if let Err(e) = hf_core::add_temp_graph(persisted) {
                    tracing::error!("Failed to save temperature graph: {}", e);
                }

                let data = GraphData::new(id, name, path, label);
                let cards_box_for_delete = cards_box_clone.clone();
                let graphs_for_delete = graphs_clone.clone();
                let stack_for_delete = stack_clone.clone();
                let card = GraphCard::new_with_delete(data, move |graph_id| {
                    Self::delete_graph(&cards_box_for_delete, &graphs_for_delete, &stack_for_delete, &graph_id);
                });

                cards_box_clone.append(card.widget());
                graphs_clone.borrow_mut().push(card);

                // Switch to list view
                stack_clone.set_visible_child_name("list");

                dialog_for_add.close();
            }
        });

        dialog.present();
    }

    fn setup_live_updates(&self) {
        let graphs = self.graphs.clone();
        let container = self.container.clone();
        let paused = self.paused.clone();
        
        // Use user-configured poll interval from settings
        let poll_interval_ms = hf_core::get_cached_settings().general.poll_interval_ms as u64;
        let poll_interval_ms = poll_interval_ms.max(50); // Minimum 50ms for safety

        // Data poll timer - fetches new sensor data at user-configured rate
        glib::timeout_add_local(Duration::from_millis(poll_interval_ms), move || {
            // PERFORMANCE: Only update if this page is visible (mapped to screen)
            if !container.is_mapped() {
                return glib::ControlFlow::Continue;
            }
            
            // Skip updates if paused
            if *paused.borrow() {
                return glib::ControlFlow::Continue;
            }
            
            // Fetch sensor data once for all graphs
            let cached = crate::runtime::get_sensors();
            let graphs_ref = graphs.borrow();
            
            // Phase 1: Update all graph data synchronously (triggers animation reset)
            // Animation duration = poll interval so animation completes before next data
            for card in graphs_ref.iter() {
                card.update_data(cached.as_ref(), poll_interval_ms);
            }
            
            // Phase 2: Immediately refresh UI at poll rate (synchronized update)
            for card in graphs_ref.iter() {
                card.refresh_ui();
            }
            
            glib::ControlFlow::Continue
        });
        
        // WORLD-CLASS ANIMATION ENGINE: VSYNC-synchronized tick callback
        // Features:
        // - Frame-perfect timing using GTK frame clock (no drift)
        // - Respects user's FPS setting from preferences
        // - Precise frame pacing with microsecond accuracy
        // - Automatic throttling to target FPS
        // - Zero tearing, zero jitter
        // - Only renders when animations are active (power efficient)
        let graphs_anim = self.graphs.clone();
        let last_render_time: Rc<RefCell<Option<i64>>> = Rc::new(RefCell::new(None));
        
        self.container.add_tick_callback(move |widget, frame_clock| {
            // PERFORMANCE: Only animate if this page is visible
            if !widget.is_mapped() {
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
            
            let graphs_ref = graphs_anim.borrow();
            
            // Tick animation for all graphs
            for card in graphs_ref.iter() {
                card.tick_animation(frame_time);
            }
            
            glib::ControlFlow::Continue
        });
    }

    pub fn widget(&self) -> &GtkBox {
        &self.container
    }
}

impl Default for GraphsPage {
    fn default() -> Self {
        Self::new()
    }
}
