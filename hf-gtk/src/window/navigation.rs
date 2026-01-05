//! Navigation module - Sidebar navigation with page switching and state management.
//!
//! This module provides a complete navigation system for the application including:
//!
//! - **Navigation bar**: Vertical sidebar with icon buttons for each page
//! - **State management**: Centralized navigation state with active page tracking
//! - **Animated icons**: Support for continuously animated button icons (e.g., spinning fan)
//! - **Keyboard shortcuts**: Ctrl+1-5 for quick page switching
//!
//! # Architecture
//!
//! The navigation system uses a [`Navigator`] struct that encapsulates all navigation
//! state and provides a clean API for page switching. This eliminates the need for
//! scattered clones and callbacks throughout the codebase.
//!
//! # Example
//!
//! ```rust,ignore
//! let navigator = Navigator::new(stack, settings_page);
//! navigator.add_page(NavPage::Dashboard, dash_btn, Some(on_activate));
//! navigator.navigate_to(NavPage::Curves);
//! ```

use gtk4::prelude::*;
use gtk4::{Button, Image};
use std::cell::RefCell;
use std::rc::Rc;

use crate::widgets::NavPage;

// ============================================================================
// Constants
// ============================================================================

/// Size of navigation button icons in pixels
const NAV_ICON_SIZE: i32 = 32;

/// Size of navigation buttons (width and height) in pixels
const NAV_BUTTON_SIZE: i32 = 40;

/// Rotation speed for animated icons in degrees per second
const ICON_ROTATION_SPEED: f32 = 120.0;

/// Default frame time for animation (60fps fallback)
const DEFAULT_FRAME_TIME: f32 = 0.016;

/// Full rotation in degrees
const FULL_ROTATION: f32 = 360.0;

// ============================================================================
// NavButton
// ============================================================================

/// A navigation button with associated metadata.
#[derive(Clone)]
pub struct NavButton {
    /// The GTK button widget
    pub button: Button,
    /// The page this button navigates to
    pub page: NavPage,
    /// Stack page name for navigation
    pub stack_name: &'static str,
}

impl NavButton {
    /// Create a new navigation button.
    ///
    /// # Arguments
    ///
    /// * `page` - The navigation page enum value
    /// * `icon_name` - GTK icon name (e.g., "go-home-symbolic")
    /// * `tooltip` - Tooltip text shown on hover
    /// * `stack_name` - Name of the stack child to show when clicked
    pub fn new(page: NavPage, icon_name: &str, tooltip: &str, stack_name: &'static str) -> Self {
        let button = create_nav_button(icon_name, tooltip);
        Self { button, page, stack_name }
    }

    /// Create a navigation button with an animated icon.
    ///
    /// Returns the button along with animation controls.
    pub fn new_animated(
        page: NavPage,
        icon_name: &str,
        tooltip: &str,
        stack_name: &'static str,
    ) -> (Self, AnimationHandle) {
        let (button, drawing_area, rotation) = create_animated_nav_button(icon_name, tooltip);
        let nav_button = Self { button, page, stack_name };
        let handle = AnimationHandle { drawing_area, rotation };
        (nav_button, handle)
    }

    /// Set whether this button appears as the active/selected page.
    pub fn set_active(&self, active: bool) {
        if active {
            self.button.add_css_class("suggested-action");
        } else {
            self.button.remove_css_class("suggested-action");
        }
    }
}

// ============================================================================
// AnimationHandle
// ============================================================================

/// Handle for controlling icon animations.
///
/// Returned when creating animated nav buttons. Call [`start`](Self::start)
/// to begin the animation.
pub struct AnimationHandle {
    drawing_area: gtk4::DrawingArea,
    rotation: Rc<RefCell<f32>>,
}

impl AnimationHandle {
    /// Start the spinning animation.
    ///
    /// The animation runs continuously using the GTK frame clock for smooth,
    /// frame-rate independent rotation.
    pub fn start(self) {
        setup_icon_spin_animation(self.drawing_area, self.rotation);
    }
}

// ============================================================================
// Navigator
// ============================================================================

/// Centralized navigation state manager.
///
/// Manages page switching, active button state, and navigation guards
/// (e.g., unsaved changes prompts).
#[derive(Clone)]
pub struct Navigator {
    inner: Rc<NavigatorInner>,
}

struct NavigatorInner {
    stack: gtk4::Stack,
    buttons: RefCell<Vec<NavButton>>,
    settings_button: RefCell<Option<Button>>,
    current_page: RefCell<NavPage>,
}

impl Navigator {
    /// Create a new navigator for the given stack.
    ///
    /// # Arguments
    ///
    /// * `stack` - The GTK stack containing all pages
    pub fn new(stack: gtk4::Stack) -> Self {
        Self {
            inner: Rc::new(NavigatorInner {
                stack,
                buttons: RefCell::new(Vec::new()),
                settings_button: RefCell::new(None),
                current_page: RefCell::new(NavPage::Dashboard),
            }),
        }
    }

    /// Register a navigation button.
    pub fn add_button(&self, nav_button: NavButton) {
        self.inner.buttons.borrow_mut().push(nav_button);
    }

    /// Set the settings button (separate from main nav buttons).
    pub fn set_settings_button(&self, button: Button) {
        *self.inner.settings_button.borrow_mut() = Some(button);
    }

    /// Navigate to a page, updating button states.
    ///
    /// # Arguments
    ///
    /// * `page` - The page to navigate to
    pub fn navigate_to(&self, page: NavPage) {
        let buttons = self.inner.buttons.borrow();
        
        // Find the target button and navigate
        if let Some(nav_btn) = buttons.iter().find(|b| b.page == page) {
            self.inner.stack.set_visible_child_name(nav_btn.stack_name);
            *self.inner.current_page.borrow_mut() = page;
            
            // Update active states
            for btn in buttons.iter() {
                btn.set_active(btn.page == page);
            }
            
            // Clear settings button highlight
            if let Some(ref settings_btn) = *self.inner.settings_button.borrow() {
                settings_btn.remove_css_class("suggested-action");
            }
        }
    }

    /// Navigate to the settings page.
    pub fn navigate_to_settings(&self) {
        self.inner.stack.set_visible_child_name("settings");
        
        // Clear all nav button highlights
        for btn in self.inner.buttons.borrow().iter() {
            btn.set_active(false);
        }
        
        // Highlight settings button
        if let Some(ref settings_btn) = *self.inner.settings_button.borrow() {
            settings_btn.add_css_class("suggested-action");
        }
    }

    /// Get the currently visible page.
    pub fn current_page(&self) -> NavPage {
        *self.inner.current_page.borrow()
    }

    /// Check if currently on the settings page.
    pub fn is_on_settings(&self) -> bool {
        self.inner.stack.visible_child_name().as_deref() == Some("settings")
    }

    /// Get the underlying stack widget.
    pub fn stack(&self) -> &gtk4::Stack {
        &self.inner.stack
    }

    /// Get all registered buttons for iteration.
    pub fn buttons(&self) -> Vec<(NavPage, Button)> {
        self.inner.buttons.borrow()
            .iter()
            .map(|b| (b.page, b.button.clone()))
            .collect()
    }
}

// ============================================================================
// Button Creation Functions
// ============================================================================

/// Create a standard 50x50 navigation button with icon.
///
/// # Arguments
///
/// * `icon_name` - GTK icon name (e.g., "go-home-symbolic")
/// * `tooltip` - Tooltip text shown on hover
pub fn create_nav_button(icon_name: &str, tooltip: &str) -> Button {
    let icon = create_icon(icon_name, NAV_ICON_SIZE);

    Button::builder()
        .child(&icon)
        .width_request(NAV_BUTTON_SIZE)
        .height_request(NAV_BUTTON_SIZE)
        .tooltip_text(tooltip)
        .css_classes(["flat", "circular", "nav-button"])
        .build()
}

/// Create a navigation button with an animated (rotating) icon.
///
/// Returns the button, drawing area for rendering, and rotation state.
///
/// # Arguments
///
/// * `icon_name` - GTK icon name for the rotating icon
/// * `tooltip` - Tooltip text shown on hover
pub fn create_animated_nav_button(
    icon_name: &str,
    tooltip: &str,
) -> (Button, gtk4::DrawingArea, Rc<RefCell<f32>>) {
    let drawing_area = gtk4::DrawingArea::builder()
        .width_request(NAV_ICON_SIZE)
        .height_request(NAV_ICON_SIZE)
        .build();

    let rotation = Rc::new(RefCell::new(0.0f32));
    let rotation_for_draw = rotation.clone();
    let icon_name_owned = icon_name.to_string();

    // Set up draw function with rotation transform
    drawing_area.set_draw_func(move |area, cr, width, height| {
        draw_rotating_icon(area, cr, width, height, &icon_name_owned, &rotation_for_draw);
    });

    let button = Button::builder()
        .child(&drawing_area)
        .width_request(NAV_BUTTON_SIZE)
        .height_request(NAV_BUTTON_SIZE)
        .tooltip_text(tooltip)
        .css_classes(["flat", "circular", "nav-button"])
        .build();

    (button, drawing_area, rotation)
}

/// Draw a rotating icon using Cairo.
fn draw_rotating_icon(
    area: &gtk4::DrawingArea,
    cr: &gtk4::cairo::Context,
    width: i32,
    height: i32,
    icon_name: &str,
    rotation: &Rc<RefCell<f32>>,
) {
    let angle = *rotation.borrow();
    let fg_color = get_widget_foreground_color(area);
    let icon_paintable = load_icon_paintable_for_widget(icon_name, NAV_ICON_SIZE, area);

    if cr.save().is_err() {
        return;
    }

    // Transform: translate to center, rotate, scale for crisp rendering
    let center_x = width as f64 / 2.0;
    let center_y = height as f64 / 2.0;
    cr.translate(center_x, center_y);
    cr.rotate(angle as f64 * std::f64::consts::PI / 180.0);
    cr.scale(0.5, 0.5);
    cr.translate(-width as f64, -height as f64);

    // Render with symbolic colors
    if let Some(ref icon) = icon_paintable {
        use gtk4::prelude::SymbolicPaintableExt;

        let snapshot = gtk4::Snapshot::new();
        let colors = [fg_color, fg_color, fg_color, fg_color];
        icon.snapshot_symbolic(&snapshot, width as f64 * 2.0, height as f64 * 2.0, &colors);

        if let Some(node) = snapshot.to_node() {
            node.draw(cr);
        }
    }

    let _ = cr.restore();
}

// ============================================================================
// Animation
// ============================================================================

/// Setup continuous spinning animation for a drawing area.
///
/// Uses the GTK frame clock for smooth, frame-rate independent animation.
/// Respects the user's configured FPS limit.
pub fn setup_icon_spin_animation(drawing_area: gtk4::DrawingArea, rotation: Rc<RefCell<f32>>) {
    let last_frame_time = Rc::new(RefCell::new(0i64));

    drawing_area.add_tick_callback(move |widget, frame_clock| {
        let target_fps = hf_core::get_frame_rate();
        let target_frame_time_us = if target_fps == 0 {
            0
        } else {
            (1_000_000.0 / target_fps as f64) as i64
        };

        let current_time = frame_clock.frame_time();
        let last_time = *last_frame_time.borrow();

        // Throttle to target FPS
        if target_frame_time_us > 0 && last_time > 0 && (current_time - last_time) < target_frame_time_us {
            return gtk4::glib::ControlFlow::Continue;
        }

        *last_frame_time.borrow_mut() = current_time;

        // Frame-rate independent rotation
        let delta_seconds = if last_time > 0 {
            (current_time - last_time) as f32 / 1_000_000.0
        } else {
            DEFAULT_FRAME_TIME
        };

        let mut current_rotation = *rotation.borrow();
        current_rotation = (current_rotation + ICON_ROTATION_SPEED * delta_seconds) % FULL_ROTATION;
        *rotation.borrow_mut() = current_rotation;

        widget.queue_draw();
        gtk4::glib::ControlFlow::Continue
    });
}

// ============================================================================
// Active State Management
// ============================================================================

/// Update active button styling for a list of buttons.
///
/// Sets the `suggested-action` CSS class on the active button and removes
/// it from all others.
pub fn set_active_button(buttons: &[(NavPage, Button)], active: NavPage) {
    for (page, btn) in buttons {
        if *page == active {
            btn.add_css_class("suggested-action");
        } else {
            btn.remove_css_class("suggested-action");
        }
    }
}

// ============================================================================
// Icon Utilities
// ============================================================================

/// Create an icon image widget.
pub fn create_icon(icon_name: &str, size: i32) -> Image {
    Image::builder()
        .icon_name(icon_name)
        .pixel_size(size)
        .build()
}

/// Get the foreground color from a widget's CSS state.
///
/// Traverses to parent button if the widget is a child (e.g., DrawingArea).
fn get_widget_foreground_color(widget: &impl IsA<gtk4::Widget>) -> gtk4::gdk::RGBA {
    widget.parent()
        .and_then(|p| p.downcast_ref::<Button>().map(|b| b.color()))
        .unwrap_or_else(|| widget.as_ref().color())
}

/// Load an icon paintable for a widget context.
fn load_icon_paintable_for_widget(
    icon_name: &str,
    size: i32,
    widget: &impl IsA<gtk4::Widget>,
) -> Option<gtk4::IconPaintable> {
    let render_size = size * 2; // 2x for crisp rendering
    let display = widget.display();
    let theme = gtk4::IconTheme::for_display(&display);

    Some(theme.lookup_icon(
        icon_name,
        &[],
        render_size,
        widget.scale_factor(),
        widget.direction(),
        gtk4::IconLookupFlags::empty(),
    ))
}

/// Load an icon as a paintable (display-independent).
#[allow(dead_code)]
pub fn load_icon_paintable(icon_name: &str, size: i32) -> Option<gtk4::gdk::Paintable> {
    let render_size = size * 2;
    let display = gtk4::gdk::Display::default()?;
    let theme = gtk4::IconTheme::for_display(&display);

    Some(theme.lookup_icon(
        icon_name,
        &[],
        render_size,
        2,
        gtk4::TextDirection::Ltr,
        gtk4::IconLookupFlags::empty(),
    ).upcast())
}
