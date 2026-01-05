//! CPU Info Card Widget
//!
//! Displays CPU information including temperature, model, cores, and memory usage.
//! This is a READ-ONLY card for sensor display.

#![allow(dead_code)]

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, Orientation, ProgressBar};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use hf_core::daemon_client;

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

/// Holds temperature sensor references for live updates
pub struct CpuTempSensor {
    pub path: String,
    pub label: Label,
}

/// CPU info card widget (read-only sensor display)
pub struct CpuInfoCard {
    card: adw::Bin,
    temp_sensors: Rc<RefCell<Vec<CpuTempSensor>>>,
    memory_bar: ProgressBar,
    memory_label: Label,
}

impl CpuInfoCard {
    /// Create a new CPU info card widget
    pub fn new() -> Self {
        let card = adw::Bin::builder().css_classes(["card"]).build();

        let content = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(layout::CARD_SPACING)
            .margin_start(layout::CARD_MARGIN)
            .margin_end(layout::CARD_MARGIN)
            .margin_top(layout::CARD_MARGIN)
            .margin_bottom(layout::CARD_MARGIN)
            .build();

        let temp_sensors: Rc<RefCell<Vec<CpuTempSensor>>> = Rc::new(RefCell::new(Vec::new()));

        // Get system info
        let system_info = hf_core::get_system_summary().ok();

        // Header with CPU name
        let header = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(layout::CARD_SPACING)
            .build();

        let cpu_name = system_info
            .as_ref()
            .map(|s| s.cpu_model.clone())
            .unwrap_or_else(|| "Unknown CPU".to_string());

        let name_label = Label::builder()
            .label(&cpu_name)
            .css_classes(["title-3"])
            .hexpand(true)
            .halign(gtk4::Align::Start)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .build();

        let cores_badge = Label::builder()
            .label(&format!(
                "{} cores",
                system_info.as_ref().map(|s| s.cpu_cores).unwrap_or(0)
            ))
            .css_classes(["caption", "dim-label"])
            .build();

        header.append(&name_label);
        header.append(&cores_badge);
        content.append(&header);

        // Stats row: Primary temp, Cores, Memory
        let stats_box = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(layout::STATS_SPACING)
            .homogeneous(true)
            .build();

        // Primary CPU Temperature
        let temp_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(layout::LABEL_SPACING)
            .build();

        let temp_title = Label::builder()
            .label("Temperature")
            .css_classes(["caption", "dim-label"])
            .build();

        let temp_placeholder = format!("--{}", hf_core::display::temp_unit_suffix());
        let primary_temp_label = Label::builder()
            .label(&temp_placeholder)
            .css_classes(["title-2", "numeric"])
            .build();

        temp_box.append(&temp_title);
        temp_box.append(&primary_temp_label);
        stats_box.append(&temp_box);

        // Cores
        let cores_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(layout::LABEL_SPACING)
            .build();

        let cores_title = Label::builder()
            .label("Threads")
            .css_classes(["caption", "dim-label"])
            .build();

        let cores_value = Label::builder()
            .label(&format!(
                "{}",
                system_info.as_ref().map(|s| s.cpu_cores).unwrap_or(0)
            ))
            .css_classes(["title-2", "numeric"])
            .build();

        cores_box.append(&cores_title);
        cores_box.append(&cores_value);
        stats_box.append(&cores_box);

        // Memory Available
        let mem_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(layout::LABEL_SPACING)
            .build();

        let mem_title = Label::builder()
            .label("Memory")
            .css_classes(["caption", "dim-label"])
            .build();

        let mem_total = system_info.as_ref().map(|s| s.memory_total_mb).unwrap_or(0);
        let memory_label = Label::builder()
            .label(&format!("{} MB", mem_total))
            .css_classes(["title-2", "numeric"])
            .build();

        mem_box.append(&mem_title);
        mem_box.append(&memory_label.clone());
        stats_box.append(&mem_box);

        content.append(&stats_box);

        // Memory usage bar
        let mem_group = adw::PreferencesGroup::builder().title("Memory Usage").build();

        let memory_bar = ProgressBar::builder().show_text(true).build();

        if let Some(info) = &system_info {
            let used_mb = info.memory_total_mb.saturating_sub(info.memory_available_mb);
            let usage_fraction = used_mb as f64 / info.memory_total_mb as f64;
            memory_bar.set_fraction(usage_fraction);
            memory_bar.set_text(Some(&format!(
                "{} / {} MB ({:.0}%)",
                used_mb,
                info.memory_total_mb,
                usage_fraction * PERCENT_MULTIPLIER
            )));
        } else {
            memory_bar.set_text(Some("N/A"));
        }

        let mem_row = adw::ActionRow::builder().build();
        mem_row.add_suffix(&memory_bar);
        mem_group.add(&mem_row);
        content.append(&mem_group);

        // CPU Temperature sensors
        let temp_group = adw::PreferencesGroup::builder()
            .title("Temperature Sensors")
            .build();

        // Find CPU-related temperature sensors via daemon (authoritative)
        if let Ok(hw) = hf_core::daemon_list_hardware() {
            for chip in hw.chips {
                // Only include CPU-related chips (coretemp, k10temp, zenpower, etc.)
                let is_cpu_chip = chip.name.contains("coretemp")
                    || chip.name.contains("k10temp")
                    || chip.name.contains("zenpower")
                    || chip.name.contains("cpu")
                    || chip.name.contains("acpitz");

                if !is_cpu_chip {
                    continue;
                }

                for temp in &chip.temperatures {
                    let label_text = temp.label.clone().unwrap_or_else(|| temp.name.clone());

                    let row = adw::ActionRow::builder()
                        .title(&label_text)
                        .subtitle(&chip.name)
                        .build();

                    let temp_placeholder = format!("--{}", hf_core::display::temp_unit_suffix());
                    let temp_label = Label::builder()
                        .label(&temp_placeholder)
                        .css_classes(["title-3", "numeric"])
                        .build();

                    row.add_suffix(&temp_label);
                    temp_group.add(&row);

                    temp_sensors.borrow_mut().push(CpuTempSensor {
                        path: temp.path.clone(),
                        label: temp_label,
                    });
                }
            }
        }

        // Update primary temp label reference
        if let Some(first_sensor) = temp_sensors.borrow().first() {
            // Clone the label reference for primary display
            let primary_path = first_sensor.path.clone();
            if let Ok(temp) = daemon_client::daemon_read_temperature(&primary_path) {
                primary_temp_label.set_label(&hf_core::display::format_temp(temp));
            }
        }

        // Store primary temp label in sensors for updates
        if !temp_sensors.borrow().is_empty() {
            let first_path = temp_sensors.borrow()[0].path.clone();
            temp_sensors.borrow_mut().insert(
                0,
                CpuTempSensor {
                    path: first_path,
                    label: primary_temp_label,
                },
            );
        }

        content.append(&temp_group);
        card.set_child(Some(&content));

        Self {
            card,
            temp_sensors,
            memory_bar,
            memory_label,
        }
    }

    /// Update the card with fresh sensor data
    /// PERFORMANCE: Uses cached data from runtime, avoids blocking I/O on main thread
    pub fn update(&self) {
        // Try to get cached sensor data (non-blocking)
        let cached = crate::runtime::get_sensors();
        
        // Update temperature sensors (only if value changed)
        for sensor in self.temp_sensors.borrow().iter() {
            // Try cached data first, fallback to direct read only if needed
            let temp = cached.as_ref()
                .and_then(|data| {
                    data.temperatures.iter()
                        .find(|t| t.path == sensor.path)
                        .map(|t| t.temp_celsius)
                })
                .or_else(|| daemon_client::daemon_read_temperature(&sensor.path).ok());
            
            if let Some(temp) = temp {
                let new_text = hf_core::display::format_temp_precise(temp);
                if sensor.label.text() != new_text {
                    sensor.label.set_label(&new_text);
                }
            }
        }

        // Update memory usage - use perf module's cached memory data
        // This avoids calling get_system_summary() which does blocking I/O
        if crate::perf::is_enabled() {
            // If perf is enabled, we already have cached memory data
            // Skip redundant memory update here
        } else {
            // Only do blocking I/O if perf module isn't caching it
            // And rate-limit to avoid excessive reads
            static LAST_MEM_UPDATE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let last = LAST_MEM_UPDATE.load(std::sync::atomic::Ordering::Relaxed);
            
            // Only update memory every 2 seconds
            if now > last + 2 {
                LAST_MEM_UPDATE.store(now, std::sync::atomic::Ordering::Relaxed);
                // PERFORMANCE: Use fast memory functions instead of full get_system_summary()
                let total_mb = hf_core::get_memory_total_mb();
                let available_mb = hf_core::get_memory_available_mb();
                let used_mb = total_mb.saturating_sub(available_mb);
                let usage_fraction = used_mb as f64 / total_mb as f64;
                
                if (self.memory_bar.fraction() - usage_fraction).abs() > 0.001 {
                    self.memory_bar.set_fraction(usage_fraction);
                    self.memory_bar.set_text(Some(&format!(
                        "{} / {} MB ({:.0}%)",
                        used_mb,
                        total_mb,
                        usage_fraction * PERCENT_MULTIPLIER
                    )));
                }
            }
        }
    }

    pub fn temp_sensors(&self) -> Rc<RefCell<Vec<CpuTempSensor>>> {
        self.temp_sensors.clone()
    }

    pub fn widget(&self) -> &adw::Bin {
        &self.card
    }
}

impl Default for CpuInfoCard {
    fn default() -> Self {
        Self::new()
    }
}
