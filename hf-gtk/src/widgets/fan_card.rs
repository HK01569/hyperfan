//! Fan control card widget
//!
//! Displays fan RPM, PWM value, and provides a slider for manual speed control.

#![allow(dead_code)]

use gtk4::prelude::*;
use gtk4::{Adjustment, Scale};
use libadwaita as adw;
use libadwaita::prelude::*;

use hf_core::{FanSensor, PwmController};
use hf_core::daemon_client;

/// Fan speed slider configuration
mod slider {
    /// Minimum fan speed percentage
    pub const MIN_PERCENT: f64 = 0.0;
    /// Maximum fan speed percentage
    pub const MAX_PERCENT: f64 = 100.0;
    /// Small step (arrow keys)
    pub const STEP_INCREMENT: f64 = 1.0;
    /// Large step (page up/down)
    pub const PAGE_INCREMENT: f64 = 10.0;
}

/// Default PWM values when sensor data unavailable
mod defaults {
    /// Default PWM value (50% = 128)
    pub const PWM_VALUE: u8 = 128;
    /// Default percentage
    pub const PERCENT: f32 = 50.0;
}

/// Fan card widget for controlling a single fan
pub struct FanCard {
    group: adw::PreferencesGroup,
    fan_path: Option<String>,
    rpm_label: Option<gtk4::Label>,
}

impl FanCard {
    pub fn new(chip_name: &str, pwm: &PwmController, fan: Option<&FanSensor>) -> Self {
        let title = fan
            .and_then(|f| f.label.as_ref())
            .map(|l| l.to_string())
            .unwrap_or_else(|| format!("{}/{}", chip_name, pwm.name));

        let group = adw::PreferencesGroup::builder().title(&title).build();

        // RPM display row
        let (fan_path, rpm_label) = if let Some(fan) = fan {
            let rpm_str = fan
                .current_rpm
                .map(|r| format!("{} RPM", r))
                .unwrap_or_else(|| "N/A".to_string());

            let rpm_label = gtk4::Label::builder()
                .label(&rpm_str)
                .css_classes(["title-3", "numeric"])
                .build();

            let rpm_row = adw::ActionRow::builder()
                .title("Current Speed")
                .build();
            
            rpm_row.add_suffix(&rpm_label);
            group.add(&rpm_row);
            
            (Some(fan.input_path.to_string_lossy().to_string()), Some(rpm_label))
        } else {
            (None, None)
        };

        // PWM control row with slider
        let pwm_value = pwm.current_value.unwrap_or(defaults::PWM_VALUE);
        let pwm_percent = pwm.current_percent.unwrap_or(defaults::PERCENT);

        let adjustment = Adjustment::new(
            pwm_percent as f64,
            slider::MIN_PERCENT,
            slider::MAX_PERCENT,
            slider::STEP_INCREMENT,
            slider::PAGE_INCREMENT,
            0.0,  // page_size (unused for sliders)
        );

        let scale = Scale::builder()
            .adjustment(&adjustment)
            .hexpand(true)
            .draw_value(true)
            .value_pos(gtk4::PositionType::Right)
            .build();

        // Store PWM path for the callback
        let pwm_path = pwm.pwm_path.clone();
        let pwm_path_for_release = pwm.pwm_path.clone();

        adjustment.connect_value_changed(move |adj| {
            let speed_percent = adj.value() as f32;
            let pwm_value = hf_core::constants::pwm::from_percent(speed_percent);

            // Daemon authoritative: use short-lived override for live preview.
            // The daemon loop will respect this override and skip curve control temporarily.
            if let Err(e) = daemon_client::daemon_set_pwm_override(
                pwm_path.to_string_lossy().as_ref(),
                pwm_value,
                1500,
            ) {
                tracing::error!("Failed to set PWM override via daemon to {} ({}%): {}", pwm_value, speed_percent, e);
            }
        });

        // Clear override when user releases the slider so daemon can resume curve control
        let gesture = gtk4::GestureClick::new();
        gesture.set_button(0); // All buttons
        gesture.connect_released(move |_, _, _, _| {
            // Clear the PWM override so daemon resumes curve-based control
            if let Err(e) = daemon_client::daemon_clear_pwm_override(pwm_path_for_release.to_string_lossy().as_ref()) {
                tracing::warn!("Failed to clear PWM override: {}", e);
            }
        });
        scale.add_controller(gesture);

        let pwm_row = adw::ActionRow::builder()
            .title("Fan Speed")
            .subtitle(&hf_core::display::format_pwm_subtitle(pwm_value, pwm_percent))
            .build();

        pwm_row.add_suffix(&scale);
        group.add(&pwm_row);

        // Mode indicator - daemon authoritative; avoid direct sysfs reads in GUI.
        // We conservatively show "Daemon" to reflect that the daemon owns control.
        let control_mode = "Daemon";

        let mode_row = adw::ActionRow::builder()
            .title("Control Mode")
            .subtitle(control_mode)
            .build();
        group.add(&mode_row);

        Self { 
            group,
            fan_path,
            rpm_label,
        }
    }

    pub fn widget(&self) -> &adw::PreferencesGroup {
        &self.group
    }

    /// Update fan RPM display from cached sensor data
    pub fn update(&self) {
        if let (Some(fan_path), Some(rpm_label)) = (&self.fan_path, &self.rpm_label) {
            // Try to get cached sensor data from runtime (non-blocking)
            let cached_data = crate::runtime::get_sensors();
            
            let rpm = cached_data.as_ref()
                .and_then(|data| {
                    data.fans.iter()
                        .find(|f| &f.path == fan_path)
                        .and_then(|f| f.rpm)
                });
            
            let new_text = rpm
                .map(|r| format!("{} RPM", r))
                .unwrap_or_else(|| "N/A".to_string());
            
            // Only update if the text changed to avoid unnecessary redraws
            if rpm_label.text() != new_text {
                rpm_label.set_label(&new_text);
            }
        }
    }
}
