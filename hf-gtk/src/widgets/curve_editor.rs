//! Fan curve editor widget
//!
//! Interactive widget for editing temperature-to-fan-speed curves.

#![allow(dead_code)]

use gtk4::prelude::*;
use gtk4::{
    cairo, Box as GtkBox, Button, DrawingArea, Label, Orientation,
};
use libadwaita as adw;
use std::cell::RefCell;
use std::rc::Rc;

use hf_core::{CurvePoint, CurvePreset};

/// A visual curve editor widget
pub struct CurveEditor {
    container: GtkBox,
    drawing_area: DrawingArea,
    points: Rc<RefCell<Vec<CurvePoint>>>,
    on_change: Rc<RefCell<Option<Box<dyn Fn(&[CurvePoint])>>>>,
}

impl CurveEditor {
    pub fn new() -> Self {
        let container = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(12)
            .build();

        // Preset buttons
        let presets_box = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(6)
            .halign(gtk4::Align::Center)
            .build();

        let points = Rc::new(RefCell::new(CurvePreset::Balanced.points()));
        let on_change: Rc<RefCell<Option<Box<dyn Fn(&[CurvePoint])>>>> =
            Rc::new(RefCell::new(None));

        // Create preset buttons
        for (label, preset) in [
            ("Quiet", CurvePreset::Quiet),
            ("Balanced", CurvePreset::Balanced),
            ("Performance", CurvePreset::Performance),
            ("Full", CurvePreset::FullSpeed),
        ] {
            let btn = Button::builder()
                .label(label)
                .css_classes(["flat"])
                .build();

            let points_clone = points.clone();
            let on_change_clone = on_change.clone();

            btn.connect_clicked(move |_| {
                let new_points = preset.points();
                *points_clone.borrow_mut() = new_points.clone();
                if let Some(ref callback) = *on_change_clone.borrow() {
                    callback(&new_points);
                }
            });

            presets_box.append(&btn);
        }

        container.append(&presets_box);

        // Drawing area for the curve
        let drawing_area = DrawingArea::builder()
            .height_request(200)
            .hexpand(true)
            .build();

        let points_for_draw = points.clone();
        drawing_area.set_draw_func(move |_, cr, width, height| {
            Self::draw_curve(cr, width, height, &points_for_draw.borrow());
        });

        let frame = adw::Bin::builder()
            .css_classes(["card"])
            .child(&drawing_area)
            .build();

        container.append(&frame);

        // Legend
        let legend = Label::builder()
            .label("Temperature (°C) → Fan Speed (%)")
            .css_classes(["dim-label", "caption"])
            .build();
        container.append(&legend);

        Self {
            container,
            drawing_area,
            points,
            on_change,
        }
    }

    fn draw_curve(cr: &cairo::Context, width: i32, height: i32, points: &[CurvePoint]) {
        let w = width as f64;
        let h = height as f64;
        let margin = 30.0;

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

        // Vertical grid lines (temperature)
        for temp in (0..=100).step_by(20) {
            let x = margin + (temp as f64 / 100.0) * (w - 2.0 * margin);
            cr.move_to(x, margin);
            cr.line_to(x, h - margin);
        }

        // Horizontal grid lines (fan speed)
        for pct in (0..=100).step_by(25) {
            let y = h - margin - (pct as f64 / 100.0) * (h - 2.0 * margin);
            cr.move_to(margin, y);
            cr.line_to(w - margin, y);
        }
        let _ = cr.stroke();

        // Axis labels - theme-aware
        if is_dark {
            cr.set_source_rgb(0.7, 0.7, 0.7);
        } else {
            cr.set_source_rgb(0.15, 0.15, 0.15);
        }
        cr.set_font_size(10.0);

        // Temperature labels
        for temp in (0..=100).step_by(20) {
            let x = margin + (temp as f64 / 100.0) * (w - 2.0 * margin);
            cr.move_to(x - 8.0, h - 10.0);
            let _ = cr.show_text(&format!("{}", temp));
        }

        // Fan speed labels
        for pct in (0..=100).step_by(25) {
            let y = h - margin - (pct as f64 / 100.0) * (h - 2.0 * margin);
            cr.move_to(5.0, y + 4.0);
            let _ = cr.show_text(&format!("{}%", pct));
        }

        if points.is_empty() {
            return;
        }

        // Draw curve line - use accent color
        let accent = super::curve_card::theme_colors::curve_line();
        cr.set_source_rgba(accent.0, accent.1, accent.2, 1.0);
        cr.set_line_width(2.5);

        let temp_to_x = |t: f32| margin + (t.clamp(0.0, 100.0) as f64 / 100.0) * (w - 2.0 * margin);
        let pct_to_y = |p: f32| h - margin - (p.clamp(0.0, 100.0) as f64 / 100.0) * (h - 2.0 * margin);

        // Start from 0°C
        cr.move_to(temp_to_x(0.0), pct_to_y(points[0].fan_percent));

        // Draw to first point
        cr.line_to(temp_to_x(points[0].temperature), pct_to_y(points[0].fan_percent));

        // Draw through all points
        for point in points {
            cr.line_to(temp_to_x(point.temperature), pct_to_y(point.fan_percent));
        }

        // Extend to 100°C
        if let Some(last) = points.last() {
            cr.line_to(temp_to_x(100.0), pct_to_y(last.fan_percent));
        }

        let _ = cr.stroke();

        // Draw points - theme-aware
        if is_dark {
            cr.set_source_rgb(0.95, 0.6, 0.2);
        } else {
            cr.set_source_rgb(0.85, 0.45, 0.1);
        }
        for point in points {
            let x = temp_to_x(point.temperature);
            let y = pct_to_y(point.fan_percent);
            cr.arc(x, y, 6.0, 0.0, 2.0 * std::f64::consts::PI);
            let _ = cr.fill();
        }

        // Point outline - theme-aware
        if is_dark {
            cr.set_source_rgb(1.0, 1.0, 1.0);
        } else {
            cr.set_source_rgb(0.1, 0.1, 0.1);
        }
        cr.set_line_width(1.5);
        for point in points {
            let x = temp_to_x(point.temperature);
            let y = pct_to_y(point.fan_percent);
            cr.arc(x, y, 6.0, 0.0, 2.0 * std::f64::consts::PI);
            let _ = cr.stroke();
        }
    }

    /// Set the curve points
    pub fn set_points(&self, points: Vec<CurvePoint>) {
        *self.points.borrow_mut() = points;
        self.drawing_area.queue_draw();
    }

    /// Get the current curve points
    pub fn get_points(&self) -> Vec<CurvePoint> {
        self.points.borrow().clone()
    }

    /// Set callback for when curve changes
    pub fn connect_changed<F: Fn(&[CurvePoint]) + 'static>(&self, callback: F) {
        *self.on_change.borrow_mut() = Some(Box::new(callback));
    }

    /// Get the widget container
    pub fn widget(&self) -> &GtkBox {
        &self.container
    }
}

impl Default for CurveEditor {
    fn default() -> Self {
        Self::new()
    }
}
