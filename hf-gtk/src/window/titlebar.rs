//! Titlebar module - Custom header bar with window controls and daemon indicator.
//!
//! This module provides a custom titlebar implementation that replaces the default
//! GTK/libadwaita window decorations with compact, tightly-spaced controls that
//! match the GNOME HIG while providing better visual density.
//!
//! # Features
//!
//! - **Custom window controls**: Minimize, maximize, and close buttons with 24px
//!   touch targets and 4px spacing (vs default 44px with large gaps)
//! - **Daemon status indicator**: Shows when the hyperfand daemon is running
//! - **Builder pattern**: Fluent API for configuring titlebar appearance
//!
//! # Example
//!
//! ```rust,ignore
//! let titlebar = Titlebar::builder(&window)
//!     .title("My App")
//!     .subtitle("v1.0.0")
//!     .show_daemon_indicator(true)
//!     .build();
//! ```

use gtk4::prelude::*;
use gtk4::Button;
use libadwaita as adw;

// ============================================================================
// Constants
// ============================================================================

/// Spacing between window control buttons in pixels
const WINDOW_CONTROL_SPACING: i32 = 4;

/// Default application title
const DEFAULT_TITLE: &str = "Hyperfan";

/// Icon names for window controls
const ICON_MINIMIZE: &str = "window-minimize-symbolic";
const ICON_MAXIMIZE: &str = "window-maximize-symbolic";
const ICON_RESTORE: &str = "window-restore-symbolic";
const ICON_CLOSE: &str = "window-close-symbolic";
const ICON_DAEMON: &str = "emblem-system-symbolic";
const ICON_SUPPORT: &str = "money-symbolic";

/// CSS classes for window controls
const CLASSES_WINDOW_CONTROL: &[&str] = &["flat", "window-control"];
const CLASSES_CLOSE_BUTTON: &[&str] = &["flat", "window-control", "close-button"];
const CLASSES_DAEMON_INDICATOR: &[&str] = &["flat", "daemon-active"];
const CLASSES_SUPPORT_BUTTON: &[&str] = &["flat", "support-button"];

/// Default tooltip text
const TOOLTIP_DAEMON_DEFAULT: &str = "Daemon service is running - Click to view settings";
const TOOLTIP_SUPPORT: &str = "Support Hyperfan development";

// ============================================================================
// Titlebar
// ============================================================================

/// A custom titlebar with window controls and optional daemon indicator.
///
/// This struct owns the header bar and provides access to interactive elements
/// that need to be connected to application logic.
#[derive(Debug, Clone)]
pub struct Titlebar {
    /// The libadwaita HeaderBar widget
    header: adw::HeaderBar,
    /// Button showing daemon connection status (hidden by default)
    daemon_indicator: Button,
    /// Button to show support dialog
    support_button: Button,
    /// The window title widget for dynamic updates
    title_widget: adw::WindowTitle,
}

impl Titlebar {
    /// Create a new titlebar builder for the given window.
    ///
    /// # Arguments
    ///
    /// * `window` - The application window to attach controls to
    pub fn builder(window: &adw::ApplicationWindow) -> TitlebarBuilder<'_> {
        TitlebarBuilder::new(window)
    }

    /// Get the header bar widget for adding to a container.
    #[inline]
    pub fn header(&self) -> &adw::HeaderBar {
        &self.header
    }

    /// Get the daemon indicator button for connecting click handlers.
    #[inline]
    pub fn daemon_indicator(&self) -> &Button {
        &self.daemon_indicator
    }

    /// Get the support button for connecting click handlers.
    #[inline]
    pub fn support_button(&self) -> &Button {
        &self.support_button
    }

    /// Update the daemon indicator visibility.
    ///
    /// # Arguments
    ///
    /// * `visible` - Whether the daemon is currently available
    pub fn set_daemon_available(&self, visible: bool) {
        self.daemon_indicator.set_visible(visible);
    }

    /// Update the window title dynamically.
    ///
    /// # Arguments
    ///
    /// * `title` - New title text
    pub fn set_title(&self, title: &str) {
        self.title_widget.set_title(title);
    }

    /// Update the window subtitle dynamically.
    ///
    /// # Arguments
    ///
    /// * `subtitle` - New subtitle text (empty string to hide)
    pub fn set_subtitle(&self, subtitle: &str) {
        self.title_widget.set_subtitle(subtitle);
    }
}

// ============================================================================
// TitlebarBuilder
// ============================================================================

/// Builder for constructing a [`Titlebar`] with custom configuration.
///
/// Uses the builder pattern for a fluent, readable API.
pub struct TitlebarBuilder<'a> {
    window: &'a adw::ApplicationWindow,
    title: String,
    subtitle: String,
    show_daemon_indicator: bool,
    daemon_indicator_tooltip: String,
}

impl<'a> TitlebarBuilder<'a> {
    /// Create a new builder with default settings.
    fn new(window: &'a adw::ApplicationWindow) -> Self {
        Self {
            window,
            title: DEFAULT_TITLE.to_string(),
            subtitle: String::new(),
            show_daemon_indicator: true,
            daemon_indicator_tooltip: TOOLTIP_DAEMON_DEFAULT.to_string(),
        }
    }

    /// Set the window title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Set the window subtitle (shown below title in smaller text).
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = subtitle.into();
        self
    }

    /// Whether to include the daemon indicator button.
    pub fn show_daemon_indicator(mut self, show: bool) -> Self {
        self.show_daemon_indicator = show;
        self
    }

    /// Set custom tooltip for the daemon indicator.
    pub fn daemon_indicator_tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.daemon_indicator_tooltip = tooltip.into();
        self
    }

    /// Build the titlebar with the configured options.
    pub fn build(self) -> Titlebar {
        // Create header bar with custom window controls
        let header = adw::HeaderBar::builder()
            .show_end_title_buttons(false)
            .show_start_title_buttons(false)
            .build();

        // Title widget
        let title_widget = adw::WindowTitle::new(&self.title, &self.subtitle);
        header.set_title_widget(Some(&title_widget));

        // Daemon status indicator (left side)
        let daemon_indicator = Button::builder()
            .icon_name(ICON_DAEMON)
            .css_classes(CLASSES_DAEMON_INDICATOR)
            .tooltip_text(&self.daemon_indicator_tooltip)
            .accessible_role(gtk4::AccessibleRole::Button)
            .visible(false) // Hidden until daemon is detected
            .build();

        if self.show_daemon_indicator {
            header.pack_start(&daemon_indicator);
        }

        // Support button (left side, after daemon indicator)
        let support_button = Button::builder()
            .icon_name(ICON_SUPPORT)
            .css_classes(CLASSES_SUPPORT_BUTTON)
            .tooltip_text(TOOLTIP_SUPPORT)
            .accessible_role(gtk4::AccessibleRole::Button)
            .build();
        header.pack_start(&support_button);

        // Window controls (right side)
        let controls = WindowControls::new(self.window);
        header.pack_end(controls.widget());

        Titlebar {
            header,
            daemon_indicator,
            support_button,
            title_widget,
        }
    }
}

// ============================================================================
// WindowControls
// ============================================================================

/// Custom window control buttons (minimize, maximize, close).
///
/// Provides compact 24px buttons with 4px spacing, replacing the default
/// 44px buttons with large gaps.
///
/// **Note**: 24px buttons are below WCAG 2.1 AAA (44x44px) but acceptable
/// for desktop window chrome where precision pointing is expected.
struct WindowControls {
    container: gtk4::Box,
    maximize_btn: Button,
}

impl WindowControls {
    /// Create window controls connected to the given window.
    fn new(window: &adw::ApplicationWindow) -> Self {
        let container = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(WINDOW_CONTROL_SPACING)
            .build();

        // Minimize button
        let minimize_btn = Self::create_control_button(
            ICON_MINIMIZE,
            "Minimize",
            CLASSES_WINDOW_CONTROL,
        );
        let win = window.clone();
        minimize_btn.connect_clicked(move |_| win.minimize());

        // Maximize button with icon toggle
        let maximize_btn = Self::create_control_button(
            ICON_MAXIMIZE,
            "Maximize",
            CLASSES_WINDOW_CONTROL,
        );
        
        // Update icon based on window state
        let btn_for_notify = maximize_btn.clone();
        window.connect_notify_local(Some("maximized"), move |win, _| {
            if win.is_maximized() {
                btn_for_notify.set_icon_name(ICON_RESTORE);
                btn_for_notify.set_tooltip_text(Some("Restore"));
            } else {
                btn_for_notify.set_icon_name(ICON_MAXIMIZE);
                btn_for_notify.set_tooltip_text(Some("Maximize"));
            }
        });
        
        let win = window.clone();
        maximize_btn.connect_clicked(move |_| {
            if win.is_maximized() {
                win.unmaximize();
            } else {
                win.maximize();
            }
        });

        // Close button (with destructive styling on hover)
        let close_btn = Self::create_control_button(
            ICON_CLOSE,
            "Close",
            CLASSES_CLOSE_BUTTON,
        );
        let win = window.clone();
        close_btn.connect_clicked(move |_| win.close());

        container.append(&minimize_btn);
        container.append(&maximize_btn);
        container.append(&close_btn);

        Self { container, maximize_btn }
    }

    /// Get the container widget.
    fn widget(&self) -> &gtk4::Box {
        &self.container
    }

    /// Create a single window control button.
    fn create_control_button(icon_name: &str, tooltip: &str, css_classes: &[&str]) -> Button {
        Button::builder()
            .icon_name(icon_name)
            .css_classes(css_classes.to_vec())
            .tooltip_text(tooltip)
            .accessible_role(gtk4::AccessibleRole::Button)
            .build()
    }
}

