//! Dashboard view with separate curves and pairs sections

#![allow(dead_code)]

use gtk4::prelude::*;
use gtk4::Box as GtkBox;
use gtk4::{Button, Label, Orientation, ScrolledWindow};
use gtk4::glib;
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use tracing::{debug, warn};

use hf_core::daemon_client;

/// Theme-aware colors for graph drawing
/// Uses system accent color from curve_card::theme_colors
mod theme_colors {
    use libadwaita as adw;
    
    pub fn is_dark_mode() -> bool {
        let style_manager = adw::StyleManager::default();
        style_manager.is_dark()
    }
    
    /// Graph background color - subtle, theme-appropriate
    pub fn graph_bg() -> (f64, f64, f64, f64) {
        if is_dark_mode() {
            (0.12, 0.12, 0.14, 1.0)  // Dark mode: near-black
        } else {
            (0.85, 0.85, 0.88, 1.0)  // Light mode: medium gray for WCAG AA contrast
        }
    }
    
    /// Mini curve background - slightly more opaque
    pub fn mini_graph_bg() -> (f64, f64, f64, f64) {
        if is_dark_mode() {
            (0.2, 0.2, 0.22, 1.0)    // Dark mode
        } else {
            (0.88, 0.88, 0.90, 1.0)  // Light mode: darker for better contrast
        }
    }
    
    /// Primary curve line color - uses GNOME system accent color
    pub fn curve_line() -> (f64, f64, f64) {
        let (r, g, b, _) = super::super::curve_card::theme_colors::accent_color();
        (r, g, b)
    }
    
    /// Curve fill color (semi-transparent accent)
    pub fn curve_fill() -> (f64, f64, f64, f64) {
        let (r, g, b, _) = super::super::curve_card::theme_colors::accent_color();
        if is_dark_mode() {
            (r, g, b, 0.15)
        } else {
            (r, g, b, 0.35)  // WCAG AA: increased opacity for better visibility
        }
    }
    
    /// Temperature indicator color (red/orange)
    pub fn indicator() -> (f64, f64, f64) {
        if is_dark_mode() {
            (0.95, 0.3, 0.3)   // Bright red for dark
        } else {
            (0.75, 0.15, 0.15)   // WCAG AA: darker red for better contrast
        }
    }
    
    /// Indicator line (semi-transparent)
    pub fn indicator_line() -> (f64, f64, f64, f64) {
        let (r, g, b) = indicator();
        (r, g, b, 0.8)
    }
}

use super::add_pair_dialog::{AddPairDialog, PairData};

/// Spring physics for smooth temperature animation
mod animation {
    pub const SPRING_STIFFNESS: f32 = 25.0;
    pub const DAMPING_COEFFICIENT: f32 = 10.0;
    pub const SETTLE_THRESHOLD: f32 = 0.01;
    pub const VELOCITY_THRESHOLD: f32 = 0.02;
}

/// Animation state for smooth temperature transitions
struct TempAnimation {
    target: f32,
    display: f32,
    velocity: f32,
    last_time: Option<i64>,
}

impl TempAnimation {
    fn new(initial: f32) -> Self {
        Self {
            target: initial,
            display: initial,
            velocity: 0.0,
            last_time: None,
        }
    }

    fn set_target(&mut self, new_target: f32) {
        self.target = new_target;
    }

    fn tick(&mut self, frame_time: i64) -> bool {
        let dt = match self.last_time {
            Some(last) => ((frame_time - last) as f64 / 1_000_000.0).min(0.05) as f32,
            None => 0.016,
        };
        self.last_time = Some(frame_time);

        let diff = self.target - self.display;
        
        if diff.abs() < animation::SETTLE_THRESHOLD && self.velocity.abs() < animation::VELOCITY_THRESHOLD {
            self.display = self.target;
            self.velocity = 0.0;
            return false;
        }

        let acceleration = animation::SPRING_STIFFNESS * diff - animation::DAMPING_COEFFICIENT * self.velocity;
        self.velocity += acceleration * dt;
        self.display += self.velocity * dt;
        true
    }
}

/// Dashboard state
#[derive(Default)]
pub struct DashboardState {
    pub curves: Vec<hf_core::PersistedCurve>,
    pub pairs: Vec<PairData>,
}

/// Data for backward compatibility with CurveCard
#[derive(Clone)]
pub struct CurveCardData {
    pub id: String,
    pub name: String,
    pub temp_source_path: String,
    pub temp_source_label: String,
    pub points: Vec<(f32, f32)>,
    pub current_temp: f32,
    /// Hysteresis in degrees Celsius
    pub hysteresis: f32,
    /// Delay in milliseconds before responding to temperature changes
    pub delay_ms: u32,
    /// Ramp up speed in percent per second
    pub ramp_up_speed: f32,
    /// Ramp down speed in percent per second
    pub ramp_down_speed: f32,
}

/// Main dashboard widget
pub struct Dashboard {
    pub container: GtkBox,
    state: Rc<RefCell<DashboardState>>,
    curves_list: GtkBox,
    pairs_list: GtkBox,
    pairs_section: GtkBox,
    curves_empty: adw::StatusPage,
    pairs_empty: adw::StatusPage,
    curves_stack: gtk4::Stack,
    pairs_stack: gtk4::Stack,
    add_pair_btn: Button,
    on_navigate_curves: Rc<RefCell<Option<Box<dyn Fn()>>>>,
}

impl Dashboard {
    pub fn new() -> Self {
        let container = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .build();

        let state = Rc::new(RefCell::new(DashboardState::default()));

        // Load persisted curves
        let persisted = hf_core::load_curves().unwrap_or_else(|e| {
            warn!("Failed to load curves: {}", e);
            hf_core::CurveStore::new()
        });

        for curve in persisted.all() {
            state.borrow_mut().curves.push(curve.clone());
        }

        debug!("Loaded {} persisted curves", state.borrow().curves.len());

        // Load pairs from settings
        if let Ok(settings) = hf_core::load_settings() {
            let curves_store = hf_core::load_curves().unwrap_or_else(|_| hf_core::CurveStore::new());
            
            for pair in settings.active_pairs {
                // Find the curve to get its points
                if let Some(curve) = curves_store.all().iter().find(|c| c.id == pair.curve_id) {
                    // Use fan_paths if available, otherwise fall back to single fan_path
                    let fan_paths = if !pair.fan_paths.is_empty() {
                        pair.fan_paths.clone()
                    } else {
                        vec![pair.fan_path.clone()]
                    };
                    let pair_data = PairData {
                        id: pair.id,
                        name: pair.name,
                        curve_id: pair.curve_id,
                        curve_name: curve.name.clone(),
                        temp_source_path: pair.temp_source_path,
                        temp_source_label: String::new(),
                        fan_path: pair.fan_path.clone(),
                        fan_label: String::new(),
                        fan_paths,
                        fan_labels: vec![],
                        points: curve.points.clone(),
                        hysteresis_ms: pair.hysteresis_ms,
                    };
                    state.borrow_mut().pairs.push(pair_data);
                }
            }
            debug!("Loaded {} pairs from settings", state.borrow().pairs.len());
        }

        // Main scroll container
        let scroll = ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .build();

        let main_content = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(24)
            .margin_start(24)
            .margin_end(24)
            .margin_top(24)
            .margin_bottom(24)
            .build();

        // Hidden elements for internal tracking (not displayed, but need valid stack children)
        let curves_list = GtkBox::new(Orientation::Vertical, 0);
        let curves_stack = gtk4::Stack::new();
        let curves_empty = adw::StatusPage::new();
        // Add children to curves_stack to avoid GTK warnings
        curves_stack.add_named(&curves_empty, Some("empty"));
        curves_stack.add_named(&curves_list, Some("list"));

        // ================================================================
        // Active Controls Section (the ONLY section on dashboard)
        // ================================================================
        let pairs_section = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(12)
            .build();

        let pairs_header = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .margin_bottom(6)
            .build();

        let pairs_title = Label::builder()
            .label("Active Controls")
            .css_classes(["title-1"])
            .hexpand(true)
            .halign(gtk4::Align::Start)
            .build();

        let refresh_btn = Button::builder()
            .icon_name("view-refresh-symbolic")
            .css_classes(["circular", "flat"])
            .tooltip_text("Refresh dashboard data")
            .build();

        let add_pair_btn = Button::builder()
            .icon_name("list-add-symbolic")
            .css_classes(["fab", "suggested-action"])
            .tooltip_text("Create Fan-Curve Pair")
            .sensitive(false) // Disabled until curves exist
            .build();

        pairs_header.append(&pairs_title);
        pairs_header.append(&refresh_btn);
        pairs_header.append(&add_pair_btn);
        pairs_section.append(&pairs_header);

        // Pairs stack (no_curves state vs empty state vs list)
        let pairs_stack = gtk4::Stack::builder()
            .transition_type(gtk4::StackTransitionType::Crossfade)
            .transition_duration(150)
            .build();

        // State 1: No curves exist - prompt to create a curve first
        let no_curves_state = adw::StatusPage::builder()
            .icon_name("document-new-symbolic")
            .title("Create Your First Fan Curve")
            .description("Fan curves define how fan speed responds to temperature.\nCreate a curve first, then pair it with a fan.")
            .build();

        let go_to_curves_btn = Button::builder()
            .label("Go to Fan Curves")
            .css_classes(["suggested-action", "pill"])
            .halign(gtk4::Align::Center)
            .build();
        no_curves_state.set_child(Some(&go_to_curves_btn));

        // State 2: Curves exist but no pairs - prompt to create a pair
        let pairs_empty = adw::StatusPage::builder()
            .icon_name("emblem-synchronizing-symbolic")
            .title("No Active Controls")
            .description("Pair a fan with a curve to start automatic temperature-based control")
            .build();

        let pairs_empty_btn = Button::builder()
            .label("Create Your First Pair")
            .css_classes(["suggested-action", "pill"])
            .halign(gtk4::Align::Center)
            .build();
        pairs_empty.set_child(Some(&pairs_empty_btn));

        // State 3: Active pairs list
        let pairs_list = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(8)
            .build();

        pairs_stack.add_named(&no_curves_state, Some("no_curves"));
        pairs_stack.add_named(&pairs_empty, Some("empty"));
        pairs_stack.add_named(&pairs_list, Some("list"));
        pairs_section.append(&pairs_stack);

        main_content.append(&pairs_section);

        scroll.set_child(Some(&main_content));
        container.append(&scroll);

        // Add Ctrl+N keyboard shortcut to add new pair
        let key_controller = gtk4::EventControllerKey::new();
        let add_btn_for_keys = add_pair_btn.clone();
        key_controller.connect_key_pressed(move |_, key, _, modifiers| {
            if modifiers.contains(gtk4::gdk::ModifierType::CONTROL_MASK) {
                if matches!(key, gtk4::gdk::Key::n | gtk4::gdk::Key::N) {
                    if add_btn_for_keys.is_sensitive() {
                        add_btn_for_keys.activate();
                    }
                    return glib::Propagation::Stop;
                }
            }
            glib::Propagation::Proceed
        });
        container.add_controller(key_controller);

        let on_navigate_curves: Rc<RefCell<Option<Box<dyn Fn()>>>> = Rc::new(RefCell::new(None));

        let dashboard = Self {
            container,
            state,
            curves_list,
            pairs_list,
            pairs_section,
            curves_empty,
            pairs_empty,
            curves_stack,
            pairs_stack,
            add_pair_btn: add_pair_btn.clone(),
            on_navigate_curves,
        };

        // Load curves for internal state (needed for pair creation)
        dashboard.rebuild_curves_list();
        // Rebuild pairs list from loaded data and update add button sensitivity
        Self::rebuild_pairs_list_static(&dashboard.state, &dashboard.pairs_list, &dashboard.pairs_stack, &dashboard.add_pair_btn);
        
        // Wire up refresh button
        let state_for_refresh = dashboard.state.clone();
        let pairs_list_for_refresh = dashboard.pairs_list.clone();
        let pairs_stack_for_refresh = dashboard.pairs_stack.clone();
        let add_pair_btn_for_refresh = dashboard.add_pair_btn.clone();
        refresh_btn.connect_clicked(move |btn| {
            // Show loading state
            btn.set_sensitive(false);
            btn.set_icon_name("content-loading-symbolic");
            // Reload curves from disk
            let persisted = hf_core::load_curves().unwrap_or_else(|e| {
                tracing::warn!("Failed to load curves: {}", e);
                hf_core::CurveStore::new()
            });
            
            state_for_refresh.borrow_mut().curves.clear();
            for curve in persisted.all() {
                state_for_refresh.borrow_mut().curves.push(curve.clone());
            }
            
            // Reload pairs from settings
            state_for_refresh.borrow_mut().pairs.clear();
            if let Ok(settings) = hf_core::load_settings() {
                let curves_store = hf_core::load_curves().unwrap_or_else(|_| hf_core::CurveStore::new());
                
                for pair in settings.active_pairs {
                    if let Some(curve) = curves_store.all().iter().find(|c| c.id == pair.curve_id) {
                        // Use fan_paths if available, otherwise fall back to single fan_path
                        let fan_paths = if !pair.fan_paths.is_empty() {
                            pair.fan_paths.clone()
                        } else {
                            vec![pair.fan_path.clone()]
                        };
                        let pair_data = super::add_pair_dialog::PairData {
                            id: pair.id,
                            name: pair.name,
                            curve_id: pair.curve_id,
                            curve_name: curve.name.clone(),
                            temp_source_path: pair.temp_source_path,
                            temp_source_label: String::new(),
                            fan_path: pair.fan_path.clone(),
                            fan_label: String::new(),
                            fan_paths,
                            fan_labels: vec![],
                            points: curve.points.clone(),
                            hysteresis_ms: pair.hysteresis_ms,
                        };
                        state_for_refresh.borrow_mut().pairs.push(pair_data);
                    }
                }
            }
            
            // Rebuild UI
            Self::rebuild_pairs_list_static(&state_for_refresh, &pairs_list_for_refresh, &pairs_stack_for_refresh, &add_pair_btn_for_refresh);
            
            let curve_count = state_for_refresh.borrow().curves.len();
            let pair_count = state_for_refresh.borrow().pairs.len();
            
            tracing::debug!("Dashboard refreshed: {} curves, {} pairs", curve_count, pair_count);
            
            // Restore button state after a small delay to prevent spam
            let btn_clone = btn.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
                btn_clone.set_sensitive(true);
                btn_clone.set_icon_name("view-refresh-symbolic");
            });
            
            // Show success toast
            if let Some(window) = btn.root().and_then(|r| r.downcast::<gtk4::Window>().ok()) {
                let toast = adw::Toast::new(&format!("Refreshed: {} curves, {} pairs", curve_count, pair_count));
                toast.set_timeout(2);
                if let Some(toast_overlay) = window.child()
                    .and_then(|c| c.downcast::<adw::ToastOverlay>().ok()) 
                {
                    toast_overlay.add_toast(toast);
                }
            }
        });

        // Wire up "Go to Fan Curves" button
        let on_nav = dashboard.on_navigate_curves.clone();
        go_to_curves_btn.connect_clicked(move |_| {
            if let Some(ref callback) = *on_nav.borrow() {
                callback();
            }
        });

        // Wire up add pair buttons
        let state_for_pair = dashboard.state.clone();
        let pairs_list_for_pair = dashboard.pairs_list.clone();
        let pairs_stack_for_pair = dashboard.pairs_stack.clone();
        let add_pair_btn_for_pair = dashboard.add_pair_btn.clone();

        let add_pair_handler = move |btn: &Button| {
            let state_ref = state_for_pair.borrow();
            let curves = state_ref.curves.clone();
            let existing_pairs = state_ref.pairs.clone();
            drop(state_ref);
            
            if curves.is_empty() {
                return;
            }

            let dialog = AddPairDialog::new_with_existing_pairs(&curves, &existing_pairs);

            let state_for_dialog = state_for_pair.clone();
            let pairs_list_for_dialog = pairs_list_for_pair.clone();
            let pairs_stack_for_dialog = pairs_stack_for_pair.clone();
            let add_pair_btn_for_dialog = add_pair_btn_for_pair.clone();

            dialog.connect_create(move |data: PairData| {
                debug!("Created pair: {} with curve {}", data.name, data.curve_name);

                // Save pair to settings JSON
                let settings_pair = hf_core::FanCurvePair {
                    id: data.id.clone(),
                    name: data.name.clone(),
                    curve_id: data.curve_id.clone(),
                    temp_source_path: data.temp_source_path.clone(),
                    fan_path: data.fan_path.clone(),
                    fan_paths: data.fan_paths.clone(),
                    hysteresis_ms: data.hysteresis_ms,
                    active: true,
                };
                
                if let Err(e) = hf_core::save_pair(settings_pair) {
                    warn!("Failed to save pair to settings: {}", e);
                } else {
                    debug!("Pair saved to settings: {}", data.name);
                    // Signal daemon to reload config
                    if let Err(e) = hf_core::daemon_reload_config() {
                        debug!("Failed to signal daemon reload: {}", e);
                    } else {
                        debug!("Daemon reload signaled");
                    }
                }

                // Add to state
                state_for_dialog.borrow_mut().pairs.push(data.clone());

                // Rebuild pairs list
                Self::rebuild_pairs_list_static(
                    &state_for_dialog,
                    &pairs_list_for_dialog,
                    &pairs_stack_for_dialog,
                    &add_pair_btn_for_dialog,
                );
            });

            // Present dialog with the root window as parent for proper modal behavior
            if let Some(root) = btn.root() {
                if let Some(window) = root.downcast_ref::<gtk4::Window>() {
                    dialog.present(window);
                } else {
                    dialog.present(btn);
                }
            } else {
                dialog.present(btn);
            }
        };

        add_pair_btn.connect_clicked(add_pair_handler.clone());
        pairs_empty_btn.connect_clicked(add_pair_handler);

        // NOTE: Dashboard refresh is handled by window.rs via refresh_temps_only()
        // No separate timer needed here - avoids duplicate refresh cycles

        dashboard
    }

    fn rebuild_curves_list(&self) {
        // Curves are NOT displayed on dashboard anymore - they're on the Curves page
        // This method is kept for compatibility but does nothing
    }

    fn create_curve_card(
        curve: &hf_core::PersistedCurve,
        state: Rc<RefCell<DashboardState>>,
        curves_list: GtkBox,
        curves_stack: gtk4::Stack,
        pairs_section: Option<GtkBox>,
    ) -> adw::Bin {
        let card = adw::Bin::builder()
            .css_classes(["card"])
            .build();

        let content = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .margin_start(16)
            .margin_end(16)
            .margin_top(12)
            .margin_bottom(12)
            .build();

        // Curve mini-preview
        let preview = gtk4::DrawingArea::builder()
            .width_request(80)
            .height_request(50)
            .build();

        let points = curve.points.clone();
        preview.set_draw_func(move |_, cr, width, height| {
            Self::draw_mini_curve(cr, width, height, &points);
        });

        content.append(&preview);

        // Info
        let info = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(4)
            .hexpand(true)
            .valign(gtk4::Align::Center)
            .build();

        let name = Label::builder()
            .label(&curve.name)
            .css_classes(["title-4"])
            .halign(gtk4::Align::Start)
            .build();

        let detail = Label::builder()
            .label(&format!("{} points", curve.points.len()))
            .css_classes(["dim-label", "caption"])
            .halign(gtk4::Align::Start)
            .build();

        info.append(&name);
        info.append(&detail);
        content.append(&info);

        // Delete button
        let delete_btn = Button::builder()
            .icon_name("user-trash-symbolic")
            .css_classes(["flat", "circular", "destructive-action"])
            .tooltip_text("Delete Curve")
            .valign(gtk4::Align::Center)
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

            if let Some(root) = btn.root() {
                if let Some(window) = root.downcast_ref::<gtk4::Window>() {
                    confirm.choose(window, None::<&gtk4::gio::Cancellable>, move |response| {
                        if response == "delete" {
                            if let Err(e) = hf_core::delete_curve(&curve_id) {
                                warn!("Failed to delete curve: {}", e);
                            } else {
                                debug!("Deleted curve: {}", curve_id);
                            }
                            state.borrow_mut().curves.retain(|c| c.id != curve_id);
                        }
                    });
                }
            }
        });

        content.append(&delete_btn);
        card.set_child(Some(&content));
        card
    }

    fn rebuild_pairs_list_static(
        state: &Rc<RefCell<DashboardState>>,
        pairs_list: &GtkBox,
        pairs_stack: &gtk4::Stack,
        add_pair_btn: &Button,
    ) {
        // Clear existing
        while let Some(child) = pairs_list.first_child() {
            pairs_list.remove(&child);
        }

        let state_ref = state.borrow();
        let curves = &state_ref.curves;
        let pairs = &state_ref.pairs;
        let has_curves = !curves.is_empty();
        let pairs_clone: Vec<_> = pairs.clone();
        drop(state_ref);

        // Update add button sensitivity based on whether curves exist
        add_pair_btn.set_sensitive(has_curves);

        // If no curves exist, show "Create your first fan curve" CTA
        if !has_curves {
            pairs_stack.set_visible_child_name("no_curves");
            return;
        }

        // If curves exist but no pairs, show "No Active Controls" CTA
        if pairs_clone.is_empty() {
            pairs_stack.set_visible_child_name("empty");
            return;
        }

        // Show pairs list
        pairs_stack.set_visible_child_name("list");

        for pair in &pairs_clone {
            let card = Self::create_pair_card(pair, state.clone(), pairs_list.clone(), pairs_stack.clone(), add_pair_btn.clone());
            pairs_list.append(&card);
        }
    }

    fn create_pair_card(
        pair: &PairData,
        state: Rc<RefCell<DashboardState>>,
        pairs_list: GtkBox,
        pairs_stack: gtk4::Stack,
        add_pair_btn: Button,
    ) -> adw::Bin {
        let card = adw::Bin::builder()
            .css_classes(["card", "activatable"])
            .build();
        
        // Make card clickable for editing
        let gesture = gtk4::GestureClick::new();
        let pair_for_edit = pair.clone();
        let state_for_edit = state.clone();
        let pairs_list_for_edit = pairs_list.clone();
        let pairs_stack_for_edit = pairs_stack.clone();
        let add_pair_btn_for_edit = add_pair_btn.clone();
        gesture.connect_released(move |gesture, _, _, _| {
            if let Some(widget) = gesture.widget() {
                Self::show_edit_pair_dialog(
                    &widget,
                    &pair_for_edit,
                    &state_for_edit,
                    &pairs_list_for_edit,
                    &pairs_stack_for_edit,
                    &add_pair_btn_for_edit,
                );
            }
        });
        card.add_controller(gesture);

        let content = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(8)
            .margin_start(16)
            .margin_end(16)
            .margin_top(12)
            .margin_bottom(12)
            .build();

        // Header row
        let header = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .build();

        let name = Label::builder()
            .label(&pair.name)
            .css_classes(["title-3"])
            .hexpand(true)
            .halign(gtk4::Align::Start)
            .build();

        // Live temp display
        let temp_placeholder = format!("--{}", hf_core::display::temp_unit_suffix());
        let temp_label = Label::builder()
            .label(&temp_placeholder)
            .css_classes(["title-2", "numeric"])
            .build();

        let arrow = Label::builder()
            .label("→")
            .css_classes(["dim-label"])
            .build();

        let percent_placeholder = format!("--{}", hf_core::display::fan_metric_suffix());
        let percent_label = Label::builder()
            .label(&percent_placeholder)
            .css_classes(["title-2", "numeric", "accent"])
            .build();

        let delete_btn = Button::builder()
            .icon_name("user-trash-symbolic")
            .css_classes(["flat", "circular", "destructive-action"])
            .tooltip_text("Remove Pair")
            .build();

        header.append(&name);
        header.append(&temp_label);
        header.append(&arrow);
        header.append(&percent_label);
        header.append(&delete_btn);
        content.append(&header);

        // Info row - only show non-empty labels
        let mut info_parts = vec![pair.curve_name.clone()];
        if !pair.temp_source_label.is_empty() {
            info_parts.push(pair.temp_source_label.clone());
        }
        if !pair.fan_label.is_empty() {
            info_parts.push(pair.fan_label.clone());
        }
        let info_text = info_parts.join(" • ");
        
        let info = Label::builder()
            .label(&info_text)
            .css_classes(["dim-label", "caption"])
            .halign(gtk4::Align::Start)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .build();
        content.append(&info);

        // Live curve preview with current temperature indicator
        let preview = gtk4::DrawingArea::builder()
            .height_request(100)
            .hexpand(true)
            .build();

        // Store data for live updates with animation
        let points = pair.points.clone();
        let temp_path = pair.temp_source_path.clone();
        let anim_state: Rc<RefCell<TempAnimation>> = Rc::new(RefCell::new(TempAnimation::new(40.0)));

        // Draw function uses animated temperature from RefCell
        let anim_for_draw = anim_state.clone();
        let points_for_draw = points.clone();
        preview.set_draw_func(move |_, cr, width, height| {
            let temp = anim_for_draw.borrow().display;
            Self::draw_live_curve(cr, width, height, &points_for_draw, temp);
        });

        content.append(&preview);
        
        debug!("Widget tree built for '{}' - temp_label parent: {:?}, percent_label parent: {:?}", 
               pair.name, temp_label.parent().is_some(), percent_label.parent().is_some());

        // Initialize with current temp reading from cached runtime data
        let initial_temp = crate::runtime::get_sensors()
            .and_then(|data| {
                debug!("Control pair '{}' initializing - runtime cache has {} temps", 
                       pair.name, data.temperatures.len());
                if data.temperatures.is_empty() {
                    debug!("Runtime cache is empty during control pair initialization");
                }
                debug!("Looking for temp path: {}", temp_path);
                debug!("Available temp paths: {:?}", 
                       data.temperatures.iter().map(|t| &t.path).collect::<Vec<_>>());
                data.temperatures.iter()
                    .find(|t| t.path == temp_path)
                    .map(|t| t.temp_celsius)
            })
            .or_else(|| {
                debug!("Control pair '{}' - runtime cache miss, using daemon read for: {}", 
                       pair.name, temp_path);
                daemon_client::daemon_read_temperature(&temp_path).ok()
            });
        
        if let Some(temp) = initial_temp {
            debug!("Control pair '{}' initialized with temp: {:.1}°C", pair.name, temp);
            anim_state.borrow_mut().target = temp;
            anim_state.borrow_mut().display = temp;
            let temp_str = hf_core::display::format_temp_precise(temp);
            let percent = Self::interpolate(&points, temp);
            let percent_str = hf_core::display::format_fan_speed_f32(percent);
            temp_label.set_label(&temp_str);
            percent_label.set_label(&percent_str);
            debug!("Control pair '{}' - initial labels set: temp='{}' percent='{}'", pair.name, temp_str, percent_str);
        } else {
            warn!("Control pair '{}' failed to get initial temperature for path: {}", pair.name, temp_path);
        }
        
        // Set up polling timer to update temperature from cached runtime data
        let poll_interval_ms = hf_core::get_cached_settings().general.poll_interval_ms as u64;
        let poll_interval_ms = poll_interval_ms.max(50);
        
        debug!("Setting up polling timer for '{}' with interval {}ms for path: {}", 
               pair.name, poll_interval_ms, temp_path);
        
        let anim_for_poll = anim_state.clone();
        let temp_label_for_poll = temp_label.clone();
        let percent_label_for_poll = percent_label.clone();
        let points_for_poll = points.clone();
        let temp_path_for_poll = temp_path.clone();
        let pair_name_for_poll = pair.name.clone();
        
        // CRITICAL: Use weak reference to card so timeout stops when card is destroyed
        // This prevents memory leak when pairs are removed/rebuilt
        let card_weak = glib::SendWeakRef::from(card.downgrade());
        
        let poll_count = Rc::new(RefCell::new(0u32));
        let poll_count_for_timer = poll_count.clone();
        
        let pair_name_for_log = pair_name_for_poll.clone();
        debug!("Polling timer created for '{}'", pair_name_for_log);
        
        glib::timeout_add_local(std::time::Duration::from_millis(poll_interval_ms), move || {
            // Check if card still exists - if not, stop the timer
            if card_weak.upgrade().is_none() {
                debug!("Card '{}' destroyed, stopping temperature poll timer", pair_name_for_poll);
                return glib::ControlFlow::Break;
            }
            
            let count = *poll_count_for_timer.borrow();
            *poll_count_for_timer.borrow_mut() = count + 1;
            
            // Log first poll and every 10th poll
            if count == 0 {
                debug!("FIRST POLL for '{}' - timer is running!", pair_name_for_poll);
            }
            if count % 10 == 0 {
                debug!("Active Controls poll #{} for '{}' path: {}", count, pair_name_for_poll, temp_path_for_poll);
            }
            
            // PERFORMANCE: Use cached sensor data from runtime worker (non-blocking)
            let temp_result = crate::runtime::get_sensors()
                .and_then(|data| {
                    let found = data.temperatures.iter()
                        .find(|t| t.path == temp_path_for_poll)
                        .map(|t| t.temp_celsius);
                    
                    if found.is_none() && count % 10 == 0 {
                        debug!("Temperature lookup failed for path: {}", temp_path_for_poll);
                        debug!("Available paths in cache: {:?}", 
                               data.temperatures.iter().map(|t| &t.path).take(5).collect::<Vec<_>>());
                    }
                    found
                })
                .or_else(|| {
                    if count % 10 == 0 {
                        debug!("Falling back to daemon read for: {}", temp_path_for_poll);
                    }
                    daemon_client::daemon_read_temperature(&temp_path_for_poll).ok()
                });
            
            if let Some(temp) = temp_result {
                let old_target = anim_for_poll.borrow().target;
                
                // CRITICAL DEBUG: Log every single update attempt
                if count == 0 || count % 5 == 0 {
                    debug!("UPDATING LABELS for '{}': temp={:.1}°C", pair_name_for_poll, temp);
                }
                
                // Always update to ensure UI reflects current sensor state
                // Animation will smooth out the visual changes
                anim_for_poll.borrow_mut().set_target(temp);
                
                let temp_str = hf_core::display::format_temp_precise(temp);
                temp_label_for_poll.set_label(&temp_str);
                
                let percent = Self::interpolate(&points_for_poll, temp);
                let percent_str = hf_core::display::format_fan_speed_f32(percent);
                percent_label_for_poll.set_label(&percent_str);
                
                if count == 0 || count % 5 == 0 {
                    debug!("LABELS SET for '{}': temp='{}' percent='{}'", pair_name_for_poll, temp_str, percent_str);
                }
                
                if (temp - old_target).abs() > 0.5 {
                    debug!("Active Controls temp updated: {:.1}°C -> {:.1}°C ({}%)", old_target, temp, percent);
                }
            } else {
                if count % 10 == 0 {
                    warn!("Active Controls failed to read temperature for path: {}", temp_path_for_poll);
                }
            }
            glib::ControlFlow::Continue
        });
        
        debug!("Polling timer REGISTERED for '{}' - timer should start firing in {}ms", pair.name, poll_interval_ms);
        
        // Animation frame timer for smooth indicator movement (60fps)
        let anim_for_frame = anim_state.clone();
        let preview_for_frame = preview.clone();
        let card_weak_frame = glib::SendWeakRef::from(card.downgrade());
        
        glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
            if card_weak_frame.upgrade().is_none() {
                return glib::ControlFlow::Break;
            }
            
            let frame_time = glib::monotonic_time();
            let needs_redraw = anim_for_frame.borrow_mut().tick(frame_time);
            
            if needs_redraw {
                preview_for_frame.queue_draw();
            }
            
            glib::ControlFlow::Continue
        });

        let pair_id = pair.id.clone();
        let pair_id_for_settings = pair.id.clone();
        let pair_name = pair.name.clone();
        delete_btn.connect_clicked(move |btn| {
            let confirm = libadwaita::AlertDialog::builder()
                .heading("Remove Fan Control?")
                .body(&format!("Are you sure you want to remove \"{}\"? The fan will return to automatic control.", pair_name))
                .build();

            confirm.add_response("cancel", "Cancel");
            confirm.add_response("remove", "Remove");
            confirm.set_response_appearance("remove", libadwaita::ResponseAppearance::Destructive);
            confirm.set_default_response(Some("cancel"));
            confirm.set_close_response("cancel");

            let pair_id = pair_id.clone();
            let pair_id_for_settings = pair_id_for_settings.clone();
            let state = state.clone();
            let pairs_list = pairs_list.clone();
            let pairs_stack = pairs_stack.clone();
            let add_pair_btn = add_pair_btn.clone();

            if let Some(root) = btn.root() {
                if let Some(window) = root.downcast_ref::<gtk4::Window>() {
                    confirm.choose(window, None::<&gtk4::gio::Cancellable>, move |response| {
                        if response == "remove" {
                            if let Err(e) = hf_core::delete_pair(&pair_id_for_settings) {
                                warn!("Failed to delete pair from settings: {}", e);
                            } else {
                                if let Err(e) = hf_core::daemon_reload_config() {
                                    debug!("Failed to signal daemon reload: {}", e);
                                }
                            }
                            // Remove from state
                            {
                                let mut state_mut = state.borrow_mut();
                                state_mut.pairs.retain(|p| p.id != pair_id);
                            } // Drop borrow before rebuild
                            Self::rebuild_pairs_list_static(&state, &pairs_list, &pairs_stack, &add_pair_btn);
                        }
                    });
                }
            }
        });

        card.set_child(Some(&content));
        card
    }

    fn draw_mini_curve(cr: &gtk4::cairo::Context, width: i32, height: i32, points: &[(f32, f32)]) {
        let w = width as f64;
        let h = height as f64;
        let m = 4.0;

        // Theme-aware background
        let bg = theme_colors::mini_graph_bg();
        cr.set_source_rgba(bg.0, bg.1, bg.2, bg.3);
        cr.rectangle(0.0, 0.0, w, h);
        let _ = cr.fill();

        if points.is_empty() { return; }

        // PERFORMANCE: Use cached settings (no disk I/O in draw function)
        let graph_style = hf_core::get_graph_style();

        let temp_to_x = |t: f32| m + ((t - 20.0) / 80.0) as f64 * (w - 2.0 * m);
        let pct_to_y = |p: f32| h - m - (p / 100.0) as f64 * (h - 2.0 * m);

        // Theme-aware fill (only for "filled" style)
        if graph_style == "filled" {
            let fill = theme_colors::curve_fill();
            cr.set_source_rgba(fill.0, fill.1, fill.2, fill.3 * 2.0);
            cr.move_to(temp_to_x(20.0), pct_to_y(0.0));
            cr.line_to(temp_to_x(20.0), pct_to_y(points[0].1));
            for (t, p) in points { cr.line_to(temp_to_x(*t), pct_to_y(*p)); }
            if let Some((_, last_p)) = points.last() { cr.line_to(temp_to_x(100.0), pct_to_y(*last_p)); }
            cr.line_to(temp_to_x(100.0), pct_to_y(0.0));
            cr.close_path();
            let _ = cr.fill();
        }

        // Theme-aware line
        let line = theme_colors::curve_line();
        cr.set_source_rgb(line.0, line.1, line.2);
        cr.set_line_width(1.5);
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
        if let Some((_, last_p)) = points.last() { cr.line_to(temp_to_x(100.0), pct_to_y(*last_p)); }
        let _ = cr.stroke();
    }

    fn draw_live_curve(cr: &gtk4::cairo::Context, width: i32, height: i32, points: &[(f32, f32)], current_temp: f32) {
        let w = width as f64;
        let h = height as f64;
        let m = 8.0;
        let radius = 8.0;

        // Theme-aware background with rounded corners
        let bg = theme_colors::graph_bg();
        cr.set_source_rgba(bg.0, bg.1, bg.2, bg.3);
        
        // Draw rounded rectangle
        cr.new_path();
        cr.arc(radius, radius, radius, std::f64::consts::PI, 1.5 * std::f64::consts::PI);
        cr.arc(w - radius, radius, radius, 1.5 * std::f64::consts::PI, 2.0 * std::f64::consts::PI);
        cr.arc(w - radius, h - radius, radius, 0.0, 0.5 * std::f64::consts::PI);
        cr.arc(radius, h - radius, radius, 0.5 * std::f64::consts::PI, std::f64::consts::PI);
        cr.close_path();
        let _ = cr.fill();

        if points.is_empty() { return; }

        // PERFORMANCE: Use cached settings (no disk I/O in draw function)
        let graph_style = hf_core::get_graph_style();

        let temp_to_x = |t: f32| m + ((t - 20.0) / 80.0) as f64 * (w - 2.0 * m);
        let pct_to_y = |p: f32| h - m - (p / 100.0) as f64 * (h - 2.0 * m);

        // Theme-aware fill (only for "filled" style)
        if graph_style == "filled" {
            let fill = theme_colors::curve_fill();
            cr.set_source_rgba(fill.0, fill.1, fill.2, fill.3);
            cr.move_to(temp_to_x(20.0), pct_to_y(0.0));
            cr.line_to(temp_to_x(20.0), pct_to_y(points[0].1));
            for (t, p) in points { cr.line_to(temp_to_x(*t), pct_to_y(*p)); }
            if let Some((_, last_p)) = points.last() { cr.line_to(temp_to_x(100.0), pct_to_y(*last_p)); }
            cr.line_to(temp_to_x(100.0), pct_to_y(0.0));
            cr.close_path();
            let _ = cr.fill();
        }

        // Theme-aware line
        let line = theme_colors::curve_line();
        cr.set_source_rgb(line.0, line.1, line.2);
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
        if let Some((_, last_p)) = points.last() { cr.line_to(temp_to_x(100.0), pct_to_y(*last_p)); }
        let _ = cr.stroke();

        // Theme-aware points
        cr.set_source_rgb(line.0, line.1, line.2);
        for (t, p) in points {
            cr.arc(temp_to_x(*t), pct_to_y(*p), 3.0, 0.0, 2.0 * std::f64::consts::PI);
            let _ = cr.fill();
        }

        // Current temp indicator - theme aware
        let clamped_temp = current_temp.clamp(20.0, 100.0);
        let current_percent = Self::interpolate(points, current_temp);
        let ix = temp_to_x(clamped_temp);
        let iy = pct_to_y(current_percent);

        // Vertical line
        let ind_line = theme_colors::indicator_line();
        cr.set_source_rgba(ind_line.0, ind_line.1, ind_line.2, ind_line.3);
        cr.set_line_width(2.0);
        cr.move_to(ix, m);
        cr.line_to(ix, h - m);
        let _ = cr.stroke();

        // Indicator dot
        let ind = theme_colors::indicator();
        cr.set_source_rgb(ind.0, ind.1, ind.2);
        cr.arc(ix, iy, 5.0, 0.0, 2.0 * std::f64::consts::PI);
        let _ = cr.fill();
        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.set_line_width(2.0);
        cr.arc(ix, iy, 5.0, 0.0, 2.0 * std::f64::consts::PI);
        let _ = cr.stroke();
    }

    fn show_edit_pair_dialog(
        widget: &gtk4::Widget,
        pair: &PairData,
        state: &Rc<RefCell<DashboardState>>,
        pairs_list: &GtkBox,
        pairs_stack: &gtk4::Stack,
        add_pair_btn: &Button,
    ) {
        // Load curves for the dialog
        let curves_store = match hf_core::load_curves() {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to load curves for edit dialog: {}", e);
                return;
            }
        };
        let curves: Vec<hf_core::PersistedCurve> = curves_store.all().into_iter().cloned().collect();

        // Create edit dialog using AddPairDialog
        let dialog = AddPairDialog::new_for_edit(&curves, pair);

        let state_for_dialog = state.clone();
        let pairs_list_for_dialog = pairs_list.clone();
        let pairs_stack_for_dialog = pairs_stack.clone();
        let add_pair_btn_for_dialog = add_pair_btn.clone();

        dialog.connect_create(move |data: PairData| {
            debug!("Edit pair callback: id={}, name={}", data.id, data.name);
            
            // Save updated pair to settings
            let settings_pair = hf_core::FanCurvePair {
                id: data.id.clone(),
                name: data.name.clone(),
                curve_id: data.curve_id.clone(),
                temp_source_path: data.temp_source_path.clone(),
                fan_path: data.fan_path.clone(),
                fan_paths: data.fan_paths.clone(),
                hysteresis_ms: data.hysteresis_ms,
                active: true,
            };
            
            if let Err(e) = hf_core::save_pair(settings_pair) {
                warn!("Failed to save updated pair: {}", e);
            } else {
                debug!("Pair updated in settings: {}", data.name);
                // Signal daemon to reload config
                if let Err(e) = hf_core::daemon_reload_config() {
                    debug!("Failed to signal daemon reload: {}", e);
                } else {
                    debug!("Daemon reload signaled");
                }
            }

            // Update in state
            {
                let mut state_ref = state_for_dialog.borrow_mut();
                if let Some(p) = state_ref.pairs.iter_mut().find(|p| p.id == data.id) {
                    *p = data.clone();
                }
            }

            // Rebuild UI
            Self::rebuild_pairs_list_static(
                &state_for_dialog,
                &pairs_list_for_dialog,
                &pairs_stack_for_dialog,
                &add_pair_btn_for_dialog,
            );
        });

        dialog.present(widget);
    }

    fn interpolate(points: &[(f32, f32)], temp: f32) -> f32 {
        if points.is_empty() { return 100.0; }
        if temp <= points[0].0 { return points[0].1; }
        let last_point = match points.last() {
            Some(p) => p,
            None => return 100.0,
        };
        if temp >= last_point.0 { return last_point.1; }

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

    fn refresh_pair_temps(_state: &Rc<RefCell<DashboardState>>, _pairs_list: &GtkBox) {
        // No-op: Each pair card now has its own polling timer that updates from
        // cached runtime sensor data. This avoids the previous performance issue
        // of blindly calling queue_draw() on all children every poll interval.
        // Individual cards only redraw when their temperature actually changes.
    }

    /// Light refresh - only update live temperatures, don't reload from disk
    pub fn refresh_temps_only(&self) {
        Self::refresh_pair_temps(&self.state, &self.pairs_list);
    }

    /// Full refresh - reload curves from disk and rebuild UI
    pub fn refresh(&self) {
        // Reload curves from disk (they may have been created/edited in CurvesPage)
        let persisted = hf_core::load_curves().unwrap_or_else(|_| hf_core::CurveStore::new());
        
        {
            let mut state = self.state.borrow_mut();
            state.curves.clear();
            for curve in persisted.all() {
                state.curves.push(curve.clone());
            }
            
            // Update pairs' curve points from the reloaded curves
            // This ensures dashboard shows updated curves after editing in CurvesPage
            for pair in state.pairs.iter_mut() {
                if let Some(curve) = persisted.get(&pair.curve_id) {
                    pair.points = curve.points.clone();
                }
            }
        } // Drop mutable borrow before rebuild
        
        // Rebuild UI
        Self::rebuild_pairs_list_static(&self.state, &self.pairs_list, &self.pairs_stack, &self.add_pair_btn);
    }

    /// Connect a callback for when user wants to navigate to curves page
    pub fn connect_navigate_curves<F: Fn() + 'static>(&self, callback: F) {
        *self.on_navigate_curves.borrow_mut() = Some(Box::new(callback));
    }

    pub fn widget(&self) -> &GtkBox {
        &self.container
    }
}

impl Default for Dashboard {
    fn default() -> Self {
        Self::new()
    }
}
