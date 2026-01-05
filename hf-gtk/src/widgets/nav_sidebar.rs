//! Navigation Sidebar
//!
//! Vertical navigation bar with icon buttons for switching between pages.
//! Uses standard GTK symbolic icons for consistency with the desktop theme.

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, Orientation};
use std::cell::RefCell;
use std::rc::Rc;

// ============================================================================
// Types
// ============================================================================

/// Available navigation pages in the application
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavPage {
    Dashboard,
    Curves,
    FanPairing,
    Sensors,
    Graphs,
    EcControl,
}

impl NavPage {
    /// Get the GTK symbolic icon name for this page
    fn icon_name(&self) -> &'static str {
        match self {
            NavPage::Dashboard => "go-home-symbolic",
            NavPage::Curves => "document-edit-symbolic",
            NavPage::FanPairing => "fan-symbolic",
            NavPage::Sensors => "dialog-information-symbolic",
            NavPage::Graphs => "utilities-system-monitor-symbolic",
            NavPage::EcControl => "utilities-terminal-symbolic", // EC register access
        }
    }

    /// Get the tooltip text for this page
    fn tooltip(&self) -> &'static str {
        match self {
            NavPage::Dashboard => "Dashboard",
            NavPage::Curves => "Fan Curves",
            NavPage::FanPairing => "Fan Pairing",
            NavPage::Sensors => "Temperature Sensors",
            NavPage::Graphs => "Temperature Graphs",
            NavPage::EcControl => "EC Direct Control (DANGEROUS)",
        }
    }
}

// ============================================================================
// Navigation Sidebar Widget
// ============================================================================

/// Navigation sidebar with icon buttons for page switching
pub struct NavSidebar {
    container: GtkBox,
    on_navigate: Rc<RefCell<Option<Box<dyn Fn(NavPage)>>>>,
    buttons: Vec<(NavPage, Button)>,
}

impl NavSidebar {
    /// Create a new navigation sidebar
    pub fn new() -> Self {
        let container = Self::build_container();
        let on_navigate: Rc<RefCell<Option<Box<dyn Fn(NavPage)>>>> = Rc::new(RefCell::new(None));
        let mut buttons = Vec::new();

        // Create buttons for each page
        let pages = [NavPage::Dashboard, NavPage::Sensors, NavPage::Graphs];
        
        for page in pages {
            let button = Self::build_nav_button(page);
            container.append(&button);
            
            // Connect click handler
            let callback = on_navigate.clone();
            button.connect_clicked(move |_| {
                if let Some(cb) = callback.borrow().as_ref() {
                    cb(page);
                }
            });
            
            buttons.push((page, button));
        }

        // Mark Dashboard as initially active
        if let Some((_, btn)) = buttons.first() {
            btn.add_css_class("suggested-action");
        }

        Self { container, on_navigate, buttons }
    }

    /// Build the sidebar container
    fn build_container() -> GtkBox {
        GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(8)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(8)
            .margin_end(8)
            .css_classes(["nav-sidebar"])
            .build()
    }

    /// Build a navigation button for a specific page
    fn build_nav_button(page: NavPage) -> Button {
        Button::builder()
            .icon_name(page.icon_name())
            .width_request(50)
            .height_request(50)
            .tooltip_text(page.tooltip())
            .css_classes(["flat", "circular", "nav-button"])
            .build()
    }

    /// Connect a callback for when navigation occurs
    pub fn connect_navigate<F: Fn(NavPage) + 'static>(&self, callback: F) {
        *self.on_navigate.borrow_mut() = Some(Box::new(callback));
    }

    /// Update the visual indicator for the active page
    pub fn set_active(&self, active_page: NavPage) {
        for (page, button) in &self.buttons {
            if *page == active_page {
                button.add_css_class("suggested-action");
            } else {
                button.remove_css_class("suggested-action");
            }
        }
    }

    /// Get the underlying GTK widget
    pub fn widget(&self) -> &GtkBox {
        &self.container
    }
}

impl Default for NavSidebar {
    fn default() -> Self {
        Self::new()
    }
}
