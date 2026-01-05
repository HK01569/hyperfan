//! PWM-Fan Detection Dialog
//!
//! Shows progress during first-run PWM-to-fan mapping detection.
//! Runs detection in background thread to avoid blocking UI.

#![allow(dead_code)]

use gtk4::prelude::*;
use gtk4::{Label, Orientation, ProgressBar};
use gtk4::Box as GtkBox;
use gtk4::glib;
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use hf_core::FanMapping;

/// Detection dialog state
#[derive(Clone, Copy, PartialEq)]
enum DetectionState {
    Ready,
    Running,
    Complete,
    Error,
}

/// PWM-Fan Detection Dialog
pub struct DetectionDialog {
    dialog: adw::Window,
    on_complete: Rc<RefCell<Option<Box<dyn Fn(Vec<FanMapping>)>>>>,
}

impl DetectionDialog {
    pub fn new() -> Rc<Self> {
        let dialog = adw::Window::builder()
            .title("Fan Detection")
            .default_width(450)
            .default_height(350)
            .modal(true)
            .build();

        let on_complete: Rc<RefCell<Option<Box<dyn Fn(Vec<FanMapping>)>>>> = 
            Rc::new(RefCell::new(None));

        let content = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(24)
            .margin_start(32)
            .margin_end(32)
            .margin_top(32)
            .margin_bottom(32)
            .valign(gtk4::Align::Center)
            .build();

        // Icon
        let icon = gtk4::Image::builder()
            .icon_name("preferences-system-symbolic")
            .pixel_size(64)
            .css_classes(["dim-label"])
            .build();
        content.append(&icon);

        // Title
        let title = Label::builder()
            .label("Detecting Fan Controllers")
            .css_classes(["title-1"])
            .build();
        content.append(&title);

        // Description
        let desc = Label::builder()
            .label("Hyperfan will test each PWM controller to identify which fans they control.\n\nThis process will:\n• Set all fans to 100% speed\n• Wait 3 seconds for stabilization\n• Test each controller individually\n\nYour fans may speed up and slow down during this process.")
            .wrap(true)
            .justify(gtk4::Justification::Center)
            .css_classes(["dim-label"])
            .build();
        content.append(&desc);

        // Progress bar (hidden initially)
        let progress = ProgressBar::builder()
            .show_text(true)
            .visible(false)
            .margin_top(12)
            .build();
        content.append(&progress);

        // Status label
        let status = Label::builder()
            .label("")
            .css_classes(["caption"])
            .visible(false)
            .build();
        content.append(&status);

        // Results container (shown after detection)
        let results_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(8)
            .visible(false)
            .build();
        content.append(&results_box);

        // Buttons
        let button_box = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .halign(gtk4::Align::Center)
            .margin_top(12)
            .build();

        let start_btn = gtk4::Button::builder()
            .label("Start Detection")
            .css_classes(["suggested-action", "pill"])
            .build();

        let skip_btn = gtk4::Button::builder()
            .label("Skip")
            .css_classes(["pill"])
            .build();

        let cancel_btn = gtk4::Button::builder()
            .label("Cancel")
            .css_classes(["pill"])
            .visible(false)
            .build();
        
        let close_btn = gtk4::Button::builder()
            .label("Done")
            .css_classes(["suggested-action", "pill"])
            .visible(false)
            .build();

        button_box.append(&skip_btn);
        button_box.append(&cancel_btn);
        button_box.append(&start_btn);
        button_box.append(&close_btn);
        content.append(&button_box);

        dialog.set_content(Some(&content));

        let this = Rc::new(Self { dialog, on_complete });

        // Skip button - just close
        let dialog_for_skip = this.dialog.clone();
        skip_btn.connect_clicked(move |_| {
            dialog_for_skip.close();
        });

        // Close button - close after completion
        let this_for_close = this.clone();
        close_btn.connect_clicked(move |_| {
            this_for_close.dialog.close();
        });

        // Cancel button - abort detection
        let dialog_for_cancel = this.dialog.clone();
        let cancelled = Rc::new(RefCell::new(false));
        let cancelled_for_cancel = cancelled.clone();
        cancel_btn.connect_clicked(move |_| {
            *cancelled_for_cancel.borrow_mut() = true;
            dialog_for_cancel.close();
        });
        
        // Start detection button
        let progress_for_start = progress.clone();
        let status_for_start = status.clone();
        let start_btn_for_start = start_btn.clone();
        let skip_btn_for_start = skip_btn.clone();
        let cancel_btn_for_start = cancel_btn.clone();
        let close_btn_for_start = close_btn.clone();
        let results_box_for_start = results_box.clone();
        let title_for_start = title.clone();
        let desc_for_start = desc.clone();
        let this_for_start = this.clone();
        let cancelled_for_start = cancelled.clone();

        start_btn.connect_clicked(move |_| {
            // Reset cancelled flag
            *cancelled_for_start.borrow_mut() = false;
            
            // Update UI for running state
            start_btn_for_start.set_visible(false);
            skip_btn_for_start.set_visible(false);
            cancel_btn_for_start.set_visible(true);
            progress_for_start.set_visible(true);
            status_for_start.set_visible(true);
            desc_for_start.set_visible(false);
            title_for_start.set_label("Detection in Progress...");
            status_for_start.set_label("Setting all fans to 100%...");
            progress_for_start.set_fraction(0.0);

            // Channel for thread communication
            let (tx, rx) = mpsc::channel::<DetectionUpdate>();
            let rx = Rc::new(RefCell::new(Some(rx)));

            // Spawn detection thread
            thread::spawn(move || {
                run_detection_blocking(tx);
            });

            // Poll for updates from detection thread
            let progress_for_rx = progress_for_start.clone();
            let status_for_rx = status_for_start.clone();
            let close_btn_for_rx = close_btn_for_start.clone();
            let results_box_for_rx = results_box_for_start.clone();
            let title_for_rx = title_for_start.clone();
            let this_for_rx = this_for_start.clone();

            let cancelled_for_rx = cancelled_for_start.clone();
            let cancel_btn_for_rx = cancel_btn_for_start.clone();
            glib::timeout_add_local(Duration::from_millis(50), move || {
                // Check if user cancelled
                if *cancelled_for_rx.borrow() {
                    cancel_btn_for_rx.set_visible(false);
                    return glib::ControlFlow::Break;
                }
                
                let rx_opt = rx.borrow_mut();
                let rx_ref = match rx_opt.as_ref() {
                    Some(r) => r,
                    None => return glib::ControlFlow::Break,
                };

                // Process all available updates
                while let Ok(update) = rx_ref.try_recv() {
                    match update {
                        DetectionUpdate::Progress { fraction, message } => {
                            progress_for_rx.set_fraction(fraction);
                            status_for_rx.set_label(&message);
                        }
                        DetectionUpdate::Complete { mappings } => {
                            title_for_rx.set_label("Detection Complete");
                            progress_for_rx.set_visible(false);
                            status_for_rx.set_visible(false);
                            cancel_btn_for_rx.set_visible(false);
                            close_btn_for_rx.set_visible(true);

                            // Show results
                            results_box_for_rx.set_visible(true);
                            
                            // Clear previous results
                            while let Some(child) = results_box_for_rx.first_child() {
                                results_box_for_rx.remove(&child);
                            }

                            if mappings.is_empty() {
                                let no_results = Label::builder()
                                    .label("No PWM-fan mappings detected.\nYou may need to check BIOS settings.")
                                    .wrap(true)
                                    .justify(gtk4::Justification::Center)
                                    .css_classes(["dim-label"])
                                    .build();
                                results_box_for_rx.append(&no_results);
                            } else {
                                let summary = Label::builder()
                                    .label(&format!("Found {} PWM-fan mapping(s):", mappings.len()))
                                    .css_classes(["heading"])
                                    .build();
                                results_box_for_rx.append(&summary);

                                for mapping in &mappings {
                                    let row = Label::builder()
                                        .label(&format!("• {} → {} ({:.0}% confidence)", 
                                            mapping.pwm_name, 
                                            mapping.fan_name,
                                            mapping.confidence * 100.0))
                                        .halign(gtk4::Align::Start)
                                        .css_classes(["caption"])
                                        .build();
                                    results_box_for_rx.append(&row);
                                }
                            }

                            // Save results
                            if let Err(e) = hf_core::save_pwm_fan_mappings(mappings.clone()) {
                                tracing::error!("Failed to save PWM-fan mappings: {}", e);
                            } else {
                                // Signal daemon to reload config
                                if let Err(e) = hf_core::daemon_reload_config() {
                                    tracing::debug!("Failed to signal daemon reload: {}", e);
                                }
                            }

                            // Call completion callback
                            if let Some(callback) = this_for_rx.on_complete.borrow().as_ref() {
                                callback(mappings);
                            }

                            return glib::ControlFlow::Break;
                        }
                        DetectionUpdate::Error { message } => {
                            title_for_rx.set_label("Detection Failed");
                            progress_for_rx.set_visible(false);
                            status_for_rx.set_label(&message);
                            cancel_btn_for_rx.set_visible(false);
                            close_btn_for_rx.set_visible(true);
                            return glib::ControlFlow::Break;
                        }
                    }
                }
                glib::ControlFlow::Continue
            });
        });

        this
    }

    pub fn connect_complete<F: Fn(Vec<FanMapping>) + 'static>(&self, callback: F) {
        *self.on_complete.borrow_mut() = Some(Box::new(callback));
    }

    pub fn present(&self) {
        self.dialog.present();
    }

    pub fn set_transient_for(&self, parent: &impl gtk4::prelude::IsA<gtk4::Window>) {
        self.dialog.set_transient_for(Some(parent));
    }
}

/// Updates sent from detection thread to UI
enum DetectionUpdate {
    Progress { fraction: f64, message: String },
    Complete { mappings: Vec<FanMapping> },
    Error { message: String },
}

/// Run detection in blocking thread and send updates via channel
fn run_detection_blocking(tx: mpsc::Sender<DetectionUpdate>) {
    // Daemon authoritative: request mapping detection via daemon IPC.
    let _ = tx.send(DetectionUpdate::Progress {
        fraction: 0.1,
        message: "Requesting detection from daemon...".to_string(),
    });

    let daemon_mappings = match hf_core::daemon_detect_fan_mappings() {
        Ok(m) => m,
        Err(e) => {
            let _ = tx.send(DetectionUpdate::Error { message: e });
            return;
        }
    };

    // Map daemon result (paths) into a UI-friendly list.
    // We keep these as display-only; the daemon persists mappings.
    let mappings: Vec<FanMapping> = daemon_mappings
        .into_iter()
        .map(|m| FanMapping {
            fan_name: m.fan_path,
            pwm_name: m.pwm_path,
            confidence: m.confidence,
            temp_sources: Vec::new(),
            response_time_ms: None,
            min_pwm: None,
            max_rpm: None,
        })
        .collect();

    let _ = tx.send(DetectionUpdate::Complete { mappings });
}
