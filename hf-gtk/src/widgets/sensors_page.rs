//! Temperature Sensors Page
//!
//! Displays all detected temperature sensors with live readings.
//! Sensors are grouped by hardware chip for easy identification.
//! Now includes GPU temperature sensors from NVIDIA and AMD GPUs.

#![allow(dead_code)]

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, Entry, Label, Orientation, ScrolledWindow};
use gtk4::glib;
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use super::cpu_info_card::CpuInfoCard;
use super::gpu_info_card::GpuInfoCard;

use hf_core::daemon_client;

// ============================================================================
// Constants
// ============================================================================

/// Get the user-configured poll interval from settings
fn get_poll_interval_ms() -> u64 {
    let interval = hf_core::get_cached_settings().general.poll_interval_ms as u64;
    interval.max(50) // Minimum 50ms for safety
}

// ============================================================================
// Types
// ============================================================================

/// Holds references to a sensor's UI elements for live updates
struct SensorDisplay {
    path: String,
    temp_label: Label,
}

/// Holds references to a fan sensor's UI elements for live updates
struct FanDisplay {
    path: String,
    rpm_label: Label,
    pwm_label: Option<Label>,
}

/// Holds references to GPU display elements for live updates
struct GpuDisplay {
    index: u32,
    card: GpuInfoCard,
}

/// Holds reference to CPU info card for live updates
struct CpuDisplay {
    card: CpuInfoCard,
}

/// Sensors page widget
pub struct SensorsPage {
    container: GtkBox,
    sensors: Rc<RefCell<Vec<SensorDisplay>>>,
    fans: Rc<RefCell<Vec<FanDisplay>>>,
    gpu_displays: Rc<RefCell<Vec<GpuDisplay>>>,
    cpu_display: Rc<RefCell<Option<CpuDisplay>>>,
}

impl SensorsPage {
    pub fn new() -> Self {
        let container = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .build();

        // Header - HIG: consistent 24px margins, 12px bottom spacing
        let header_box = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .margin_start(24)
            .margin_end(24)
            .margin_top(24)
            .margin_bottom(12)
            .build();

        let title = Label::builder()
            .label("Temperature Sensors")
            .css_classes(["title-1"])
            .hexpand(true)
            .halign(gtk4::Align::Start)
            .build();

        let refresh_btn = Button::builder()
            .icon_name("view-refresh-symbolic")
            .css_classes(["circular", "flat"])
            .tooltip_text("Refresh sensor list")
            .build();

        header_box.append(&title);
        header_box.append(&refresh_btn);
        container.append(&header_box);

        // Scrollable list
        let scroll = ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .build();

        let list_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(18)
            .margin_start(24)
            .margin_end(24)
            .margin_top(0)
            .margin_bottom(24)
            .build();

        let sensors: Rc<RefCell<Vec<SensorDisplay>>> = Rc::new(RefCell::new(Vec::new()));
        let fans: Rc<RefCell<Vec<FanDisplay>>> = Rc::new(RefCell::new(Vec::new()));
        let gpu_displays: Rc<RefCell<Vec<GpuDisplay>>> = Rc::new(RefCell::new(Vec::new()));
        let cpu_display: Rc<RefCell<Option<CpuDisplay>>> = Rc::new(RefCell::new(None));

        // ================================================================
        // CPU Section
        // ================================================================
        let cpu_section_label = Label::builder()
            .label("Processor")
            .css_classes(["title-2"])
            .halign(gtk4::Align::Start)
            .margin_top(6)
            .margin_bottom(6)
            .build();
        list_box.append(&cpu_section_label);

        let cpu_card = CpuInfoCard::new();
        list_box.append(cpu_card.widget());
        *cpu_display.borrow_mut() = Some(CpuDisplay { card: cpu_card });

        // ================================================================
        // GPU Section (Sensor info only - NO controls)
        // ================================================================
        // Use daemon for GPU enumeration (authoritative)
        if let Ok(daemon_gpus) = daemon_client::daemon_list_gpus() {
            if !daemon_gpus.is_empty() {
                let gpu_section_label = Label::builder()
                    .label("Graphics")
                    .css_classes(["title-2"])
                    .halign(gtk4::Align::Start)
                    .margin_top(12)
                    .margin_bottom(6)
                    .build();
                list_box.append(&gpu_section_label);

                // Convert daemon GPU info to GpuDevice format
                for daemon_gpu in &daemon_gpus {
                    // Parse vendor string to enum
                    let vendor = match daemon_gpu.vendor.to_lowercase().as_str() {
                        "nvidia" => hf_core::GpuVendor::Nvidia,
                        "amd" => hf_core::GpuVendor::Amd,
                        "intel" => hf_core::GpuVendor::Intel,
                        _ => hf_core::GpuVendor::Nvidia, // Default fallback
                    };
                    
                    // Create a minimal GpuDevice from daemon data
                    let gpu = hf_core::GpuDevice {
                        index: daemon_gpu.index,
                        name: daemon_gpu.name.clone(),
                        vendor,
                        pci_bus_id: None,
                        vram_total_mb: None,
                        vram_used_mb: None,
                        temperatures: Vec::new(), // Will be populated by live updates
                        fans: Vec::new(),
                        power_watts: None,
                        power_limit_watts: None,
                        utilization_percent: None,
                    };
                    
                    let card = GpuInfoCard::new(&gpu);
                    list_box.append(card.widget());
                    
                    gpu_displays.borrow_mut().push(GpuDisplay {
                        index: daemon_gpu.index,
                        card,
                    });
                }
            }
        }

        // ================================================================
        // Fan Sensors Section
        // ================================================================
        if let Ok(hw) = daemon_client::daemon_list_hardware() {
            let mut has_fans = false;
            for chip in &hw.chips {
                if !chip.fans.is_empty() {
                    has_fans = true;
                    break;
                }
            }
            
            if has_fans {
                let fan_section_label = Label::builder()
                    .label("Fan Sensors")
                    .css_classes(["title-2"])
                    .halign(gtk4::Align::Start)
                    .margin_top(12)
                    .margin_bottom(6)
                    .build();
                list_box.append(&fan_section_label);

                let fan_group = adw::PreferencesGroup::builder().build();

                for chip in &hw.chips {
                    for fan in &chip.fans {
                        let fan_path = fan.path.clone();
                        let default_label = fan.label.clone().unwrap_or_else(|| fan.name.clone());
                        let chip_name = chip.name.clone();
                        
                        // Check if there's a matching PWM controller
                        let fan_index = fan.name.chars()
                            .find(|c| c.is_ascii_digit())
                            .and_then(|c| c.to_digit(10));
                        
                        let has_pwm = fan_index.is_some() && fan_index
                            .map(|idx| {
                                let pwm_name = format!("pwm{}", idx);
                                chip.pwms.iter().any(|p| p.name == pwm_name)
                            })
                            .unwrap_or(false);
                        
                        let display_name = format!("{} • {}", chip_name, default_label);
                        
                        let row = adw::ActionRow::builder()
                            .title(&display_name)
                            .subtitle(&fan_path)
                            .build();

                        // Create horizontal box for RPM and PWM display
                        let stats_box = gtk4::Box::builder()
                            .orientation(gtk4::Orientation::Horizontal)
                            .spacing(12)
                            .build();

                        let rpm_placeholder = "-- RPM";
                        let rpm_label = Label::builder()
                            .label(rpm_placeholder)
                            .css_classes(["title-3", "numeric"])
                            .build();

                        stats_box.append(&rpm_label);
                        
                        // Add PWM label if this fan has a controller
                        let pwm_label = if has_pwm {
                            let label = Label::builder()
                                .label("--%")
                                .css_classes(["title-3", "numeric", "dim-label"])
                                .build();
                            stats_box.append(&label);
                            Some(label)
                        } else {
                            None
                        };
                        
                        row.add_suffix(&stats_box);
                        fan_group.add(&row);

                        fans.borrow_mut().push(FanDisplay {
                            path: fan_path,
                            rpm_label,
                            pwm_label,
                        });
                    }
                }

                list_box.append(&fan_group);
            }
        }

        // ================================================================
        // Other Sensors Section (Motherboard, etc)
        // ================================================================
        let other_section_label = Label::builder()
            .label("Other Sensors")
            .css_classes(["title-2"])
            .halign(gtk4::Align::Start)
            .margin_top(12)
            .margin_bottom(6)
            .build();
        list_box.append(&other_section_label);

        let group = adw::PreferencesGroup::builder()
            .build();

        // Use daemon for hardware enumeration (authoritative)
        let hw_result = daemon_client::daemon_list_hardware();
        
        if hw_result.is_err() {
            // Daemon unreachable - show empty state
            let empty_state = adw::StatusPage::builder()
                .icon_name("network-error-symbolic")
                .title("Cannot Connect to Daemon")
                .description("The hyperfand daemon is not responding. Sensor data is unavailable.\n\nMake sure the daemon is installed and running.")
                .build();
            
            let retry_btn = gtk4::Button::builder()
                .label("Retry")
                .css_classes(["suggested-action", "pill"])
                .halign(gtk4::Align::Center)
                .build();
            
            let scroll_for_retry = scroll.clone();
            retry_btn.connect_clicked(move |_| {
                // Trigger a refresh by recreating the page
                if let Some(parent) = scroll_for_retry.parent() {
                    // This is a bit hacky but works for a quick refresh
                    scroll_for_retry.set_visible(false);
                    scroll_for_retry.set_visible(true);
                }
            });
            
            empty_state.set_child(Some(&retry_btn));
            scroll.set_child(Some(&empty_state));
            container.append(&scroll);
            
            return Self {
                container,
                sensors,
                fans,
                gpu_displays,
                cpu_display,
            };
        }
        
        if let Ok(hw) = hw_result {
            for chip in hw.chips {
                // Skip CPU and GPU chips as they're handled in their own sections
                let is_cpu_chip = chip.name.contains("coretemp")
                    || chip.name.contains("k10temp")
                    || chip.name.contains("zenpower")
                    || chip.name.contains("cpu")
                    || chip.name.contains("acpitz");
                let is_gpu_chip = chip.name.contains("amdgpu") || chip.name.contains("nvidia");

                if is_cpu_chip || is_gpu_chip {
                    continue;
                }

                for temp in &chip.temperatures {
                    let sensor_path = temp.path.clone();
                    let default_label = temp.label.clone().unwrap_or_else(|| temp.name.clone());
                    let chip_name = chip.name.clone();
                    
                    // Check for user-defined friendly name
                    let display_name = hf_core::get_sensor_friendly_name(&sensor_path)
                        .ok()
                        .flatten()
                        .unwrap_or_else(|| default_label.clone());
                    
                    let row = adw::ActionRow::builder()
                        .title(&display_name)
                        .subtitle(&format!("{} • {}", chip_name, sensor_path))
                        .activatable(true)
                        .build();

                    let temp_placeholder = format!("--{}", hf_core::display::temp_unit_suffix());
                    let temp_label = Label::builder()
                        .label(&temp_placeholder)
                        .css_classes(["title-3", "numeric"])
                        .build();

                    // Edit button for friendly name
                    let edit_btn = Button::builder()
                        .icon_name("document-edit-symbolic")
                        .css_classes(["flat", "circular"])
                        .tooltip_text("Set friendly name")
                        .valign(gtk4::Align::Center)
                        .build();

                    let sensor_path_for_edit = sensor_path.clone();
                    let default_label_for_edit = default_label.clone();
                    let row_for_edit = row.clone();
                    edit_btn.connect_clicked(move |btn| {
                        Self::show_rename_dialog(
                            btn,
                            &sensor_path_for_edit,
                            &default_label_for_edit,
                            &row_for_edit,
                        );
                    });

                    row.add_suffix(&edit_btn);
                    row.add_suffix(&temp_label);
                    group.add(&row);

                    sensors.borrow_mut().push(SensorDisplay {
                        path: sensor_path,
                        temp_label,
                    });
                }
            }
        }

        list_box.append(&group);
        scroll.set_child(Some(&list_box));
        container.append(&scroll);

        let this = Self { 
            container, 
            sensors,
            fans,
            gpu_displays,
            cpu_display,
        };

        // Setup live updates
        this.setup_live_updates();

        this
    }

    /// Start the live temperature update loop
    fn setup_live_updates(&self) {
        let sensors = self.sensors.clone();
        let fans = self.fans.clone();
        let gpu_displays = self.gpu_displays.clone();
        let cpu_display = self.cpu_display.clone();
        let container = self.container.clone();

        glib::timeout_add_local(
            Duration::from_millis(get_poll_interval_ms()),
            move || {
                // PERFORMANCE: Only update if this page is visible (mapped to screen)
                if !container.is_mapped() {
                    return glib::ControlFlow::Continue;
                }
                
                // Update CPU info card
                Self::update_cpu_readings(&cpu_display);
                
                // Update other sensors
                Self::update_sensor_readings(&sensors);
                
                // Update fan sensors
                Self::update_fan_readings(&fans);
                
                // Update GPU sensors
                Self::update_gpu_readings(&gpu_displays);
                
                glib::ControlFlow::Continue
            }
        );
    }

    /// Update CPU info card
    fn update_cpu_readings(cpu_display: &Rc<RefCell<Option<CpuDisplay>>>) {
        if let Some(display) = cpu_display.borrow().as_ref() {
            display.card.update();
        }
    }

    /// Read and display current temperatures for all sensors
    /// PERFORMANCE: Uses cached sensor data from runtime instead of blocking file I/O
    fn update_sensor_readings(sensors: &Rc<RefCell<Vec<SensorDisplay>>>) {
        // Try to get cached sensor data from runtime (non-blocking)
        let cached_temps = crate::runtime::get_sensors();
        
        for sensor in sensors.borrow().iter() {
            // First try cached data, fall back to direct read only if cache miss
            let temp = cached_temps.as_ref()
                .and_then(|data| {
                    data.temperatures.iter()
                        .find(|t| t.path == sensor.path)
                        .map(|t| t.temp_celsius)
                })
                .or_else(|| {
                    // Fallback: daemon read (should be rare)
                    daemon_client::daemon_read_temperature(&sensor.path).ok()
                });
            
            if let Some(temp) = temp {
                let new_text = hf_core::display::format_temp_precise(temp);
                if sensor.temp_label.text() != new_text {
                    sensor.temp_label.set_label(&new_text);
                }
            }
        }
    }

    /// Read and display current fan RPM for all fan sensors
    /// PERFORMANCE: Uses cached sensor data from runtime instead of blocking file I/O
    fn update_fan_readings(fans: &Rc<RefCell<Vec<FanDisplay>>>) {
        // Try to get cached sensor data from runtime (non-blocking)
        let cached_data = crate::runtime::get_sensors();
        
        for fan in fans.borrow().iter() {
            // First try cached data, fall back to direct read only if cache miss
            let fan_data = cached_data.as_ref()
                .and_then(|data| {
                    data.fans.iter()
                        .find(|f| f.path == fan.path)
                });
            
            let rpm = fan_data
                .and_then(|f| f.rpm)
                .or_else(|| {
                    // Fallback: daemon read (should be rare)
                    daemon_client::daemon_read_fan_rpm(&fan.path).ok()
                });
            
            let new_rpm_text = rpm
                .map(|r| format!("{} RPM", r))
                .unwrap_or_else(|| "-- RPM".to_string());
            
            if fan.rpm_label.text() != new_rpm_text {
                fan.rpm_label.set_label(&new_rpm_text);
            }
            
            // Update PWM percentage if label exists
            if let Some(pwm_label) = &fan.pwm_label {
                let percent = fan_data.and_then(|f| f.percent);
                
                let new_pwm_text = percent
                    .map(|p| format!("{:.0}%", p))
                    .unwrap_or_else(|| "--%".to_string());
                
                if pwm_label.text() != new_pwm_text {
                    pwm_label.set_label(&new_pwm_text);
                }
            }
        }
    }

    /// Update GPU readings
    /// PERFORMANCE: Uses cached GPU data from runtime instead of blocking I/O
    fn update_gpu_readings(gpu_displays: &Rc<RefCell<Vec<GpuDisplay>>>) {
        // Try cached data first (non-blocking)
        let cached = crate::runtime::get_sensors();
        
        if let Some(sensor_data) = cached {
            let displays = gpu_displays.borrow();
            for display in displays.iter() {
                if let Some(gpu_reading) = sensor_data.gpus.iter().find(|g| g.index == display.index) {
                    // Convert runtime GpuReading to hf_core GpuDevice for update
                    // Only update labels that changed
                    display.card.update_from_reading(gpu_reading);
                }
            }
        }
    }

    pub fn widget(&self) -> &GtkBox {
        &self.container
    }

    /// Show dialog to rename a sensor with a friendly name
    fn show_rename_dialog(
        btn: &Button,
        sensor_path: &str,
        default_label: &str,
        row: &adw::ActionRow,
    ) {
        let dialog = adw::Window::builder()
            .title("Set Friendly Name")
            .default_width(400)
            .default_height(200)
            .modal(true)
            .build();

        // Set transient parent
        if let Some(window) = btn.root().and_then(|r| r.downcast::<gtk4::Window>().ok()) {
            dialog.set_transient_for(Some(&window));
        }

        let content = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(18)
            .margin_start(24)
            .margin_end(24)
            .margin_top(18)
            .margin_bottom(24)
            .build();

        // Header bar
        let header = adw::HeaderBar::builder()
            .show_end_title_buttons(false)
            .show_start_title_buttons(false)
            .build();

        let cancel_btn = Button::builder().label("Cancel").build();
        let save_btn = Button::builder()
            .label("Save")
            .css_classes(["suggested-action"])
            .build();

        header.pack_start(&cancel_btn);
        header.pack_end(&save_btn);

        // Current name info
        let info_label = Label::builder()
            .label(&format!("Original: {}", default_label))
            .css_classes(["dim-label"])
            .halign(gtk4::Align::Start)
            .build();

        // Name entry
        let current_friendly = hf_core::get_sensor_friendly_name(sensor_path)
            .ok()
            .flatten()
            .unwrap_or_default();

        let name_entry = Entry::builder()
            .placeholder_text("Enter friendly name (leave empty to reset)")
            .text(&current_friendly)
            .build();

        content.append(&info_label);
        content.append(&name_entry);

        let main_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .build();
        main_box.append(&header);
        main_box.append(&content);

        dialog.set_content(Some(&main_box));

        // Cancel handler
        let dialog_for_cancel = dialog.clone();
        cancel_btn.connect_clicked(move |_| {
            dialog_for_cancel.close();
        });

        // Save handler
        let dialog_for_save = dialog.clone();
        let sensor_path_for_save = sensor_path.to_string();
        let default_label_for_save = default_label.to_string();
        let row_for_save = row.clone();
        save_btn.connect_clicked(move |_| {
            let new_name = name_entry.text().to_string();
            
            // Save to settings
            if let Err(e) = hf_core::set_sensor_friendly_name(&sensor_path_for_save, &new_name) {
                tracing::warn!("Failed to save sensor friendly name: {}", e);
            }

            // Update row title
            let display_name = if new_name.is_empty() {
                default_label_for_save.clone()
            } else {
                new_name
            };
            row_for_save.set_title(&display_name);

            dialog_for_save.close();
        });

        dialog.present();
    }
}

impl Default for SensorsPage {
    fn default() -> Self {
        Self::new()
    }
}
