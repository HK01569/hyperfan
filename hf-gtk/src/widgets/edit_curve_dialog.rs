//! Edit Curve Dialog
//!
//! Modal dialog for editing fan curve configurations.
//! Features an interactive graph with drag-to-edit curve points
//! and real-time temperature visualization with smooth animations.

#![allow(dead_code)]
#![allow(deprecated)]

use gtk4::prelude::*;
use gtk4::{cairo, DrawingArea, Entry, GestureDrag, Orientation};
use gtk4::gdk::{BUTTON_PRIMARY, BUTTON_SECONDARY};
use gtk4::Box as GtkBox;
use gtk4::GestureClick;
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use super::dashboard::CurveCardData;

// ============================================================================
// Constants
// ============================================================================

/// Spring physics constants for smooth animation
/// Tuned for natural, responsive motion with zero overshoot
mod physics {
    // Spring-damper system: critically damped for fastest settling without overshoot
    // Formula: damping_ratio = damping / (2 * sqrt(mass * stiffness))
    // For critical damping: damping_ratio = 1.0, assuming mass = 1.0
    pub const SPRING_STIFFNESS: f32 = 30.0;      // Higher = faster response
    pub const DAMPING_COEFFICIENT: f32 = 10.95;  // sqrt(4 * stiffness) for critical damping
    pub const SETTLE_THRESHOLD: f32 = 0.005;     // Position tolerance for "settled" state
    pub const VELOCITY_THRESHOLD: f32 = 0.01;    // Velocity tolerance for "settled" state
    pub const DEFAULT_FRAME_TIME: f32 = 0.016;   // Fallback: 60fps
    pub const MAX_DELTA_TIME: f32 = 0.05;        // Cap at 50ms to prevent huge jumps
}

mod graph {
    pub const MARGIN: f64 = 20.0;
    pub const TEMP_MIN: f32 = 20.0;
    pub const TEMP_MAX: f32 = 100.0;
}

/// Minimum epsilon to prevent control points from overlapping exactly
const POINT_SEPARATION_EPSILON: f32 = 0.1;

mod unused_graph {
    pub const TEMP_RANGE: f32 = 80.0;
    pub const HIT_RADIUS: f64 = 15.0;
    pub const POINT_RADIUS: f64 = 6.0;
    pub const POINT_GLOW_RADIUS: f64 = 10.0;
    pub const INDICATOR_RADIUS: f64 = 8.0;
    pub const INDICATOR_GLOW_RADIUS: f64 = 12.0;
}

mod config {
    pub const SENSOR_POLL_MS: u64 = 100;
}

// ============================================================================
// Animation State
// ============================================================================

/// Manages smooth temperature display transitions using spring physics
struct AnimationState {
    target_temp: f32,
    display_temp: f32,
    velocity: f32,
    last_frame_time: Option<i64>,
}

impl AnimationState {
    fn new(initial: f32) -> Self {
        Self {
            target_temp: initial,
            display_temp: initial,
            velocity: 0.0,
            last_frame_time: None,
        }
    }

    /// Advance animation by one frame using spring-damper physics
    fn tick(&mut self, frame_time: i64) -> bool {
        let dt = self.calculate_delta_time(frame_time);
        self.last_frame_time = Some(frame_time);

        let diff = self.target_temp - self.display_temp;
        
        if diff.abs() < physics::SETTLE_THRESHOLD 
            && self.velocity.abs() < physics::VELOCITY_THRESHOLD 
        {
            self.display_temp = self.target_temp;
            self.velocity = 0.0;
            return false;
        }

        let acceleration = physics::SPRING_STIFFNESS * diff 
            - physics::DAMPING_COEFFICIENT * self.velocity;
        self.velocity += acceleration * dt;
        self.display_temp += self.velocity * dt;
        true
    }

    fn calculate_delta_time(&self, frame_time: i64) -> f32 {
        match self.last_frame_time {
            Some(last) => {
                let dt = (frame_time - last) as f64 / 1_000_000.0;
                (dt as f32).min(physics::MAX_DELTA_TIME)
            }
            None => physics::DEFAULT_FRAME_TIME,
        }
    }
}

/// Edit curve dialog with live updates
pub struct EditCurveDialog {
    dialog: adw::Window,
    name_entry: Entry,
    points: Rc<RefCell<Vec<(f32, f32)>>>,
    original_points: Vec<(f32, f32)>,
    original_name: String,
    current_temp: Rc<RefCell<f32>>,
    curve_id: String,
    temp_source_path: String,
    on_save: Rc<RefCell<Option<Box<dyn Fn(CurveCardData)>>>>,
    drawing_area: DrawingArea,
    is_dirty: Rc<RefCell<bool>>,
    /// Curve parameters
    hysteresis: Rc<RefCell<f32>>,
    delay_ms: Rc<RefCell<u32>>,
    ramp_up_speed: Rc<RefCell<f32>>,
    ramp_down_speed: Rc<RefCell<f32>>,
}

impl EditCurveDialog {
    pub fn new(data: &CurveCardData) -> Rc<Self> {
        let dialog = adw::Window::builder()
            .title("Edit Fan Curve")
            .default_width(500)
            .default_height(500)
            .modal(true)
            .build();

        let points: Rc<RefCell<Vec<(f32, f32)>>> = Rc::new(RefCell::new(data.points.clone()));
        let original_points = data.points.clone();
        let original_name = data.name.clone();
        let current_temp: Rc<RefCell<f32>> = Rc::new(RefCell::new(data.current_temp));
        let on_save: Rc<RefCell<Option<Box<dyn Fn(CurveCardData)>>>> = Rc::new(RefCell::new(None));
        let is_dirty: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));
        
        // Curve parameters
        let hysteresis: Rc<RefCell<f32>> = Rc::new(RefCell::new(data.hysteresis));
        let delay_ms: Rc<RefCell<u32>> = Rc::new(RefCell::new(data.delay_ms));
        let ramp_up_speed: Rc<RefCell<f32>> = Rc::new(RefCell::new(data.ramp_up_speed));
        let ramp_down_speed: Rc<RefCell<f32>> = Rc::new(RefCell::new(data.ramp_down_speed));

        let content = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(18)
            .margin_start(24)
            .margin_end(24)
            .margin_top(18)
            .margin_bottom(24)
            .build();

        // Header bar (modal dialogs don't show window buttons)
        let header = adw::HeaderBar::builder()
            .show_end_title_buttons(false)
            .show_start_title_buttons(false)
            .build();

        let cancel_btn = gtk4::Button::builder()
            .label("Cancel")
            .build();
        
        let revert_btn = gtk4::Button::builder()
            .label("Revert")
            .tooltip_text("Restore original curve")
            .sensitive(false)
            .build();

        let save_btn = gtk4::Button::builder()
            .label("Save")
            .css_classes(["suggested-action"])
            .build();

        header.pack_start(&cancel_btn);
        header.pack_start(&revert_btn);
        header.pack_end(&save_btn);

        // Name entry
        let name_group = adw::PreferencesGroup::builder()
            .title("Curve Name")
            .build();

        let name_entry = Entry::builder()
            .text(&data.name)
            .placeholder_text("e.g. Silent, Balanced, Performance")
            .activates_default(true)
            .build();

        let name_row = adw::ActionRow::builder()
            .title("Name")
            .build();
        name_row.add_suffix(&name_entry);
        name_group.add(&name_row);

        content.append(&name_group);

        // Graph section
        let graph_group = adw::PreferencesGroup::builder()
            .title("Fan Curve")
            .description("Left-click to add, right-click to remove, drag to move")
            .build();

        let drawing_area = DrawingArea::builder()
            .height_request(200)
            .hexpand(true)
            .build();

        let graph_frame = adw::Bin::builder()
            .css_classes(["card"])
            .child(&drawing_area)
            .build();

        graph_group.add(&graph_frame);
        content.append(&graph_group);

        // Curve parameters section
        let params_group = adw::PreferencesGroup::builder()
            .title("Curve Parameters")
            .description("Fine-tune how the curve responds to temperature changes")
            .build();

        // Hysteresis row
        let hysteresis_adj = gtk4::Adjustment::new(
            data.hysteresis as f64,
            0.0,
            hf_core::constants::curve::MAX_HYSTERESIS_CELSIUS as f64,
            0.5,
            1.0,
            0.0,
        );
        let hysteresis_spin = gtk4::SpinButton::builder()
            .adjustment(&hysteresis_adj)
            .digits(1)
            .width_chars(6)
            .build();
        let hysteresis_row = adw::ActionRow::builder()
            .title("Hysteresis")
            .subtitle("Temperature must change by this amount before adjusting fan speed (°C)")
            .build();
        hysteresis_row.add_suffix(&hysteresis_spin);
        params_group.add(&hysteresis_row);

        // Delay row
        let delay_adj = gtk4::Adjustment::new(
            data.delay_ms as f64,
            0.0,
            hf_core::constants::curve::MAX_DELAY_MS as f64,
            100.0,
            1000.0,
            0.0,
        );
        let delay_spin = gtk4::SpinButton::builder()
            .adjustment(&delay_adj)
            .digits(0)
            .width_chars(6)
            .build();
        let delay_row = adw::ActionRow::builder()
            .title("Response Delay")
            .subtitle("Wait this long before responding to temperature changes (ms)")
            .build();
        delay_row.add_suffix(&delay_spin);
        params_group.add(&delay_row);

        // Ramp up speed row
        let ramp_up_adj = gtk4::Adjustment::new(
            data.ramp_up_speed as f64,
            0.0,
            hf_core::constants::curve::MAX_RAMP_SPEED as f64,
            5.0,
            25.0,
            0.0,
        );
        let ramp_up_spin = gtk4::SpinButton::builder()
            .adjustment(&ramp_up_adj)
            .digits(0)
            .width_chars(6)
            .build();
        let ramp_up_row = adw::ActionRow::builder()
            .title("Ramp Up Speed")
            .subtitle("How fast fan speeds up when temperature rises (%/sec, 0=instant)")
            .build();
        ramp_up_row.add_suffix(&ramp_up_spin);
        params_group.add(&ramp_up_row);

        // Ramp down speed row
        let ramp_down_adj = gtk4::Adjustment::new(
            data.ramp_down_speed as f64,
            0.0,
            hf_core::constants::curve::MAX_RAMP_SPEED as f64,
            5.0,
            25.0,
            0.0,
        );
        let ramp_down_spin = gtk4::SpinButton::builder()
            .adjustment(&ramp_down_adj)
            .digits(0)
            .width_chars(6)
            .build();
        let ramp_down_row = adw::ActionRow::builder()
            .title("Ramp Down Speed")
            .subtitle("How fast fan slows down when temperature drops (%/sec, 0=instant)")
            .build();
        ramp_down_row.add_suffix(&ramp_down_spin);
        params_group.add(&ramp_down_row);

        content.append(&params_group);

        // Connect spin buttons to update state
        let hysteresis_for_spin = hysteresis.clone();
        let is_dirty_for_hyst = is_dirty.clone();
        hysteresis_spin.connect_value_changed(move |spin| {
            *hysteresis_for_spin.borrow_mut() = spin.value() as f32;
            *is_dirty_for_hyst.borrow_mut() = true;
        });

        let delay_for_spin = delay_ms.clone();
        let is_dirty_for_delay = is_dirty.clone();
        delay_spin.connect_value_changed(move |spin| {
            *delay_for_spin.borrow_mut() = spin.value() as u32;
            *is_dirty_for_delay.borrow_mut() = true;
        });

        let ramp_up_for_spin = ramp_up_speed.clone();
        let is_dirty_for_ramp_up = is_dirty.clone();
        ramp_up_spin.connect_value_changed(move |spin| {
            *ramp_up_for_spin.borrow_mut() = spin.value() as f32;
            *is_dirty_for_ramp_up.borrow_mut() = true;
        });

        let ramp_down_for_spin = ramp_down_speed.clone();
        let is_dirty_for_ramp_down = is_dirty.clone();
        ramp_down_spin.connect_value_changed(move |spin| {
            *ramp_down_for_spin.borrow_mut() = spin.value() as f32;
            *is_dirty_for_ramp_down.borrow_mut() = true;
        });

        // Main container
        let main_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .build();
        main_box.append(&header);
        main_box.append(&content);

        dialog.set_content(Some(&main_box));

        // Track edit history for undo/redo
        let history: Rc<RefCell<Vec<Vec<(f32, f32)>>>> = Rc::new(RefCell::new(vec![points.borrow().clone()]));
        let history_index: Rc<RefCell<usize>> = Rc::new(RefCell::new(0));
        
        // Helper to save state to history
        let save_to_history = |hist: &Rc<RefCell<Vec<Vec<(f32, f32)>>>>, idx: &Rc<RefCell<usize>>, pts: &[(f32, f32)]| {
            let mut h = hist.borrow_mut();
            let mut i = idx.borrow_mut();
            // Truncate history after current index
            h.truncate(*i + 1);
            // Add new state
            h.push(pts.to_vec());
            *i = h.len() - 1;
        };

        let is_dirty_for_self = is_dirty.clone();
        
        let this = Rc::new(Self {
            dialog: dialog.clone(),
            name_entry,
            points,
            original_points,
            original_name,
            current_temp,
            curve_id: data.id.clone(),
            temp_source_path: data.temp_source_path.clone(),
            on_save,
            drawing_area: drawing_area.clone(),
            is_dirty: is_dirty_for_self,
            hysteresis,
            delay_ms,
            ramp_up_speed,
            ramp_down_speed,
        });
        
        // Add Ctrl+Z (undo) and Ctrl+Shift+Z (redo) keyboard shortcuts
        let key_controller = gtk4::EventControllerKey::new();
        let points_for_undo = this.points.clone();
        let drawing_for_undo = drawing_area.clone();
        let history_for_undo = history.clone();
        let history_idx_for_undo = history_index.clone();
        key_controller.connect_key_pressed(move |_, key, _, modifiers| {
            if modifiers.contains(gtk4::gdk::ModifierType::CONTROL_MASK) {
                if matches!(key, gtk4::gdk::Key::z | gtk4::gdk::Key::Z) {
                    if modifiers.contains(gtk4::gdk::ModifierType::SHIFT_MASK) {
                        // Redo (Ctrl+Shift+Z)
                        let mut idx = history_idx_for_undo.borrow_mut();
                        let hist = history_for_undo.borrow();
                        if *idx < hist.len() - 1 {
                            *idx += 1;
                            if let Some(next_points) = hist.get(*idx) {
                                *points_for_undo.borrow_mut() = next_points.clone();
                                drawing_for_undo.queue_draw();
                            }
                        }
                    } else {
                        // Undo (Ctrl+Z)
                        let mut idx = history_idx_for_undo.borrow_mut();
                        if *idx > 0 {
                            *idx -= 1;
                            let hist = history_for_undo.borrow();
                            if let Some(prev_points) = hist.get(*idx) {
                                *points_for_undo.borrow_mut() = prev_points.clone();
                                drawing_for_undo.queue_draw();
                            }
                        }
                    }
                    return gtk4::glib::Propagation::Stop;
                }
            }
            gtk4::glib::Propagation::Proceed
        });
        dialog.add_controller(key_controller);

        // Track dragging state
        let dragging_index: Rc<RefCell<Option<usize>>> = Rc::new(RefCell::new(None));
        let drag_start_pos: Rc<RefCell<(f64, f64)>> = Rc::new(RefCell::new((0.0, 0.0)));

        // Setup drawing with animated temp indicator
        let points_for_draw = this.points.clone();
        this.drawing_area.set_draw_func(move |_, cr, width, height| {
            Self::draw_static_curve(cr, width, height, &points_for_draw.borrow());
        });

        // Right-click gesture for removing points
        let right_click = GestureClick::new();
        right_click.set_button(BUTTON_SECONDARY);

        let points_for_right = this.points.clone();
        let drawing_area_for_right = this.drawing_area.clone();

        let history_for_right = history.clone();
        let history_idx_for_right = history_index.clone();
        right_click.connect_pressed(move |_, _n_press, x, y| {
            let width = drawing_area_for_right.width() as f64;
            let height = drawing_area_for_right.height() as f64;
            let margin = 20.0;

            let temp = 20.0 + ((x - margin) / (width - 2.0 * margin)) as f32 * 80.0;
            let percent = 100.0 - ((y - margin) / (height - 2.0 * margin)) as f32 * 100.0;

            let mut points = points_for_right.borrow_mut();
            if points.len() > 2 {
                let nearest = points.iter().enumerate()
                    .min_by(|(_, (t1, p1)), (_, (t2, p2))| {
                        let d1 = ((t1 - temp).powi(2) + (p1 - percent).powi(2)).sqrt();
                        let d2 = ((t2 - temp).powi(2) + (p2 - percent).powi(2)).sqrt();
                        d1.partial_cmp(&d2).unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .map(|(i, _)| i);

                if let Some(idx) = nearest {
                    points.remove(idx);
                    // Save to history
                    let mut hist = history_for_right.borrow_mut();
                    let mut hist_idx = history_idx_for_right.borrow_mut();
                    hist.truncate(*hist_idx + 1);
                    hist.push(points.clone());
                    *hist_idx = hist.len() - 1;
                }
            }
            drop(points);
            drawing_area_for_right.queue_draw();
        });

        this.drawing_area.add_controller(right_click);

        // Drag gesture for moving/adding points
        let drag = GestureDrag::new();
        drag.set_button(BUTTON_PRIMARY);

        let points_for_drag = this.points.clone();
        let drawing_area_for_drag = this.drawing_area.clone();
        let dragging_for_start = dragging_index.clone();
        let drag_start_for_start = drag_start_pos.clone();

        drag.connect_drag_begin(move |_, x, y| {
            let width = drawing_area_for_drag.width() as f64;
            let height = drawing_area_for_drag.height() as f64;
            let margin = 20.0;

            *drag_start_for_start.borrow_mut() = (x, y);

            let temp = 20.0 + ((x - margin) / (width - 2.0 * margin)) as f32 * 80.0;
            let percent = 100.0 - ((y - margin) / (height - 2.0 * margin)) as f32 * 100.0;

            let points = points_for_drag.borrow();
            let hit_radius = 15.0;

            for (i, (pt, pp)) in points.iter().enumerate() {
                let px = margin + ((*pt - 20.0) / 80.0) as f64 * (width - 2.0 * margin);
                let py = height - margin - (*pp / 100.0) as f64 * (height - 2.0 * margin);

                let dist = ((x - px).powi(2) + (y - py).powi(2)).sqrt();
                if dist < hit_radius {
                    *dragging_for_start.borrow_mut() = Some(i);
                    return;
                }
            }

            drop(points);
            let mut points = points_for_drag.borrow_mut();
            let temp = temp.clamp(20.0, 100.0);
            let percent = percent.clamp(0.0, 100.0);
            let pos = points.iter().position(|(t, _)| *t > temp).unwrap_or(points.len());
            points.insert(pos, (temp, percent));
            *dragging_for_start.borrow_mut() = Some(pos);
        });

        let points_for_update = this.points.clone();
        let drawing_area_for_update = this.drawing_area.clone();
        let dragging_for_update = dragging_index.clone();

        drag.connect_drag_update(move |_, offset_x, offset_y| {
            let Some(idx) = *dragging_for_update.borrow() else { return };

            let width = drawing_area_for_update.width() as f64;
            let height = drawing_area_for_update.height() as f64;
            let margin = 20.0;

            let mut points = points_for_update.borrow_mut();
            if idx >= points.len() { return; }

            let (start_x, start_y) = *drag_start_pos.borrow();
            let x = start_x + offset_x;
            let y = start_y + offset_y;

            let mut temp = (20.0 + ((x - margin) / (width - 2.0 * margin)) as f32 * 80.0).clamp(20.0, 100.0);
            let percent = (100.0 - ((y - margin) / (height - 2.0 * margin)) as f32 * 100.0).clamp(0.0, 100.0);

            // Constrain temperature to prevent crossing adjacent points
            let min_temp = if idx > 0 {
                points[idx - 1].0 + POINT_SEPARATION_EPSILON
            } else {
                20.0
            };
            
            let max_temp = if idx < points.len() - 1 {
                points[idx + 1].0 - POINT_SEPARATION_EPSILON
            } else {
                100.0
            };
            
            temp = temp.clamp(min_temp, max_temp);

            points[idx] = (temp, percent);

            drop(points);
            drawing_area_for_update.queue_draw();
        });

        let points_for_end = this.points.clone();
        let dragging_for_end = dragging_index.clone();
        let curve_id_for_end = this.curve_id.clone();
        let name_entry_for_end = this.name_entry.clone();
        let temp_source_path_for_end = this.temp_source_path.clone();
        let history_for_end = history.clone();
        let history_idx_for_end = history_index.clone();
        let is_dirty_for_end = is_dirty.clone();
        let revert_btn_for_end = revert_btn.clone();

        drag.connect_drag_end(move |_, _, _| {
            *dragging_for_end.borrow_mut() = None;
            
            // Mark as dirty and enable revert
            *is_dirty_for_end.borrow_mut() = true;
            revert_btn_for_end.set_sensitive(true);
            
            // Save to history after drag
            let points = points_for_end.borrow();
            let mut hist = history_for_end.borrow_mut();
            let mut hist_idx = history_idx_for_end.borrow_mut();
            hist.truncate(*hist_idx + 1);
            hist.push(points.clone());
            *hist_idx = hist.len() - 1;
            drop(points);
            drop(hist);
            drop(hist_idx);
            
            // LIVE UPDATE: Save curve and signal daemon immediately after drag ends
            // This ensures the user sees their changes applied in real-time
            let points = points_for_end.borrow();
            let persisted = hf_core::PersistedCurve {
                id: curve_id_for_end.clone(),
                name: name_entry_for_end.text().to_string(),
                temp_source_path: temp_source_path_for_end.clone(),
                temp_source_label: String::new(), // Will be filled from existing data
                points: points.clone(),
                created_at: 0,
                updated_at: 0,
                hysteresis: hf_core::constants::curve::DEFAULT_HYSTERESIS_CELSIUS,
                delay_ms: hf_core::constants::curve::DEFAULT_DELAY_MS,
                ramp_up_speed: hf_core::constants::curve::DEFAULT_RAMP_UP_SPEED,
                ramp_down_speed: hf_core::constants::curve::DEFAULT_RAMP_DOWN_SPEED,
            };
            
            if let Err(e) = hf_core::save_curve(persisted) {
                tracing::debug!("Live curve update failed: {}", e);
            } else {
                // Signal daemon to reload and apply the updated curve immediately
                if let Err(e) = hf_core::daemon_reload_config() {
                    tracing::debug!("Failed to signal daemon for live update: {}", e);
                }
            }
        });

        this.drawing_area.add_controller(drag);

        // Static drawing - no live temperature updates needed
        let points_for_draw = this.points.clone();
        
        this.drawing_area.set_draw_func(move |_, cr, width, height| {
            let points = points_for_draw.borrow();
            Self::draw_static_curve(cr, width, height, &points);
        });

        // Cancel button - check for unsaved changes
        let this_for_cancel = this.clone();
        cancel_btn.connect_clicked(move |_| {
            let has_changes = this_for_cancel.has_unsaved_changes();
            
            if has_changes {
                // Show confirmation dialog
                let confirm = libadwaita::AlertDialog::builder()
                    .heading("Discard Changes?")
                    .body("You have unsaved changes. Are you sure you want to discard them?")
                    .build();
                
                confirm.add_response("cancel", "Keep Editing");
                confirm.add_response("discard", "Discard");
                confirm.set_response_appearance("discard", libadwaita::ResponseAppearance::Destructive);
                confirm.set_default_response(Some("cancel"));
                confirm.set_close_response("cancel");
                
                let dialog_for_confirm = this_for_cancel.dialog.clone();
                confirm.choose(&this_for_cancel.dialog, None::<&gtk4::gio::Cancellable>, move |response| {
                    if response == "discard" {
                        dialog_for_confirm.close();
                    }
                });
            } else {
                this_for_cancel.dialog.close();
            }
        });

        // Save button
        let this_for_save = this.clone();
        let temp_source_label = data.temp_source_label.clone();
        save_btn.connect_clicked(move |_| {
            let name = this_for_save.name_entry.text().to_string();
            let points = this_for_save.points.borrow().clone();
            let current_temp = *this_for_save.current_temp.borrow();

            let hysteresis = *this_for_save.hysteresis.borrow();
            let delay_ms = *this_for_save.delay_ms.borrow();
            let ramp_up_speed = *this_for_save.ramp_up_speed.borrow();
            let ramp_down_speed = *this_for_save.ramp_down_speed.borrow();

            let updated_data = CurveCardData {
                id: this_for_save.curve_id.clone(),
                name,
                temp_source_path: this_for_save.temp_source_path.clone(),
                temp_source_label: temp_source_label.clone(),
                points,
                current_temp,
                hysteresis,
                delay_ms,
                ramp_up_speed,
                ramp_down_speed,
            };

            // Save to persistence
            let persisted = hf_core::PersistedCurve {
                id: updated_data.id.clone(),
                name: updated_data.name.clone(),
                temp_source_path: updated_data.temp_source_path.clone(),
                temp_source_label: updated_data.temp_source_label.clone(),
                points: updated_data.points.clone(),
                created_at: 0,
                updated_at: 0,
                hysteresis: updated_data.hysteresis,
                delay_ms: updated_data.delay_ms,
                ramp_up_speed: updated_data.ramp_up_speed,
                ramp_down_speed: updated_data.ramp_down_speed,
            };

            if let Err(e) = hf_core::save_curve(persisted) {
                tracing::warn!("Failed to save curve: {}", e);
            } else {
                // Signal daemon to reload config
                if let Err(e) = hf_core::daemon_reload_config() {
                    tracing::debug!("Failed to signal daemon reload: {}", e);
                }
            }

            if let Some(callback) = this_for_save.on_save.borrow().as_ref() {
                callback(updated_data);
            }

            this_for_save.dialog.close();
        });

        this
    }

    /// Check if there are unsaved changes
    fn has_unsaved_changes(&self) -> bool {
        let current_name = self.name_entry.text().to_string();
        let current_points = self.points.borrow();
        
        // Check if name changed
        if current_name != self.original_name {
            return true;
        }
        
        // Check if points changed
        if current_points.len() != self.original_points.len() {
            return true;
        }
        
        for (current, original) in current_points.iter().zip(self.original_points.iter()) {
            if (current.0 - original.0).abs() > 0.01 || (current.1 - original.1).abs() > 0.01 {
                return true;
            }
        }
        
        false
    }

    fn interpolate_curve(points: &[(f32, f32)], temp: f32) -> f32 {
        if points.is_empty() {
            return 100.0;
        }

        if temp <= points[0].0 {
            return points[0].1;
        }

        let last_point = match points.last() {
            Some(p) => p,
            None => return 100.0,
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

    fn draw_static_curve(cr: &cairo::Context, width: i32, height: i32, points: &[(f32, f32)]) {
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

        for pct in (0..=100).step_by(25) {
            let y = h - margin - (pct as f64 / 100.0) * (h - 2.0 * margin);
            cr.move_to(2.0, y + 4.0);
            let _ = cr.show_text(&format!("{}%", pct));
        }

        if points.is_empty() {
            return;
        }

        // PERFORMANCE: Use cached settings (no disk I/O)
        let graph_style = hf_core::get_graph_style();

        let temp_to_x = |t: f32| margin + ((t - 20.0) / 80.0) as f64 * (w - 2.0 * margin);
        let pct_to_y = |p: f32| h - margin - (p / 100.0) as f64 * (h - 2.0 * margin);

        // Draw curve fill (only for "filled" style) - use accent color
        let accent = super::curve_card::theme_colors::curve_line();
        let fill = super::curve_card::theme_colors::curve_fill();
        if graph_style == "filled" {
            cr.set_source_rgba(fill.0, fill.1, fill.2, fill.3);
            cr.move_to(temp_to_x(20.0), pct_to_y(0.0));
            cr.line_to(temp_to_x(20.0), pct_to_y(points[0].1));

            for (t, p) in points {
                cr.line_to(temp_to_x(*t), pct_to_y(*p));
            }

            if let Some((_, last_p)) = points.last() {
                cr.line_to(temp_to_x(100.0), pct_to_y(*last_p));
            }
            cr.line_to(temp_to_x(100.0), pct_to_y(0.0));
            cr.close_path();
            let _ = cr.fill();
        }

        // Draw curve line - use accent color
        cr.set_source_rgba(accent.0, accent.1, accent.2, 1.0);
        cr.set_line_width(2.5);

        cr.move_to(temp_to_x(20.0), pct_to_y(points[0].1));
        let mut prev_p = points[0].1;
        for (t, p) in points {
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
            cr.line_to(temp_to_x(100.0), pct_to_y(*last_p));
        }
        let _ = cr.stroke();

        // Draw points
        for (t, p) in points {
            let x = temp_to_x(*t);
            let y = pct_to_y(*p);

            cr.set_source_rgba(accent.0, accent.1, accent.2, 0.3);
            cr.arc(x, y, 10.0, 0.0, 2.0 * std::f64::consts::PI);
            let _ = cr.fill();

            // Point fill - theme-aware
            if is_dark {
                cr.set_source_rgb(0.95, 0.6, 0.2);
            } else {
                cr.set_source_rgb(0.85, 0.45, 0.1);
            }
            cr.arc(x, y, 6.0, 0.0, 2.0 * std::f64::consts::PI);
            let _ = cr.fill();

            // Point outline - theme-aware
            if is_dark {
                cr.set_source_rgb(1.0, 1.0, 1.0);
            } else {
                cr.set_source_rgb(0.1, 0.1, 0.1);
            }
            cr.set_line_width(2.0);
            cr.arc(x, y, 6.0, 0.0, 2.0 * std::f64::consts::PI);
            let _ = cr.stroke();
        }

    }

    pub fn connect_save<F: Fn(CurveCardData) + 'static>(&self, callback: F) {
        *self.on_save.borrow_mut() = Some(Box::new(callback));
    }

    pub fn present(&self) {
        self.dialog.present();
        // Focus the name entry for immediate keyboard input
        self.name_entry.grab_focus();
    }

    pub fn set_transient_for(&self, parent: &impl gtk4::prelude::IsA<gtk4::Window>) {
        self.dialog.set_transient_for(Some(parent));
    }
}
