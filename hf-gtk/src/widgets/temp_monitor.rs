//! Real-time temperature monitoring widget

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, LevelBar, Orientation};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::path::PathBuf;

use hf_core::TemperatureSensor;

use hf_core::daemon_client;

/// A temperature sensor display widget
pub struct TempMonitor {
    row: adw::ActionRow,
    level_bar: LevelBar,
    value_label: Label,
    sensor_path: PathBuf,
}

impl TempMonitor {
    pub fn new(chip_name: &str, sensor: &TemperatureSensor) -> Self {
        let title = sensor
            .label
            .as_ref()
            .cloned()
            .unwrap_or_else(|| format!("{}/{}", chip_name, sensor.name));

        let temp = sensor.current_temp.unwrap_or(0.0);

        let value_label = Label::builder()
            .label(&hf_core::display::format_temp_precise(temp))
            .css_classes(["numeric"])
            .build();

        let level_bar = LevelBar::builder()
            .min_value(0.0)
            .max_value(100.0)
            .value(temp.clamp(0.0, 100.0) as f64)
            .hexpand(true)
            .valign(gtk4::Align::Center)
            .build();

        // Set color ranges
        level_bar.add_offset_value("low", 45.0);
        level_bar.add_offset_value("high", 70.0);
        level_bar.add_offset_value("full", 85.0);

        let content_box = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .build();

        content_box.append(&level_bar);
        content_box.append(&value_label);

        let row = adw::ActionRow::builder()
            .title(&title)
            .build();

        row.add_suffix(&content_box);

        Self {
            row,
            level_bar,
            value_label,
            sensor_path: sensor.input_path.clone(),
        }
    }

    /// Update the displayed temperature
    pub fn update(&self, temp: f32) {
        self.value_label.set_label(&hf_core::display::format_temp_precise(temp));
        self.level_bar.set_value(temp.clamp(0.0, 100.0) as f64);

        // Update styling based on temperature
        let css_class = if temp >= 85.0 {
            "error"
        } else if temp >= 70.0 {
            "warning"
        } else {
            ""
        };

        self.value_label.set_css_classes(&["numeric", css_class].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>());
    }

    /// Get the sensor path for reading updates
    pub fn sensor_path(&self) -> &PathBuf {
        &self.sensor_path
    }

    /// Get the widget
    pub fn widget(&self) -> &adw::ActionRow {
        &self.row
    }
}

/// A group of temperature monitors
pub struct TempMonitorGroup {
    group: adw::PreferencesGroup,
    monitors: Vec<TempMonitor>,
}

impl TempMonitorGroup {
    pub fn new() -> Self {
        let group = adw::PreferencesGroup::builder()
            .title("Temperature Sensors")
            .build();

        // Load sensors from hardware (daemon authoritative)
        let mut monitors = Vec::new();

        match hf_core::daemon_list_hardware() {
            Ok(hw) => {
                for chip in hw.chips {
                    for sensor in chip.temperatures {
                        let temp_sensor = TemperatureSensor {
                            name: sensor.name,
                            input_path: PathBuf::from(sensor.path),
                            label: sensor.label,
                            current_temp: Some(sensor.value),
                        };

                        let monitor = TempMonitor::new(&chip.name, &temp_sensor);
                        group.add(monitor.widget());
                        monitors.push(monitor);
                    }
                }
            }
            Err(e) => {
                let error_row = adw::ActionRow::builder()
                    .title("Error loading sensors")
                    .subtitle(&e.to_string())
                    .build();
                group.add(&error_row);
            }
        }

        if monitors.is_empty() {
            let empty_row = adw::ActionRow::builder()
                .title("No temperature sensors found")
                .subtitle("Check if lm-sensors is installed")
                .build();
            group.add(&empty_row);
        }

        Self { group, monitors }
    }

    /// Refresh all temperature readings
    /// PERFORMANCE: Uses cached sensor data from runtime instead of blocking daemon IPC
    pub fn refresh(&self) {
        // Try to get cached sensor data from runtime (non-blocking, now uses blocking_read internally)
        let cached_temps = crate::runtime::get_sensors();
        
        for monitor in &self.monitors {
            let path = monitor.sensor_path().to_string_lossy();
            
            // First try cached data, fall back to direct read only if cache miss
            let temp = cached_temps.as_ref()
                .and_then(|data| {
                    data.temperatures.iter()
                        .find(|t| t.path == path.as_ref())
                        .map(|t| t.temp_celsius)
                })
                .or_else(|| {
                    // Fallback: daemon read (should be rare with blocking_read fix)
                    daemon_client::daemon_read_temperature(path.as_ref()).ok()
                });
            
            if let Some(temp) = temp {
                monitor.update(temp);
            }
        }
    }

    /// Get the widget
    pub fn widget(&self) -> &adw::PreferencesGroup {
        &self.group
    }
}

impl Default for TempMonitorGroup {
    fn default() -> Self {
        Self::new()
    }
}
