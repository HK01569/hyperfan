//! Fan Curves Page
//!
//! Dedicated page for creating and managing fan curve templates.

#![allow(dead_code)]

use gtk4::prelude::*;
use gtk4::Box as GtkBox;
use gtk4::{Button, Label, Orientation, ScrolledWindow, GestureClick};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use tracing::{debug, warn};

use super::add_curve_dialog::{AddCurveDialog, CurveData};
use super::edit_curve_dialog::EditCurveDialog;
use super::dashboard::CurveCardData;

/// Fan curves page state
#[derive(Default)]
struct CurvesPageState {
    curves: Vec<hf_core::PersistedCurve>,
}

/// Fan curves management page
pub struct CurvesPage {
    container: GtkBox,
    state: Rc<RefCell<CurvesPageState>>,
    curves_list: GtkBox,
    empty_state: adw::StatusPage,
    stack: gtk4::Stack,
}

impl CurvesPage {
    pub fn new() -> Self {
        let container = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .build();

        let state = Rc::new(RefCell::new(CurvesPageState::default()));

        // Load persisted curves
        let persisted = hf_core::load_curves().unwrap_or_else(|e| {
            warn!("Failed to load curves: {}", e);
            hf_core::CurveStore::new()
        });

        for curve in persisted.all() {
            state.borrow_mut().curves.push(curve.clone());
        }

        debug!("CurvesPage loaded {} curves", state.borrow().curves.len());

        // Main content (no separate header bar - uses main app header)
        let scroll = ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .build();

        let content = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(24)
            .margin_start(24)
            .margin_end(24)
            .margin_top(24)
            .margin_bottom(24)
            .build();

        // Page header with title and add button - HIG: 12px spacing between elements
        let page_header = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .margin_bottom(6)
            .build();

        let page_title = Label::builder()
            .label("Fan Curves")
            .css_classes(["title-1"])
            .hexpand(true)
            .halign(gtk4::Align::Start)
            .build();

        let add_btn = Button::builder()
            .icon_name("list-add-symbolic")
            .css_classes(["fab", "suggested-action"])
            .tooltip_text("Create New Curve")
            .build();
        
        // Sort dropdown
        let sort_menu = gtk4::gio::Menu::new();
        sort_menu.append(Some("Name (A-Z)"), Some("curves.sort-name-asc"));
        sort_menu.append(Some("Name (Z-A)"), Some("curves.sort-name-desc"));
        sort_menu.append(Some("Recently Modified"), Some("curves.sort-modified"));
        sort_menu.append(Some("Recently Created"), Some("curves.sort-created"));
        
        let sort_btn = gtk4::MenuButton::builder()
            .icon_name("view-sort-ascending-symbolic")
            .css_classes(["circular", "flat"])
            .tooltip_text("Sort curves")
            .menu_model(&sort_menu)
            .build();

        page_header.append(&page_title);
        page_header.append(&sort_btn);
        page_header.append(&add_btn);
        content.append(&page_header);
        
        // Search/filter entry
        let search_entry = gtk4::SearchEntry::builder()
            .placeholder_text("Search curves...")
            .hexpand(true)
            .margin_bottom(12)
            .build();
        content.append(&search_entry);
        
        // Add Ctrl+F keyboard shortcut to focus search
        let key_controller = gtk4::EventControllerKey::new();
        let search_for_keys = search_entry.clone();
        key_controller.connect_key_pressed(move |_, key, _, modifiers| {
            if modifiers.contains(gtk4::gdk::ModifierType::CONTROL_MASK) {
                if matches!(key, gtk4::gdk::Key::f | gtk4::gdk::Key::F) {
                    search_for_keys.grab_focus();
                    return gtk4::glib::Propagation::Stop;
                }
            }
            gtk4::glib::Propagation::Proceed
        });
        container.add_controller(key_controller);
        
        // Add Escape to clear search
        let search_key_controller = gtk4::EventControllerKey::new();
        let search_for_escape = search_entry.clone();
        search_key_controller.connect_key_pressed(move |_, key, _, _| {
            if matches!(key, gtk4::gdk::Key::Escape) {
                search_for_escape.set_text("");
                return gtk4::glib::Propagation::Stop;
            }
            gtk4::glib::Propagation::Proceed
        });
        search_entry.add_controller(search_key_controller);

        // Stack for empty vs list state
        let stack = gtk4::Stack::builder()
            .transition_type(gtk4::StackTransitionType::Crossfade)
            .transition_duration(150)
            .build();

        // Empty state
        let empty_state = adw::StatusPage::builder()
            .icon_name("document-new-symbolic")
            .title("No Fan Curves")
            .description("Fan curves define how fan speed responds to temperature.\nCreate curves here, then pair them with fans on the Dashboard.")
            .vexpand(true)
            .build();

        let empty_btn = Button::builder()
            .label("Create Your First Curve")
            .css_classes(["suggested-action", "pill"])
            .halign(gtk4::Align::Center)
            .build();
        empty_state.set_child(Some(&empty_btn));

        // Curves list
        let curves_list = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(12)
            .build();

        stack.add_named(&empty_state, Some("empty"));
        stack.add_named(&curves_list, Some("list"));

        content.append(&stack);
        scroll.set_child(Some(&content));
        container.append(&scroll);

        let page = Self {
            container,
            state,
            curves_list,
            empty_state,
            stack,
        };

        // Build initial list
        page.rebuild_list();
        
        // Wire up search entry for live filtering
        let state_for_search = page.state.clone();
        let curves_list_for_search = page.curves_list.clone();
        let stack_for_search = page.stack.clone();
        search_entry.connect_search_changed(move |entry| {
            let filter_text = entry.text().to_lowercase();
            
            // Clear current list
            while let Some(child) = curves_list_for_search.first_child() {
                curves_list_for_search.remove(&child);
            }
            
            // Filter and rebuild
            let state = state_for_search.borrow();
            let filtered_curves: Vec<_> = state.curves.iter()
                .filter(|c| {
                    if filter_text.is_empty() {
                        true
                    } else {
                        c.name.to_lowercase().contains(&filter_text)
                    }
                })
                .collect();
            
            if filtered_curves.is_empty() {
                if filter_text.is_empty() {
                    // No curves at all
                    stack_for_search.set_visible_child_name("empty");
                } else {
                    // No matches for filter
                    let no_results = adw::StatusPage::builder()
                        .icon_name("edit-find-symbolic")
                        .title("No Matching Curves")
                        .description(&format!("No curves match '{}'", entry.text()))
                        .build();
                    curves_list_for_search.append(&no_results);
                    stack_for_search.set_visible_child_name("list");
                }
            } else {
                // Show filtered results
                for curve in filtered_curves {
                    let card_data = super::dashboard::CurveCardData {
                        id: curve.id.clone(),
                        name: curve.name.clone(),
                        temp_source_path: String::new(),
                        temp_source_label: String::new(),
                        points: curve.points.clone(),
                        current_temp: 0.0,
                        hysteresis: curve.hysteresis,
                        delay_ms: curve.delay_ms,
                        ramp_up_speed: curve.ramp_up_speed,
                        ramp_down_speed: curve.ramp_down_speed,
                    };
                    let card = super::curve_card::CurveCard::new(&card_data);
                    curves_list_for_search.append(card.widget());
                }
                stack_for_search.set_visible_child_name("list");
            }
        });

        // Wire up add button
        let state_for_add = page.state.clone();
        let curves_list_for_add = page.curves_list.clone();
        let stack_for_add = page.stack.clone();

        let add_handler = move |btn: &Button| {
            let dialog = AddCurveDialog::new();

            // Set transient parent for proper modal behavior
            if let Some(window) = btn.root().and_then(|r| r.downcast::<gtk4::Window>().ok()) {
                dialog.set_transient_for(&window);
            }

            let state_for_dialog = state_for_add.clone();
            let curves_list_for_dialog = curves_list_for_add.clone();
            let stack_for_dialog = stack_for_add.clone();

            dialog.connect_create(move |data: CurveData| {
                // Save to disk
                let persisted = hf_core::PersistedCurve {
                    id: data.id.clone(),
                    name: data.name.clone(),
                    temp_source_path: String::new(),
                    temp_source_label: String::new(),
                    points: data.points.clone(),
                    created_at: 0,
                    updated_at: 0,
                    hysteresis: data.hysteresis,
                    delay_ms: data.delay_ms,
                    ramp_up_speed: data.ramp_up_speed,
                    ramp_down_speed: data.ramp_down_speed,
                };

                if let Err(e) = hf_core::save_curve(persisted.clone()) {
                    warn!("Failed to save curve: {}", e);
                } else {
                    debug!("Saved curve: {}", data.name);
                    // Signal daemon to reload config
                    if let Err(e) = hf_core::daemon_reload_config() {
                        debug!("Failed to signal daemon reload: {}", e);
                    }
                }

                // Add to state
                state_for_dialog.borrow_mut().curves.push(persisted);

                // Rebuild list
                Self::rebuild_list_static(&state_for_dialog, &curves_list_for_dialog, &stack_for_dialog);
            });

            dialog.present();
        };

        add_btn.connect_clicked(add_handler.clone());
        empty_btn.connect_clicked(add_handler);

        page
    }

    fn rebuild_list(&self) {
        Self::rebuild_list_static(&self.state, &self.curves_list, &self.stack);
    }

    fn rebuild_list_static(
        state: &Rc<RefCell<CurvesPageState>>,
        curves_list: &GtkBox,
        stack: &gtk4::Stack,
    ) {
        // Clear existing
        while let Some(child) = curves_list.first_child() {
            curves_list.remove(&child);
        }

        let curves = &state.borrow().curves;

        if curves.is_empty() {
            stack.set_visible_child_name("empty");
            return;
        }

        stack.set_visible_child_name("list");

        for curve in curves {
            let card = Self::create_curve_card(curve, state.clone(), curves_list.clone(), stack.clone());
            curves_list.append(&card);
        }
    }

    fn create_curve_card(
        curve: &hf_core::PersistedCurve,
        state: Rc<RefCell<CurvesPageState>>,
        curves_list: GtkBox,
        stack: gtk4::Stack,
    ) -> adw::Bin {
        let card = adw::Bin::builder()
            .css_classes(["card", "curve-card-clickable"])
            .build();

        let content = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(16)
            .margin_start(16)
            .margin_end(16)
            .margin_top(16)
            .margin_bottom(16)
            .build();

        // Make entire card clickable to edit
        let click = GestureClick::new();
        let curve_for_click = curve.clone();
        let state_for_click = state.clone();
        let curves_list_for_click = curves_list.clone();
        let stack_for_click = stack.clone();
        
        let card_for_click = card.clone();
        click.connect_released(move |_, _, _, _| {
            // Get parent window for modal behavior
            let parent_window = card_for_click.root().and_then(|r| r.downcast::<gtk4::Window>().ok());
            Self::open_edit_dialog(
                &curve_for_click,
                state_for_click.clone(),
                curves_list_for_click.clone(),
                stack_for_click.clone(),
                parent_window.as_ref(),
            );
        });
        card.add_controller(click);

        // Curve preview
        let preview = gtk4::DrawingArea::builder()
            .width_request(120)
            .height_request(80)
            .build();

        let points = curve.points.clone();
        preview.set_draw_func(move |_, cr, width, height| {
            Self::draw_curve_preview(cr, width, height, &points);
        });

        content.append(&preview);

        // Info section
        let info = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(6)
            .hexpand(true)
            .valign(gtk4::Align::Center)
            .build();

        let name = Label::builder()
            .label(&curve.name)
            .css_classes(["title-3"])
            .halign(gtk4::Align::Start)
            .build();

        let points_info = Label::builder()
            .label(&format!("{} control points", curve.points.len()))
            .css_classes(["dim-label"])
            .halign(gtk4::Align::Start)
            .build();

        // Show temp range
        let (min_temp, max_temp) = if !curve.points.is_empty() {
            let min = curve.points.iter().map(|(t, _)| *t).fold(f32::MAX, f32::min);
            let max = curve.points.iter().map(|(t, _)| *t).fold(f32::MIN, f32::max);
            (min, max)
        } else {
            (0.0, 100.0)
        };

        let unit = hf_core::display::temp_unit_suffix();
        let (min_display, max_display) = if hf_core::display::is_fahrenheit() {
            (hf_core::display::celsius_to_fahrenheit(min_temp), hf_core::display::celsius_to_fahrenheit(max_temp))
        } else {
            (min_temp, max_temp)
        };
        let range_info = Label::builder()
            .label(&format!("Range: {:.0}{} - {:.0}{}", min_display, unit, max_display, unit))
            .css_classes(["dim-label", "caption"])
            .halign(gtk4::Align::Start)
            .build();

        info.append(&name);
        info.append(&points_info);
        info.append(&range_info);
        content.append(&info);

        // Actions
        let actions = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(6)
            .valign(gtk4::Align::Center)
            .build();

        let delete_btn = Button::builder()
            .icon_name("user-trash-symbolic")
            .css_classes(["flat", "circular", "destructive-action"])
            .tooltip_text("Delete Curve")
            .build();

        let curve_id = curve.id.clone();
        let curve_name = curve.name.clone();
        delete_btn.connect_clicked(move |btn| {
            let confirm = libadwaita::AlertDialog::builder()
                .heading("Delete Curve?")
                .body(&format!("Are you sure you want to delete \"{}\"? This cannot be undone.", curve_name))
                .build();

            confirm.add_response("cancel", "Cancel");
            confirm.add_response("delete", "Delete");
            confirm.set_response_appearance("delete", libadwaita::ResponseAppearance::Destructive);
            confirm.set_default_response(Some("cancel"));
            confirm.set_close_response("cancel");

            let curve_id = curve_id.clone();
            let state = state.clone();
            let curves_list = curves_list.clone();
            let stack = stack.clone();

            if let Some(root) = btn.root() {
                if let Some(window) = root.downcast_ref::<gtk4::Window>() {
                    confirm.choose(window, None::<&gtk4::gio::Cancellable>, move |response| {
                        if response == "delete" {
                            if let Err(e) = hf_core::delete_curve(&curve_id) {
                                warn!("Failed to delete curve: {}", e);
                            } else {
                                debug!("Deleted curve: {}", curve_id);
                                if let Err(e) = hf_core::daemon_reload_config() {
                                    debug!("Failed to signal daemon reload: {}", e);
                                }
                            }
                            state.borrow_mut().curves.retain(|c| c.id != curve_id);
                            Self::rebuild_list_static(&state, &curves_list, &stack);
                        }
                    });
                }
            }
        });

        actions.append(&delete_btn);
        content.append(&actions);

        card.set_child(Some(&content));
        card
    }

    fn open_edit_dialog(
        curve: &hf_core::PersistedCurve,
        state: Rc<RefCell<CurvesPageState>>,
        curves_list: GtkBox,
        stack: gtk4::Stack,
        parent_window: Option<&gtk4::Window>,
    ) {
        // Create CurveCardData for EditCurveDialog
        let data = CurveCardData {
            id: curve.id.clone(),
            name: curve.name.clone(),
            temp_source_path: curve.temp_source_path.clone(),
            temp_source_label: curve.temp_source_label.clone(),
            points: curve.points.clone(),
            current_temp: 40.0, // Default temp for template editing
            hysteresis: curve.hysteresis,
            delay_ms: curve.delay_ms,
            ramp_up_speed: curve.ramp_up_speed,
            ramp_down_speed: curve.ramp_down_speed,
        };
        
        let dialog = EditCurveDialog::new(&data);
        
        // Set transient parent for proper modal behavior
        if let Some(win) = parent_window {
            dialog.set_transient_for(win);
        }

        let curve_id = curve.id.clone();
        let state_for_save = state.clone();
        let curves_list_for_save = curves_list.clone();
        let stack_for_save = stack.clone();

        dialog.connect_save(move |updated_data| {
            // Update in-memory state (disk save + daemon reload already done by EditCurveDialog)
            if let Some(c) = state_for_save.borrow_mut().curves.iter_mut().find(|c| c.id == curve_id) {
                c.name = updated_data.name.clone();
                c.points = updated_data.points.clone();
                debug!("Updated curve in state: {}", curve_id);
            }

            // Rebuild list to show updated preview
            Self::rebuild_list_static(&state_for_save, &curves_list_for_save, &stack_for_save);
        });

        dialog.present();
    }

    fn draw_curve_preview(cr: &gtk4::cairo::Context, width: i32, height: i32, points: &[(f32, f32)]) {
        let w = width as f64;
        let h = height as f64;
        let m = 8.0;

        // Background - theme-aware
        let bg = super::curve_card::theme_colors::is_dark_mode();
        if bg {
            cr.set_source_rgba(0.15, 0.15, 0.17, 1.0);
        } else {
            cr.set_source_rgba(0.88, 0.88, 0.90, 1.0);
        }
        cr.rectangle(0.0, 0.0, w, h);
        let _ = cr.fill();

        // Grid - use theme colors
        let grid = super::curve_card::theme_colors::grid_line();
        cr.set_source_rgba(grid.0, grid.1, grid.2, grid.3);
        cr.set_line_width(1.0);
        for i in 1..4 {
            let x = m + (i as f64 / 4.0) * (w - 2.0 * m);
            cr.move_to(x, m);
            cr.line_to(x, h - m);
            let y = m + (i as f64 / 4.0) * (h - 2.0 * m);
            cr.move_to(m, y);
            cr.line_to(w - m, y);
        }
        let _ = cr.stroke();

        if points.is_empty() {
            return;
        }

        // Helper functions for coordinate transformation
        let temp_to_x = |temp: f32| -> f64 {
            m + ((temp as f64 - 20.0) / 80.0) * (w - 2.0 * m)
        };
        let pct_to_y = |pct: f32| -> f64 {
            h - m - (pct as f64 / 100.0) * (h - 2.0 * m)
        };

        // Get graph style from settings
        let graph_style = hf_core::get_graph_style();

        // Fill area under curve - use accent color with transparency
        let accent = super::curve_card::theme_colors::accent_color();
        cr.set_source_rgba(accent.0, accent.1, accent.2, 0.2);
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

        // Line - use accent color
        cr.set_source_rgba(accent.0, accent.1, accent.2, 1.0);
        cr.set_line_width(2.0);
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

        // Points - use accent color for consistency
        let accent = super::curve_card::theme_colors::curve_line();
        cr.set_source_rgba(accent.0, accent.1, accent.2, 1.0);
        for (t, p) in points {
            cr.arc(temp_to_x(*t), pct_to_y(*p), 4.0, 0.0, 2.0 * std::f64::consts::PI);
            let _ = cr.fill();
        }
    }

    pub fn widget(&self) -> &GtkBox {
        &self.container
    }

    pub fn refresh(&self) {
        // Reload curves from disk
        let persisted = hf_core::load_curves().unwrap_or_else(|_| hf_core::CurveStore::new());
        
        self.state.borrow_mut().curves.clear();
        for curve in persisted.all() {
            self.state.borrow_mut().curves.push(curve.clone());
        }
        
        self.rebuild_list();
    }
}

impl Default for CurvesPage {
    fn default() -> Self {
        Self::new()
    }
}
