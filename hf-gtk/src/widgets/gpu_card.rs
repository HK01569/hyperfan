//! GPU Card Widget
//!
//! Displays GPU information including temperature, fan speed, and power usage.
//! Supports fan control for NVIDIA and AMD GPUs.
//!
//! # Features
//!
//! - Real-time temperature, fan speed, and power monitoring
//! - VRAM usage and GPU utilization bars
//! - Manual fan speed control slider
//! - Auto/Manual mode toggle

#![allow(dead_code)]

use gtk4::prelude::*;
use gtk4::{Adjustment, Box as GtkBox, Label, Orientation, ProgressBar, Scale};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::path::PathBuf;

use hf_core::{GpuDevice, GpuVendor};

// ============================================================================
// Constants
// ============================================================================

/// UI layout constants
mod layout {
    /// Spacing between elements in the card
    pub const CARD_SPACING: i32 = 12;
    /// Margin around the card content
    pub const CARD_MARGIN: i32 = 16;
    /// Spacing between stat boxes
    pub const STATS_SPACING: i32 = 24;
    /// Small spacing for label/value pairs
    pub const LABEL_SPACING: i32 = 4;
}

/// Fan control constants
mod fan_control {
    /// Minimum fan speed percentage
    pub const MIN_SPEED: f64 = 0.0;
    /// Maximum fan speed percentage
    pub const MAX_SPEED: f64 = 100.0;
    /// Step size for keyboard arrow keys
    pub const STEP_INCREMENT: f64 = 1.0;
    /// Step size for page up/down
    pub const PAGE_INCREMENT: f64 = 10.0;
    /// Default fan speed when no data available
    pub const DEFAULT_SPEED: f64 = 50.0;
}

/// Percentage multiplier for display
const PERCENT_MULTIPLIER: f64 = 100.0;

/// GPU card widget displaying GPU info and fan controls
pub struct GpuCard {
    card: adw::Bin,
    gpu_index: u32,
    vendor: GpuVendor,
    hwmon_path: Option<PathBuf>,
    temp_label: Label,
    fan_label: Label,
    power_label: Label,
    vram_bar: ProgressBar,
    util_bar: ProgressBar,
}

impl GpuCard {
    /// Create a new GPU card widget for the given GPU device
    pub fn new(gpu: &GpuDevice) -> Self {
        let card = adw::Bin::builder().css_classes(["card"]).build();

        let content = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(layout::CARD_SPACING)
            .margin_start(layout::CARD_MARGIN)
            .margin_end(layout::CARD_MARGIN)
            .margin_top(layout::CARD_MARGIN)
            .margin_bottom(layout::CARD_MARGIN)
            .build();

        // Header with GPU name and vendor badge
        let header = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(layout::CARD_SPACING)
            .build();

        let name_label = Label::builder()
            .label(&gpu.name)
            .css_classes(["title-3"])
            .hexpand(true)
            .halign(gtk4::Align::Start)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .build();

        let vendor_badge = Label::builder()
            .label(&gpu.vendor.to_string())
            .css_classes(["caption", "dim-label"])
            .build();

        header.append(&name_label);
        header.append(&vendor_badge);
        content.append(&header);

        // Stats row: Temperature, Fan Speed, Power
        let stats_box = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(layout::STATS_SPACING)
            .homogeneous(true)
            .build();

        // Temperature
        let temp_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(layout::LABEL_SPACING)
            .build();

        let temp_title = Label::builder()
            .label("Temperature")
            .css_classes(["caption", "dim-label"])
            .build();

        let temp_value = gpu
            .temperatures
            .first()
            .and_then(|t| t.current_temp)
            .map(|t| hf_core::display::format_temp(t))
            .unwrap_or_else(|| "N/A".to_string());

        let temp_label = Label::builder()
            .label(&temp_value)
            .css_classes(["title-2", "numeric"])
            .build();

        temp_box.append(&temp_title);
        temp_box.append(&temp_label);
        stats_box.append(&temp_box);

        // Fan Speed
        let fan_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(layout::LABEL_SPACING)
            .build();

        let fan_title = Label::builder()
            .label("Fan Speed")
            .css_classes(["caption", "dim-label"])
            .build();

        let fan_value = gpu
            .fans
            .first()
            .and_then(|f| f.speed_percent)
            .map(|s| hf_core::display::format_fan_speed(s))
            .unwrap_or_else(|| "N/A".to_string());

        let fan_label = Label::builder()
            .label(&fan_value)
            .css_classes(["title-2", "numeric"])
            .build();

        fan_box.append(&fan_title);
        fan_box.append(&fan_label);
        stats_box.append(&fan_box);

        // Power
        let power_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(layout::LABEL_SPACING)
            .build();

        let power_title = Label::builder()
            .label("Power")
            .css_classes(["caption", "dim-label"])
            .build();

        let power_value = gpu
            .power_watts
            .map(|p| format!("{:.0}W", p))
            .unwrap_or_else(|| "N/A".to_string());

        let power_label = Label::builder()
            .label(&power_value)
            .css_classes(["title-2", "numeric"])
            .build();

        power_box.append(&power_title);
        power_box.append(&power_label);
        stats_box.append(&power_box);

        content.append(&stats_box);

        // VRAM usage bar
        let vram_group = adw::PreferencesGroup::builder().title("VRAM Usage").build();

        let vram_bar = ProgressBar::builder()
            .show_text(true)
            .build();

        if let (Some(vram_used), Some(vram_total)) = (gpu.vram_used_mb, gpu.vram_total_mb) {
            let usage_fraction = vram_used as f64 / vram_total as f64;
            vram_bar.set_fraction(usage_fraction);
            vram_bar.set_text(Some(&format!(
                "{} / {} MB ({:.0}%)",
                vram_used,
                vram_total,
                usage_fraction * PERCENT_MULTIPLIER
            )));
        } else {
            vram_bar.set_text(Some("N/A"));
        }

        let vram_row = adw::ActionRow::builder().build();
        vram_row.add_suffix(&vram_bar);
        vram_group.add(&vram_row);
        content.append(&vram_group);

        // GPU Utilization bar
        let util_group = adw::PreferencesGroup::builder()
            .title("GPU Utilization")
            .build();

        let util_bar = ProgressBar::builder()
            .show_text(true)
            .build();

        if let Some(utilization) = gpu.utilization_percent {
            util_bar.set_fraction(utilization as f64 / PERCENT_MULTIPLIER);
            util_bar.set_text(Some(&format!("{}%", utilization)));
        } else {
            util_bar.set_text(Some("N/A"));
        }

        let util_row = adw::ActionRow::builder().build();
        util_row.add_suffix(&util_bar);
        util_group.add(&util_row);
        content.append(&util_group);

        // Fan control (if fans are available)
        if !gpu.fans.is_empty() {
            let fan_group = adw::PreferencesGroup::builder()
                .title("Fan Control")
                .description("Adjust GPU fan speed manually")
                .build();

            for fan in &gpu.fans {
                let current_speed = fan.speed_percent.unwrap_or(50) as f64;

                let adjustment = Adjustment::new(current_speed, 0.0, 100.0, 1.0, 10.0, 0.0);

                let scale = Scale::builder()
                    .adjustment(&adjustment)
                    .hexpand(true)
                    .draw_value(true)
                    .value_pos(gtk4::PositionType::Right)
                    .build();

                // Store GPU info for the callback
                let gpu_index = gpu.index;
                let fan_index = fan.index;
                let vendor = gpu.vendor;

                adjustment.connect_value_changed(move |adj| {
                    let percent = adj.value() as u32;

                    match vendor {
                        GpuVendor::Nvidia | GpuVendor::Amd | GpuVendor::Intel => {
                            if let Err(e) = hf_core::daemon_set_gpu_fan_for_fan(gpu_index, fan_index, percent) {
                                tracing::error!("Failed to set GPU fan speed via daemon: {}", e);
                            }
                        }
                    }
                });

                let fan_row = adw::ActionRow::builder()
                    .title(&fan.name)
                    .subtitle(&format!(
                        "{}",
                        fan.rpm
                            .map(|r| format!("{} RPM", r))
                            .unwrap_or_else(|| "RPM N/A".to_string())
                    ))
                    .build();

                fan_row.add_suffix(&scale);
                fan_group.add(&fan_row);
            }

            // Auto/Manual toggle
            let mode_row = adw::SwitchRow::builder()
                .title("Manual Control")
                .subtitle("Override automatic fan curve")
                .build();

            let gpu_index = gpu.index;
            let vendor = gpu.vendor;

            mode_row.connect_active_notify(move |row| {
                if !row.is_active() {
                    // Reset to auto
                    if let Err(e) = hf_core::daemon_reset_gpu_fan_auto(gpu_index) {
                        tracing::error!("Failed to reset GPU fan auto via daemon: {}", e);
                    }
                }
            });

            fan_group.add(&mode_row);
            content.append(&fan_group);
        }

        card.set_child(Some(&content));

        Self {
            card,
            gpu_index: gpu.index,
            vendor: gpu.vendor,
            hwmon_path: None,
            temp_label,
            fan_label,
            power_label,
            vram_bar,
            util_bar,
        }
    }

    /// Update the card with fresh GPU data
    pub fn update(&self, gpu: &GpuDevice) {
        // Update temperature
        let temp_value = gpu
            .temperatures
            .first()
            .and_then(|t| t.current_temp)
            .map(|t| hf_core::display::format_temp(t))
            .unwrap_or_else(|| "N/A".to_string());
        self.temp_label.set_label(&temp_value);

        // Update fan speed
        let fan_value = gpu
            .fans
            .first()
            .and_then(|f| f.speed_percent)
            .map(|s| hf_core::display::format_fan_speed(s))
            .unwrap_or_else(|| "N/A".to_string());
        self.fan_label.set_label(&fan_value);

        // Update power
        let power_value = gpu
            .power_watts
            .map(|p| format!("{:.0}W", p))
            .unwrap_or_else(|| "N/A".to_string());
        self.power_label.set_label(&power_value);

        // Update VRAM bar
        if let (Some(used), Some(total)) = (gpu.vram_used_mb, gpu.vram_total_mb) {
            let fraction = used as f64 / total as f64;
            self.vram_bar.set_fraction(fraction);
            self.vram_bar.set_text(Some(&format!(
                "{} / {} MB ({:.0}%)",
                used,
                total,
                fraction * 100.0
            )));
        }

        // Update utilization bar
        if let Some(util) = gpu.utilization_percent {
            self.util_bar.set_fraction(util as f64 / 100.0);
            self.util_bar.set_text(Some(&format!("{}%", util)));
        }
    }

    pub fn gpu_index(&self) -> u32 {
        self.gpu_index
    }

    pub fn widget(&self) -> &adw::Bin {
        &self.card
    }
}
