//! GPU Info Card Widget (Sensor-only)
//!
//! Displays GPU information including temperature, fan speed, and power usage.
//! This is a READ-ONLY card for sensor display - no fan control.

#![allow(dead_code)]

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, Orientation, ProgressBar};
use libadwaita as adw;
use libadwaita::prelude::*;

use hf_core::GpuDevice;

// ============================================================================
// Constants
// ============================================================================

mod layout {
    pub const CARD_SPACING: i32 = 12;
    pub const CARD_MARGIN: i32 = 16;
    pub const STATS_SPACING: i32 = 24;
    pub const LABEL_SPACING: i32 = 4;
}

const PERCENT_MULTIPLIER: f64 = 100.0;

/// GPU info card widget (read-only sensor display)
pub struct GpuInfoCard {
    card: adw::Bin,
    gpu_index: u32,
    temp_label: Label,
    fan_label: Label,
    power_label: Label,
    vram_bar: ProgressBar,
    util_bar: ProgressBar,
}

impl GpuInfoCard {
    /// Create a new GPU info card widget for the given GPU device
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

        // Fan Speed (read-only display)
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

        let vram_bar = ProgressBar::builder().show_text(true).build();

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

        let util_bar = ProgressBar::builder().show_text(true).build();

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

        // NO FAN CONTROLS - this is sensor-only

        card.set_child(Some(&content));

        Self {
            card,
            gpu_index: gpu.index,
            temp_label,
            fan_label,
            power_label,
            vram_bar,
            util_bar,
        }
    }

    /// Update the card with fresh GPU data
    /// PERFORMANCE: Only update widgets if values actually changed
    pub fn update(&self, gpu: &GpuDevice) {
        // Update temperature (only if changed)
        let temp_value = gpu
            .temperatures
            .first()
            .and_then(|t| t.current_temp)
            .map(|t| hf_core::display::format_temp(t))
            .unwrap_or_else(|| "N/A".to_string());
        if self.temp_label.text() != temp_value {
            self.temp_label.set_label(&temp_value);
        }

        // Update fan speed (only if changed)
        let fan_value = gpu
            .fans
            .first()
            .and_then(|f| f.speed_percent)
            .map(|s| hf_core::display::format_fan_speed(s))
            .unwrap_or_else(|| "N/A".to_string());
        if self.fan_label.text() != fan_value {
            self.fan_label.set_label(&fan_value);
        }

        // Update power (only if changed)
        let power_value = gpu
            .power_watts
            .map(|p| format!("{:.0}W", p))
            .unwrap_or_else(|| "N/A".to_string());
        if self.power_label.text() != power_value {
            self.power_label.set_label(&power_value);
        }

        // Update VRAM bar (only if changed)
        if let (Some(used), Some(total)) = (gpu.vram_used_mb, gpu.vram_total_mb) {
            let fraction = used as f64 / total as f64;
            if (self.vram_bar.fraction() - fraction).abs() > 0.001 {
                self.vram_bar.set_fraction(fraction);
                self.vram_bar.set_text(Some(&format!(
                    "{} / {} MB ({:.0}%)",
                    used,
                    total,
                    fraction * PERCENT_MULTIPLIER
                )));
            }
        }

        // Update utilization bar (only if changed)
        if let Some(util) = gpu.utilization_percent {
            let fraction = util as f64 / PERCENT_MULTIPLIER;
            if (self.util_bar.fraction() - fraction).abs() > 0.001 {
                self.util_bar.set_fraction(fraction);
                self.util_bar.set_text(Some(&format!("{}%", util)));
            }
        }
    }

    /// Update from cached runtime GpuReading (avoids blocking I/O)
    pub fn update_from_reading(&self, reading: &crate::runtime::GpuReading) {
        // Update temperature (only if changed)
        let temp_value = reading.temp
            .map(|t| hf_core::display::format_temp(t))
            .unwrap_or_else(|| "N/A".to_string());
        if self.temp_label.text() != temp_value {
            self.temp_label.set_label(&temp_value);
        }

        // Update fan speed
        let fan_value = reading.fan_percent
            .map(|s| hf_core::display::format_fan_speed(s))
            .unwrap_or_else(|| "N/A".to_string());
        if self.fan_label.text() != fan_value {
            self.fan_label.set_label(&fan_value);
        }

        // Update power
        let power_value = reading.power_watts
            .map(|p| format!("{:.0}W", p))
            .unwrap_or_else(|| "N/A".to_string());
        if self.power_label.text() != power_value {
            self.power_label.set_label(&power_value);
        }

        // Update VRAM bar (only if changed)
        if let (Some(used), Some(total)) = (reading.vram_used_mb, reading.vram_total_mb) {
            let fraction = used as f64 / total as f64;
            if (self.vram_bar.fraction() - fraction).abs() > 0.001 {
                self.vram_bar.set_fraction(fraction);
                self.vram_bar.set_text(Some(&format!(
                    "{} / {} MB ({:.0}%)",
                    used, total, fraction * PERCENT_MULTIPLIER
                )));
            }
        }

        // Update utilization bar
        if let Some(util) = reading.utilization {
            let fraction = util as f64 / PERCENT_MULTIPLIER;
            if (self.util_bar.fraction() - fraction).abs() > 0.001 {
                self.util_bar.set_fraction(fraction);
                self.util_bar.set_text(Some(&format!("{}%", util)));
            }
        }
    }

    pub fn gpu_index(&self) -> u32 {
        self.gpu_index
    }

    pub fn widget(&self) -> &adw::Bin {
        &self.card
    }
}
