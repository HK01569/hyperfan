//! Add curve dialog with interactive graph editor
//!
//! Creates a fan curve definition (name + points) without temp source binding.
//! Curves are templates that can be paired with fans later.

#![allow(dead_code)]

use gtk4::prelude::*;
use gtk4::{cairo, DrawingArea, Entry, GestureClick, GestureDrag, Label, Orientation};
use gtk4::gdk::BUTTON_PRIMARY;
use gtk4::gdk::BUTTON_SECONDARY;
use gtk4::Box as GtkBox;
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

/// Minimum temperature value for curve points
const MIN_TEMPERATURE: f32 = 20.0;

/// Maximum temperature value for curve points
const MAX_TEMPERATURE: f32 = 100.0;

/// Minimum fan speed value for curve points
const MIN_FAN_SPEED: f32 = 0.0;

/// Maximum fan speed value for curve points
const MAX_FAN_SPEED: f32 = 100.0;

/// Minimum epsilon to prevent control points from overlapping exactly
const POINT_SEPARATION_EPSILON: f32 = 0.1;

/// Data for a created curve (name + points + parameters)
#[derive(Clone)]
pub struct CurveData {
    pub id: String,
    pub name: String,
    pub points: Vec<(f32, f32)>,
    /// Hysteresis in degrees Celsius
    pub hysteresis: f32,
    /// Delay in milliseconds before responding to temperature changes
    pub delay_ms: u32,
    /// Ramp up speed in percent per second
    pub ramp_up_speed: f32,
    /// Ramp down speed in percent per second
    pub ramp_down_speed: f32,
}

/// Add curve dialog
pub struct AddCurveDialog {
    dialog: adw::Window,
    name_entry: Entry,
    points: Rc<RefCell<Vec<(f32, f32)>>>,
    on_create: Rc<RefCell<Option<Box<dyn Fn(CurveData)>>>>,
    drawing_area: DrawingArea,
    /// Curve parameters
    hysteresis: Rc<RefCell<f32>>,
    delay_ms: Rc<RefCell<u32>>,
    ramp_up_speed: Rc<RefCell<f32>>,
    ramp_down_speed: Rc<RefCell<f32>>,
}

impl AddCurveDialog {
    pub fn new() -> Rc<Self> {
        let dialog = adw::Window::builder()
            .title("Create Fan Curve")
            .default_width(500)
            .default_height(450)
            .modal(true)
            .build();

        let points: Rc<RefCell<Vec<(f32, f32)>>> = Rc::new(RefCell::new(vec![
            (30.0, 20.0),
            (50.0, 40.0),
            (70.0, 70.0),
            (85.0, 100.0),
        ]));
        let on_create: Rc<RefCell<Option<Box<dyn Fn(CurveData)>>>> = Rc::new(RefCell::new(None));
        
        // Curve parameters with defaults
        let hysteresis: Rc<RefCell<f32>> = Rc::new(RefCell::new(hf_core::constants::curve::DEFAULT_HYSTERESIS_CELSIUS));
        let delay_ms: Rc<RefCell<u32>> = Rc::new(RefCell::new(hf_core::constants::curve::DEFAULT_DELAY_MS));
        let ramp_up_speed: Rc<RefCell<f32>> = Rc::new(RefCell::new(hf_core::constants::curve::DEFAULT_RAMP_UP_SPEED));
        let ramp_down_speed: Rc<RefCell<f32>> = Rc::new(RefCell::new(hf_core::constants::curve::DEFAULT_RAMP_DOWN_SPEED));

        let content = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(18)
            .margin_start(24)
            .margin_end(24)
            .margin_top(18)
            .margin_bottom(24)
            .build();

        // Header bar
        let header = adw::HeaderBar::builder()
            .show_end_title_buttons(false)
            .show_start_title_buttons(false)
            .build();

        let cancel_btn = gtk4::Button::builder()
            .label("Cancel")
            .build();

        let create_btn = gtk4::Button::builder()
            .label("Create")
            .css_classes(["suggested-action"])
            .sensitive(false)
            .build();
        header.pack_start(&cancel_btn);
        header.pack_end(&create_btn);

        // Name entry
        let name_group = adw::PreferencesGroup::builder()
            .title("Curve Name")
            .description("Give your curve a descriptive name")
            .build();

        let name_entry = Entry::builder()
            .placeholder_text("e.g. Silent, Balanced, Performance")
            .activates_default(true)
            .build();

        let name_row = adw::ActionRow::builder()
            .title("Name")
            .build();
        name_row.add_suffix(&name_entry);

        // Validation label for name conflicts
        let validation_label = Label::builder()
            .css_classes(["error", "caption"])
            .halign(gtk4::Align::Start)
            .visible(false)
            .build();
        name_row.add_suffix(&validation_label);
        name_group.add(&name_row);

        content.append(&name_group);

        // Enable create button when name is entered
        let create_btn_for_name = create_btn.clone();
        let validation_for_name = validation_label.clone();
        name_entry.connect_changed(move |entry| {
            let text = entry.text();
            let is_empty = text.is_empty();
            
            // Check for duplicate names
            let is_duplicate = if !is_empty {
                hf_core::load_curves()
                    .ok()
                    .map(|store| store.all().iter().any(|c| c.name == text.as_str()))
                    .unwrap_or(false)
            } else {
                false
            };
            
            if is_empty {
                validation_for_name.set_text("Name cannot be empty");
                validation_for_name.set_visible(true);
                create_btn_for_name.set_sensitive(false);
            } else if is_duplicate {
                validation_for_name.set_text("A curve with this name already exists");
                validation_for_name.set_visible(true);
                create_btn_for_name.set_sensitive(false);
            } else {
                validation_for_name.set_visible(false);
                create_btn_for_name.set_sensitive(true);
            }
        });

        // Graph section
        let graph_group = adw::PreferencesGroup::builder()
            .title("Fan Curve")
            .description("Left-click to add points, right-click to remove. Drag to adjust. Hold Ctrl for snap-to-grid.")
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
            hf_core::constants::curve::DEFAULT_HYSTERESIS_CELSIUS as f64,
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
            hf_core::constants::curve::DEFAULT_DELAY_MS as f64,
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
            hf_core::constants::curve::DEFAULT_RAMP_UP_SPEED as f64,
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
            hf_core::constants::curve::DEFAULT_RAMP_DOWN_SPEED as f64,
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
        hysteresis_spin.connect_value_changed(move |spin| {
            *hysteresis_for_spin.borrow_mut() = spin.value() as f32;
        });

        let delay_for_spin = delay_ms.clone();
        delay_spin.connect_value_changed(move |spin| {
            *delay_for_spin.borrow_mut() = spin.value() as u32;
        });

        let ramp_up_for_spin = ramp_up_speed.clone();
        ramp_up_spin.connect_value_changed(move |spin| {
            *ramp_up_for_spin.borrow_mut() = spin.value() as f32;
        });

        let ramp_down_for_spin = ramp_down_speed.clone();
        ramp_down_spin.connect_value_changed(move |spin| {
            *ramp_down_for_spin.borrow_mut() = spin.value() as f32;
        });

        // Instructions
        let hint = Label::builder()
            .label("Temperature range: 20-100°C • Fan speed: 0-100%\nKeyboard: Escape=Cancel, Enter=Create, Arrow keys=Adjust point")
            .css_classes(["dim-label", "caption"])
            .halign(gtk4::Align::Start)
            .build();
        content.append(&hint);

        // Main container
        let main_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .build();
        main_box.append(&header);
        main_box.append(&content);

        dialog.set_content(Some(&main_box));

        let this = Rc::new(Self {
            dialog: dialog.clone(),
            name_entry: name_entry.clone(),
            points: points.clone(),
            on_create: on_create.clone(),
            drawing_area: drawing_area.clone(),
            hysteresis,
            delay_ms,
            ramp_up_speed,
            ramp_down_speed,
        });

        // Track which node is being dragged
        let dragging_index: Rc<RefCell<Option<usize>>> = Rc::new(RefCell::new(None));
        let drag_start_pos: Rc<RefCell<(f64, f64)>> = Rc::new(RefCell::new((0.0, 0.0)));
        
        // Track selected point for keyboard navigation
        let selected_point: Rc<RefCell<Option<usize>>> = Rc::new(RefCell::new(None));
        
        // Add keyboard controller
        let key_controller = gtk4::EventControllerKey::new();
        let dialog_for_keys = dialog.clone();
        let create_btn_for_keys = create_btn.clone();
        let points_for_keys = points.clone();
        let drawing_area_for_keys = drawing_area.clone();
        let selected_for_keys = selected_point.clone();
        
        key_controller.connect_key_pressed(move |_, key, _, _| {
            match key {
                gtk4::gdk::Key::Escape => {
                    dialog_for_keys.close();
                    gtk4::glib::Propagation::Stop
                }
                gtk4::gdk::Key::Return | gtk4::gdk::Key::KP_Enter => {
                    if create_btn_for_keys.is_sensitive() {
                        create_btn_for_keys.activate();
                    }
                    gtk4::glib::Propagation::Stop
                }
                gtk4::gdk::Key::Left => {
                    if let Some(idx) = *selected_for_keys.borrow() {
                        let mut pts = points_for_keys.borrow_mut();
                        if let Some((temp, _)) = pts.get_mut(idx) {
                            *temp = (*temp - 1.0).max(MIN_TEMPERATURE);
                            pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
                        }
                        drop(pts);
                        drawing_area_for_keys.queue_draw();
                    }
                    gtk4::glib::Propagation::Stop
                }
                gtk4::gdk::Key::Right => {
                    if let Some(idx) = *selected_for_keys.borrow() {
                        let mut pts = points_for_keys.borrow_mut();
                        if let Some((temp, _)) = pts.get_mut(idx) {
                            *temp = (*temp + 1.0).min(MAX_TEMPERATURE);
                            pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
                        }
                        drop(pts);
                        drawing_area_for_keys.queue_draw();
                    }
                    gtk4::glib::Propagation::Stop
                }
                gtk4::gdk::Key::Up => {
                    if let Some(idx) = *selected_for_keys.borrow() {
                        let mut pts = points_for_keys.borrow_mut();
                        if let Some((_, speed)) = pts.get_mut(idx) {
                            *speed = (*speed + 1.0).min(MAX_FAN_SPEED);
                        }
                        drop(pts);
                        drawing_area_for_keys.queue_draw();
                    }
                    gtk4::glib::Propagation::Stop
                }
                gtk4::gdk::Key::Down => {
                    if let Some(idx) = *selected_for_keys.borrow() {
                        let mut pts = points_for_keys.borrow_mut();
                        if let Some((_, speed)) = pts.get_mut(idx) {
                            *speed = (*speed - 1.0).max(MIN_FAN_SPEED);
                        }
                        drop(pts);
                        drawing_area_for_keys.queue_draw();
                    }
                    gtk4::glib::Propagation::Stop
                }
                gtk4::gdk::Key::Delete | gtk4::gdk::Key::BackSpace => {
                    if let Some(idx) = *selected_for_keys.borrow() {
                        let mut pts = points_for_keys.borrow_mut();
                        if pts.len() > 2 {
                            pts.remove(idx);
                            *selected_for_keys.borrow_mut() = None;
                        }
                        drop(pts);
                        drawing_area_for_keys.queue_draw();
                    }
                    gtk4::glib::Propagation::Stop
                }
                gtk4::gdk::Key::Tab => {
                    let pts = points_for_keys.borrow();
                    let current = *selected_for_keys.borrow();
                    let next = match current {
                        Some(idx) if idx + 1 < pts.len() => Some(idx + 1),
                        _ => Some(0),
                    };
                    *selected_for_keys.borrow_mut() = next;
                    drop(pts);
                    drawing_area_for_keys.queue_draw();
                    gtk4::glib::Propagation::Stop
                }
                _ => gtk4::glib::Propagation::Proceed
            }
        });
        dialog.add_controller(key_controller);

        // Setup drawing
        let points_for_draw = this.points.clone();
        this.drawing_area.set_draw_func(move |_, cr, width, height| {
            Self::draw_interactive_curve(cr, width, height, &points_for_draw.borrow());
        });

        // Right-click gesture for removing points
        let right_click = GestureClick::new();
        right_click.set_button(BUTTON_SECONDARY);

        let points_for_right = this.points.clone();
        let drawing_area_for_right = this.drawing_area.clone();

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

        let selected_for_drag = selected_point.clone();
        drag.connect_drag_begin(move |_, x, y| {
            let width = drawing_area_for_drag.width() as f64;
            let height = drawing_area_for_drag.height() as f64;
            let margin = 20.0;

            *drag_start_for_start.borrow_mut() = (x, y);

            let temp = 20.0 + ((x - margin) / (width - 2.0 * margin)) as f32 * 80.0;
            let percent = 100.0 - ((y - margin) / (height - 2.0 * margin)) as f32 * 100.0;

            // Check if clicking near an existing point
            let points = points_for_drag.borrow();
            let hit_radius = 15.0;

            for (i, (pt, pp)) in points.iter().enumerate() {
                let px = margin + ((*pt - 20.0) / 80.0) as f64 * (width - 2.0 * margin);
                let py = height - margin - (*pp / 100.0) as f64 * (height - 2.0 * margin);

                let dist = ((x - px).powi(2) + (y - py).powi(2)).sqrt();
                if dist < hit_radius {
                    *dragging_for_start.borrow_mut() = Some(i);
                    *selected_for_drag.borrow_mut() = Some(i);
                    return;
                }
            }

            // Not near a point - add a new one
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

        drag.connect_drag_update(move |gesture, offset_x, offset_y| {
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
            let mut percent = (100.0 - ((y - margin) / (height - 2.0 * margin)) as f32 * 100.0).clamp(0.0, 100.0);
            
            // Snap to grid when Ctrl is held (5°C and 5% increments)
            if let Some(device) = gesture.device() {
                let modifier_state = device.modifier_state();
                if modifier_state.contains(gtk4::gdk::ModifierType::CONTROL_MASK) {
                    temp = (temp / 5.0).round() * 5.0;
                    percent = (percent / 5.0).round() * 5.0;
                }
            }

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

        drag.connect_drag_end(move |_, _, _| {
            *dragging_for_end.borrow_mut() = None;
        });

        this.drawing_area.add_controller(drag);

        // Cancel button
        let dialog_for_cancel = this.dialog.clone();
        cancel_btn.connect_clicked(move |_| {
            dialog_for_cancel.close();
        });

        // Create button
        let this_for_create = this.clone();
        create_btn.connect_clicked(move |_| {
            let name = this_for_create.name_entry.text().to_string();
            let points = this_for_create.points.borrow().clone();
            let hysteresis = *this_for_create.hysteresis.borrow();
            let delay_ms = *this_for_create.delay_ms.borrow();
            let ramp_up_speed = *this_for_create.ramp_up_speed.borrow();
            let ramp_down_speed = *this_for_create.ramp_down_speed.borrow();

            if !name.is_empty() {
                let data = CurveData {
                    id: hf_core::generate_guid(),
                    name,
                    points,
                    hysteresis,
                    delay_ms,
                    ramp_up_speed,
                    ramp_down_speed,
                };

                if let Some(callback) = this_for_create.on_create.borrow().as_ref() {
                    callback(data);
                }

                this_for_create.dialog.close();
            }
        });

        this
    }

    fn draw_interactive_curve(cr: &cairo::Context, width: i32, height: i32, points: &[(f32, f32)]) {
        let w = width as f64;
        let h = height as f64;
        let margin = 20.0;

        // Background - theme-aware for WCAG AA compliance
        let is_dark = super::curve_card::theme_colors::is_dark_mode();
        if is_dark {
            cr.set_source_rgb(0.12, 0.12, 0.14);
        } else {
            cr.set_source_rgb(0.85, 0.85, 0.88);  // WCAG AA: proper contrast
        }
        cr.rectangle(0.0, 0.0, w, h);
        if let Err(e) = cr.fill() {
            tracing::debug!("Cairo fill error: {:?}", e);
        }

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
        if let Err(e) = cr.stroke() {
            tracing::debug!("Cairo stroke error: {:?}", e);
        }

        // Axis labels - theme-aware for WCAG AA compliance
        if is_dark {
            cr.set_source_rgb(0.6, 0.6, 0.6);
        } else {
            cr.set_source_rgb(0.15, 0.15, 0.15);  // WCAG AA: dark text on light bg
        }
        cr.set_font_size(10.0);

        for temp in (20..=100).step_by(20) {
            let x = margin + ((temp - 20) as f64 / 80.0) * (w - 2.0 * margin);
            cr.move_to(x - 8.0, h - 5.0);
            if let Err(e) = cr.show_text(&format!("{}°", temp)) {
                tracing::debug!("Cairo text error: {:?}", e);
            }
        }

        for pct in (0..=100).step_by(25) {
            let y = h - margin - (pct as f64 / 100.0) * (h - 2.0 * margin);
            cr.move_to(2.0, y + 4.0);
            if let Err(e) = cr.show_text(&format!("{}%", pct)) {
                tracing::debug!("Cairo text error: {:?}", e);
            }
        }

        if points.is_empty() {
            return;
        }

        // PERFORMANCE: Use cached settings (no disk I/O in draw function)
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
            if let Err(e) = cr.fill() {
                tracing::debug!("Cairo fill error: {:?}", e);
            }
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
        if let Err(e) = cr.stroke() {
            tracing::debug!("Cairo stroke error: {:?}", e);
        }

        // Draw points
        for (t, p) in points {
            let x = temp_to_x(*t);
            let y = pct_to_y(*p);

            // Outer glow - use accent color
            cr.set_source_rgba(accent.0, accent.1, accent.2, 0.3);
            cr.arc(x, y, 10.0, 0.0, 2.0 * std::f64::consts::PI);
            if let Err(e) = cr.fill() {
                tracing::debug!("Cairo fill error: {:?}", e);
            }

            // Point fill - theme-aware
            if is_dark {
                cr.set_source_rgb(0.95, 0.6, 0.2);
            } else {
                cr.set_source_rgb(0.85, 0.45, 0.1);  // WCAG AA: darker for light mode
            }
            cr.arc(x, y, 6.0, 0.0, 2.0 * std::f64::consts::PI);
            if let Err(e) = cr.fill() {
                tracing::debug!("Cairo fill error: {:?}", e);
            }

            // Point outline - theme-aware
            if is_dark {
                cr.set_source_rgb(1.0, 1.0, 1.0);
            } else {
                cr.set_source_rgb(0.1, 0.1, 0.1);  // WCAG AA: dark outline for light mode
            }
            cr.set_line_width(2.0);
            cr.arc(x, y, 6.0, 0.0, 2.0 * std::f64::consts::PI);
            if let Err(e) = cr.stroke() {
                tracing::debug!("Cairo stroke error: {:?}", e);
            }
        }
    }

    pub fn connect_create<F: Fn(CurveData) + 'static>(&self, callback: F) {
        *self.on_create.borrow_mut() = Some(Box::new(callback));
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


impl Clone for AddCurveDialog {
    fn clone(&self) -> Self {
        Self {
            dialog: self.dialog.clone(),
            name_entry: self.name_entry.clone(),
            points: self.points.clone(),
            on_create: self.on_create.clone(),
            drawing_area: self.drawing_area.clone(),
            hysteresis: self.hysteresis.clone(),
            delay_ms: self.delay_ms.clone(),
            ramp_up_speed: self.ramp_up_speed.clone(),
            ramp_down_speed: self.ramp_down_speed.clone(),
        }
    }
}
