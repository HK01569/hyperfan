//! Fan Pairing Page
//!
//! GNOME HIG compliant page for manually pairing PWM controls with fan RPM sensors.
//! Shows a list of PWM controls as clickable rows that open a modal dialog with
//! fan RPM list and PWM test slider.

#![allow(dead_code)]

use gtk4::prelude::*;
use gtk4::glib;
use gtk4::gio;
use gtk4::{Box as GtkBox, Button, GestureClick, Label, ListBox, Orientation, Scale, ScrolledWindow, SearchEntry, SelectionMode};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;
use tracing::{debug, warn};

use hf_core::daemon_client;

/// Data for a PWM control with optional fan pairing
#[derive(Clone, Debug)]
pub struct PwmPairingData {
    /// Stable UUID for this PWM control (survives hwmon reindexing)
    pub pwm_uuid: String,
    pub pwm_path: String,
    pub pwm_name: String,
    /// Controller/chip name (e.g., "nct6798")
    pub controller_name: String,
    /// PWM channel number (e.g., "1" for pwm1)
    pub pwm_num: String,
    /// UUID of paired fan sensor
    pub fan_uuid: Option<String>,
    pub fan_path: Option<String>,
    pub fan_name: Option<String>,
    pub friendly_name: Option<String>,
    pub current_pwm: u8,
    /// Last manual PWM value set via slider (for display in list)
    pub manual_pwm: Option<u8>,
}

/// Data for a fan sensor
#[derive(Clone, Debug)]
pub struct FanSensorData {
    /// Stable UUID for this fan sensor (survives hwmon reindexing)
    pub uuid: String,
    pub path: String,
    pub name: String,
    pub label: Option<String>,
    pub rpm: Option<u32>,
}

/// Fan Pairing Page widget
pub struct FanPairingPage {
    pub container: GtkBox,
    pwm_list: ListBox,
    filter_entry: SearchEntry,
    state: Rc<RefCell<FanPairingState>>,
}

impl Clone for FanPairingPage {
    fn clone(&self) -> Self {
        Self {
            container: self.container.clone(),
            pwm_list: self.pwm_list.clone(),
            filter_entry: self.filter_entry.clone(),
            state: self.state.clone(),
        }
    }
}

#[derive(Default)]
struct FanPairingState {
    pwm_controls: Vec<PwmPairingData>,
    fan_sensors: Vec<FanSensorData>,
    filter_text: String,
}

impl FanPairingPage {
    pub fn new() -> Self {
        let container = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .build();

        let state = Rc::new(RefCell::new(FanPairingState::default()));

        // Header with title and description (GNOME HIG: clear page purpose)
        let header_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(8)
            .margin_start(24)
            .margin_end(24)
            .margin_top(24)
            .margin_bottom(12)
            .build();

        let title = Label::builder()
            .label("Fan Pairing")
            .css_classes(["title-1"])
            .halign(gtk4::Align::Start)
            .build();

        let description = Label::builder()
            .label("Click a PWM control to pair it with a fan sensor and test which fan responds.")
            .css_classes(["dim-label"])
            .halign(gtk4::Align::Start)
            .wrap(true)
            .xalign(0.0)
            .build();

        // Title row with auto-detect and refresh buttons (HIG: action buttons at end)
        let title_row = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .build();
        title.set_hexpand(true);
        title_row.append(&title);
        
        let refresh_btn = Button::builder()
            .icon_name("view-refresh-symbolic")
            .tooltip_text("Refresh hardware list")
            .valign(gtk4::Align::Center)
            .build();
        title_row.append(&refresh_btn);
        
        let auto_detect_btn = Button::builder()
            .label("Auto Detect")
            .css_classes(["suggested-action"])
            .valign(gtk4::Align::Center)
            .build();
        title_row.append(&auto_detect_btn);

        header_box.append(&title_row);
        header_box.append(&description);
        container.append(&header_box);

        // Filter/search entry (HIG: consistent margins)
        let filter_entry = SearchEntry::builder()
            .placeholder_text("Filter PWM controls...")
            .margin_start(24)
            .margin_end(24)
            .margin_top(4)
            .margin_bottom(16)
            .build();
        container.append(&filter_entry);

        // Scrollable list of PWM controls
        let scroll = ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .build();

        let pwm_list = ListBox::builder()
            .selection_mode(SelectionMode::None)
            .css_classes(["boxed-list"])
            .margin_start(24)
            .margin_end(24)
            .margin_bottom(24)
            .build();

        scroll.set_child(Some(&pwm_list));
        container.append(&scroll);

        let page = Self {
            container,
            pwm_list,
            filter_entry: filter_entry.clone(),
            state: state.clone(),
        };

        // Connect filter entry
        let state_filter = state.clone();
        let pwm_list_filter = page.pwm_list.clone();
        filter_entry.connect_search_changed(move |entry| {
            let text = entry.text().to_string().to_lowercase();
            state_filter.borrow_mut().filter_text = text;
            Self::rebuild_list_static(&state_filter, &pwm_list_filter);
        });

        // Connect refresh button
        let page_refresh = Rc::new(RefCell::new(None::<FanPairingPage>));
        let page_refresh_clone = page_refresh.clone();
        refresh_btn.connect_clicked(move |_| {
            if let Some(page) = page_refresh_clone.borrow().as_ref() {
                page.refresh();
            }
        });
        
        // Connect auto-detect button
        let state_autodetect = state.clone();
        let pwm_list_autodetect = page.pwm_list.clone();
        let _container_ref = page.container.clone();
        auto_detect_btn.connect_clicked(move |btn| {
            let window = btn.root()
                .and_then(|r| r.downcast::<gtk4::Window>().ok());
            Self::show_autodetect_dialog(window.as_ref(), &state_autodetect, &pwm_list_autodetect);
        });

        // Initial load
        page.refresh();
        
        // Store self-reference for refresh button
        *page_refresh.borrow_mut() = Some(page.clone());

        page
    }

    /// Refresh the page data from hardware
    pub fn refresh(&self) {
        self.load_hardware_data();
        self.rebuild_list();
    }

    fn load_hardware_data(&self) {
        let mut state = self.state.borrow_mut();
        state.pwm_controls.clear();
        state.fan_sensors.clear();

        // Daemon authoritative: load PWM controls and fan sensors via daemon IPC
        if let Ok(hw) = daemon_client::daemon_list_hardware() {
            for chip in &hw.chips {
                for pwm in &chip.pwms {
                    // Extract PWM number from name (e.g., "pwm1" -> "1")
                    let pwm_num: String = pwm.name.chars().filter(|c| c.is_ascii_digit()).collect();
                    let pwm_num = if pwm_num.is_empty() { "?".to_string() } else { pwm_num };

                    state.pwm_controls.push(PwmPairingData {
                        pwm_uuid: pwm.uuid.clone(),
                        pwm_path: pwm.path.clone(),
                        pwm_name: format!("{} - {}", chip.name, pwm.name),
                        controller_name: chip.name.clone(),
                        pwm_num,
                        fan_uuid: None,
                        fan_path: None,
                        fan_name: None,
                        friendly_name: None,
                        current_pwm: pwm.value,
                        manual_pwm: None,
                    });
                }

                for fan in &chip.fans {
                    state.fan_sensors.push(FanSensorData {
                        uuid: fan.uuid.clone(),
                        path: fan.path.clone(),
                        name: fan.name.clone(),
                        label: fan.label.clone(),
                        rpm: fan.rpm,
                    });
                }
            }
        }

        // Load saved pairings from settings - match by UUID first, then path
        if let Ok(settings) = hf_core::load_settings() {
            for pairing in &settings.pwm_fan_pairings {
                // Try to find by UUID first (stable), then by path (fallback)
                let pwm = state.pwm_controls.iter_mut().find(|p| {
                    pairing.pwm_uuid.as_ref().map_or(false, |uuid| &p.pwm_uuid == uuid)
                        || p.pwm_path == pairing.pwm_path
                });
                
                if let Some(pwm) = pwm {
                    pwm.fan_uuid = pairing.fan_uuid.clone();
                    pwm.fan_path = pairing.fan_path.clone();
                    pwm.fan_name = pairing.fan_name.clone();
                    pwm.friendly_name = pairing.friendly_name.clone();
                }
            }
        }

        debug!("Loaded {} PWM controls, {} fan sensors", 
               state.pwm_controls.len(), state.fan_sensors.len());
    }

    fn rebuild_list(&self) {
        Self::rebuild_list_static(&self.state, &self.pwm_list);
    }

    fn rebuild_list_static(state: &Rc<RefCell<FanPairingState>>, pwm_list: &ListBox) {
        // Clear existing children
        while let Some(child) = pwm_list.first_child() {
            pwm_list.remove(&child);
        }

        let state_ref = state.borrow();
        let filter = &state_ref.filter_text;
        
        // Filter PWM controls
        let filtered: Vec<_> = state_ref.pwm_controls.iter()
            .filter(|pwm| {
                if filter.is_empty() {
                    return true;
                }
                pwm.pwm_name.to_lowercase().contains(filter) ||
                pwm.fan_name.as_ref().map(|n| n.to_lowercase().contains(filter)).unwrap_or(false)
            })
            .collect();
        
        if state_ref.pwm_controls.is_empty() {
            // Empty state
            let empty = adw::StatusPage::builder()
                .icon_name("fan-symbolic")
                .title("No PWM Controls Found")
                .description("No hardware PWM controls were detected on this system.")
                .build();
            pwm_list.append(&empty);
            return;
        }

        if filtered.is_empty() {
            // No matches for filter
            let empty = adw::StatusPage::builder()
                .icon_name("edit-find-symbolic")
                .title("No Matches")
                .description("No PWM controls match your filter.")
                .build();
            pwm_list.append(&empty);
            return;
        }

        // Create a clickable row for each PWM control
        for pwm in filtered {
            let row = Self::create_pwm_row_static(pwm, state);
            pwm_list.append(&row);
        }
    }

    fn create_pwm_row_static(pwm: &PwmPairingData, state: &Rc<RefCell<FanPairingState>>) -> adw::ActionRow {
        // Build title: "FriendlyName   PWM{N}     controller" or "PWM{N}     controller"
        let title = if let Some(ref friendly) = pwm.friendly_name {
            format!("{}   PWM{}     {}", friendly, pwm.pwm_num, pwm.controller_name)
        } else {
            format!("PWM{}     {}", pwm.pwm_num, pwm.controller_name)
        };
        
        let row = adw::ActionRow::builder()
            .title(&title)
            .activatable(true)
            .build();

        // Subtitle shows pairing status and manual PWM if set
        let subtitle = if let Some(ref fan_name) = pwm.fan_name {
            if let Some(manual_pwm) = pwm.manual_pwm {
                let pwm_percent = hf_core::constants::pwm::to_percent(manual_pwm);
                let pwm_display = hf_core::display::format_pwm_subtitle(manual_pwm, pwm_percent);
                format!("Paired with: {} • Test PWM: {}", fan_name, pwm_display)
            } else {
                format!("Paired with: {}", fan_name)
            }
        } else if let Some(manual_pwm) = pwm.manual_pwm {
            let pwm_percent = hf_core::constants::pwm::to_percent(manual_pwm);
            let pwm_display = hf_core::display::format_pwm_subtitle(manual_pwm, pwm_percent);
            format!("Not paired • Test PWM: {}", pwm_display)
        } else {
            "Not paired".to_string()
        };
        row.set_subtitle(&subtitle);

        // PWM/signal icon
        let icon = gtk4::Image::builder()
            .icon_name("speedometer-symbolic")
            .build();
        row.add_prefix(&icon);

        // Arrow to indicate clickable
        let arrow = gtk4::Image::builder()
            .icon_name("go-next-symbolic")
            .css_classes(["dim-label"])
            .build();
        row.add_suffix(&arrow);

        // Connect click via GestureClick for reliable activation
        let pwm_data = pwm.clone();
        let state = state.clone();
        let gesture = GestureClick::new();
        gesture.connect_released(move |gesture, _, _, _| {
            if let Some(widget) = gesture.widget() {
                if let Some(row) = widget.downcast_ref::<adw::ActionRow>() {
                    // Find the parent ListBox for rebuild after save
                    let pwm_list = row.parent()
                        .and_then(|p| p.downcast::<ListBox>().ok());
                    Self::show_pairing_dialog(row, &pwm_data, &state, pwm_list.as_ref());
                }
            }
        });
        row.add_controller(gesture);

        row
    }

    fn show_pairing_dialog(
        parent_row: &adw::ActionRow,
        pwm: &PwmPairingData,
        state: &Rc<RefCell<FanPairingState>>,
        pwm_list: Option<&ListBox>,
    ) {
        // Get the window for transient_for
        let window = parent_row.root()
            .and_then(|r| r.downcast::<gtk4::Window>().ok());

        // Adaptive dialog size based on number of fans
        let state_peek = state.borrow();
        let fan_count = state_peek.fan_sensors.len();
        drop(state_peek);
        let dialog_height = if fan_count == 0 {
            400
        } else if fan_count <= 3 {
            500
        } else if fan_count <= 6 {
            600
        } else {
            700
        };
        
        let dialog = adw::Window::builder()
            .title(&pwm.pwm_name)
            .default_width(500)
            .default_height(dialog_height)
            .modal(true)
            .build();

        if let Some(ref win) = window {
            dialog.set_transient_for(Some(win));
        }

        let content = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(0)
            .build();

        // Header bar with Cancel button
        let header = adw::HeaderBar::builder()
            .show_end_title_buttons(false)
            .build();
        
        let cancel_btn = Button::builder()
            .label("Cancel")
            .build();
        header.pack_start(&cancel_btn);
        
        let save_btn = Button::builder()
            .label("Save")
            .css_classes(["suggested-action"])
            .sensitive(false)
            .build();
        header.pack_end(&save_btn);
        
        content.append(&header);

        // Main content area
        let main_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(24)
            .margin_start(24)
            .margin_end(24)
            .margin_top(12)
            .margin_bottom(24)
            .build();

        // Friendly Name section (HIG: use EntryRow inside PreferencesGroup)
        let name_group = adw::PreferencesGroup::builder()
            .title("Friendly Name")
            .build();
        
        let name_entry = adw::EntryRow::builder()
            .title("Custom Name")
            .text(pwm.friendly_name.as_deref().unwrap_or(""))
            .build();
        name_group.add(&name_entry);
        
        // Enable Save button when friendly name changes (debounced)
        let save_btn_name = save_btn.clone();
        let debounce_name: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
        let debounce_name_clear = debounce_name.clone();
        name_entry.connect_changed(move |_| {
            let mut timer = debounce_name.borrow_mut();
            if let Some(id) = timer.take() {
                id.remove();
            }
            let save_btn_debounce = save_btn_name.clone();
            let debounce_clear = debounce_name_clear.clone();
            *timer = Some(glib::timeout_add_local_once(Duration::from_millis(300), move || {
                // Clear the SourceId since this one-shot timer has now completed
                debounce_clear.borrow_mut().take();
                save_btn_debounce.set_sensitive(true);
            }));
        });
        
        main_box.append(&name_group);

        // PWM Slider section
        let slider_group = adw::PreferencesGroup::builder()
            .title("PWM Control")
            .description("Adjust to test which fan responds")
            .build();

        let slider = Scale::builder()
            .orientation(Orientation::Horizontal)
            .hexpand(true)
            .draw_value(false)
            .build();
        slider.set_range(0.0, 255.0);
        // Restore last user-set value, or default to 30% (77 PWM) for testing
        let initial_pwm = pwm.manual_pwm.unwrap_or(77);
        slider.set_value(initial_pwm as f64);
        slider.set_increments(1.0, 25.0);

        // Add marks at exact percentages (255 * percentage)
        slider.add_mark(0.0, gtk4::PositionType::Bottom, Some("0%"));
        slider.add_mark(63.75, gtk4::PositionType::Bottom, Some("25%"));
        slider.add_mark(127.5, gtk4::PositionType::Bottom, Some("50%"));
        slider.add_mark(191.25, gtk4::PositionType::Bottom, Some("75%"));
        slider.add_mark(255.0, gtk4::PositionType::Bottom, Some("100%"));

        // HIG: Use ActionRow with slider for consistent styling
        let slider_row = adw::ActionRow::builder()
            .title("PWM Value")
            .build();
        
        slider.set_valign(gtk4::Align::Center);
        slider.set_size_request(200, -1);
        slider_row.add_suffix(&slider);
        slider_group.add(&slider_row);
        
        main_box.append(&slider_group);
        
        // Fan RPM List section
        let fan_group = adw::PreferencesGroup::builder()
            .title("Fan Sensors")
            .description("Select a fan to pair - responding fans highlight green")
            .build();

        // Store references to RPM labels for live updates
        let rpm_labels: Rc<RefCell<Vec<(String, Label)>>> = Rc::new(RefCell::new(Vec::new()));
        let baseline_rpms: Rc<RefCell<Vec<(String, Option<u32>)>>> = Rc::new(RefCell::new(Vec::new()));
        
        // Track selected fan for Save button: (fan_uuid, fan_path, fan_name)
        let selected_fan: Rc<RefCell<(Option<String>, Option<String>, Option<String>)>> = Rc::new(RefCell::new((
            pwm.fan_uuid.clone(),
            pwm.fan_path.clone(),
            pwm.fan_name.clone(),
        )));

        // Add "None" option
        let none_row = adw::ActionRow::builder()
            .title("None (unpaired)")
            .activatable(true)
            .build();
        
        let none_check = gtk4::CheckButton::new();
        none_check.set_group(None::<&gtk4::CheckButton>);
        if pwm.fan_path.is_none() {
            none_check.set_active(true);
        }
        none_row.add_prefix(&none_check);
        none_row.set_activatable_widget(Some(&none_check));
        
        // Update selection tracker when None is selected
        let selected_fan_none = selected_fan.clone();
        let save_btn_none = save_btn.clone();
        none_check.connect_toggled(move |btn| {
            if btn.is_active() {
                *selected_fan_none.borrow_mut() = (None, None, None);
                save_btn_none.set_sensitive(true);
            }
        });
        
        fan_group.add(&none_row);

        // Add fan sensor rows
        let state_ref = state.borrow();
        let first_check = none_check.clone();
        
        // Check if there are no fan sensors
        if state_ref.fan_sensors.is_empty() {
            let empty_row = adw::ActionRow::builder()
                .title("No fan sensors detected")
                .subtitle("Your hardware may not expose fan RPM sensors, or they are not readable.")
                .sensitive(false)
                .build();
            empty_row.add_prefix(&gtk4::Image::builder()
                .icon_name("dialog-information-symbolic")
                .build());
            fan_group.add(&empty_row);
        }
        
        for fan in &state_ref.fan_sensors {
            let fan_row = adw::ActionRow::builder()
                .activatable(true)
                .build();

            let display_name = fan.label.as_ref()
                .map(|l| l.clone())
                .unwrap_or_else(|| fan.name.clone());
            fan_row.set_title(&display_name);

            // RPM label (will be updated live)
            let rpm_text = fan.rpm
                .map(|r| format!("{} RPM", r))
                .unwrap_or_else(|| "-- RPM".to_string());
            
            // RPM label with visual indicator for responding fans
            let rpm_box = gtk4::Box::builder()
                .orientation(Orientation::Horizontal)
                .spacing(6)
                .build();
            
            let rpm_label = Label::builder()
                .label(&rpm_text)
                .css_classes(["dim-label", "numeric"])
                .build();
            
            // Icon that appears when fan is responding (accessibility)
            let response_icon = gtk4::Image::builder()
                .icon_name("emblem-ok-symbolic")
                .visible(false)
                .css_classes(["success"])
                .build();
            
            rpm_box.append(&response_icon);
            rpm_box.append(&rpm_label);
            fan_row.add_suffix(&rpm_box);
            
            // Highlight row if this is the currently paired fan
            if pwm.fan_path.as_ref() == Some(&fan.path) {
                fan_row.add_css_class("card");
                fan_row.add_css_class("activatable");
            }

            // Store for live updates (baseline will be captured after high PWM is set)
            // Store label, icon, and row for responding state
            rpm_labels.borrow_mut().push((fan.path.clone(), rpm_label.clone()));
            baseline_rpms.borrow_mut().push((fan.path.clone(), None));

            // Radio button for selection
            let check = gtk4::CheckButton::new();
            check.set_group(Some(&first_check));
            if pwm.fan_path.as_ref() == Some(&fan.path) {
                check.set_active(true);
            }
            fan_row.add_prefix(&check);
            fan_row.set_activatable_widget(Some(&check));
            
            // Update selection tracker when this fan is selected
            let selected_fan_row = selected_fan.clone();
            let fan_uuid_row = fan.uuid.clone();
            let fan_path_row = fan.path.clone();
            let fan_name_row = display_name.clone();
            let save_btn_fan = save_btn.clone();
            check.connect_toggled(move |btn| {
                if btn.is_active() {
                    *selected_fan_row.borrow_mut() = (Some(fan_uuid_row.clone()), Some(fan_path_row.clone()), Some(fan_name_row.clone()));
                    save_btn_fan.set_sensitive(true);
                }
            });

            fan_group.add(&fan_row);
        }
        drop(state_ref);

        main_box.append(&fan_group);
        content.append(&main_box);

        dialog.set_content(Some(&content));

        // Set initial PWM when dialog opens (restore last user value or default to 30%)
        // PERFORMANCE: Run on thread pool to avoid blocking UI
        let pwm_path_init = pwm.pwm_path.clone();
        let baseline_rpms_capture = baseline_rpms.clone();
        glib::spawn_future_local(async move {
            // Set initial PWM
            let pwm_path = pwm_path_init.clone();
            let set_result = gio::spawn_blocking(move || {
                daemon_client::daemon_set_pwm_override(&pwm_path, initial_pwm, 10000)
            }).await;
            
            if let Ok(Err(e)) = set_result {
                warn!("Failed to set initial PWM to {}: {}", initial_pwm, e);
            }
            
            // Wait for fans to stabilize before capturing baseline
            glib::timeout_future(Duration::from_millis(500)).await;
            
            // Capture baseline RPMs using batch call
            let hw_result = gio::spawn_blocking(|| {
                daemon_client::daemon_list_hardware()
            }).await;
            
            if let Ok(Ok(hw_data)) = hw_result {
                let mut baselines = baseline_rpms_capture.borrow_mut();
                for (path, rpm_opt) in baselines.iter_mut() {
                    // Find this fan in hardware data
                    for chip in &hw_data.chips {
                        if let Some(fan) = chip.fans.iter().find(|f| &f.path == path) {
                            *rpm_opt = fan.rpm;
                            break;
                        }
                    }
                }
            }
        });

        // Connect PWM slider with debouncing (daemon authoritative)
        let pwm_path = pwm.pwm_path.clone();
        let pwm_uuid_slider = pwm.pwm_uuid.clone();
        let state_slider = state.clone();
        
        // Debounce timer to avoid spamming daemon
        let debounce_timer: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
        
        slider.connect_change_value(move |_s, _scroll_type, value| {
            let value = (value.clamp(0.0, 255.0)) as u8;
            
            // Save manual PWM value to memory
            {
                let mut state_ref = state_slider.borrow_mut();
                if let Some(pwm_data) = state_ref.pwm_controls.iter_mut()
                    .find(|p| p.pwm_uuid == pwm_uuid_slider)
                {
                    pwm_data.manual_pwm = Some(value);
                }
            }
            
            // Debounce daemon calls to avoid spam (100ms delay)
            let mut timer = debounce_timer.borrow_mut();
            if let Some(id) = timer.take() {
                id.remove();
            }
            
            let pwm_path_debounce = pwm_path.clone();
            let debounce_clear = debounce_timer.clone();
            *timer = Some(glib::timeout_add_local_once(Duration::from_millis(50), move || {
                // Clear the SourceId since this one-shot timer has now completed
                debounce_clear.borrow_mut().take();
                // PERFORMANCE: Run PWM set on thread pool to avoid blocking UI
                let pwm_path_async = pwm_path_debounce.clone();
                glib::spawn_future_local(async move {
                    let result = gio::spawn_blocking(move || {
                        daemon_client::daemon_set_pwm_override(&pwm_path_async, value, 2000)
                    }).await;
                    if let Ok(Err(e)) = result {
                        warn!("Failed to set PWM override via daemon: {}", e);
                    }
                });
            }));
            
            glib::Propagation::Proceed
        });

        // Live RPM update timer - use user-configured poll interval
        // PERFORMANCE: Use async polling to avoid blocking the UI thread
        let poll_interval_ms = hf_core::get_cached_settings().general.poll_interval_ms as u64;
        let poll_interval_ms = poll_interval_ms.max(100); // Minimum 100ms for stability
        
        let rpm_labels_timer = rpm_labels.clone();
        let baseline_rpms_timer = baseline_rpms.clone();
        let dialog_weak = glib::SendWeakRef::from(dialog.downgrade());
        
        // Store last known RPM values to avoid showing "--" on transient failures
        let last_rpms: Rc<RefCell<std::collections::HashMap<String, u32>>> = 
            Rc::new(RefCell::new(std::collections::HashMap::new()));
        let last_rpms_timer = last_rpms.clone();
        
        // Track if a fetch is in progress to avoid overlapping requests
        let fetch_in_progress = Rc::new(RefCell::new(false));
        let fetch_in_progress_timer = fetch_in_progress.clone();
        
        glib::timeout_add_local(Duration::from_millis(poll_interval_ms), move || {
            // Check if dialog still exists
            let dialog_ref = match dialog_weak.upgrade() {
                Some(d) => d,
                None => return glib::ControlFlow::Break,
            };
            
            // Skip if a fetch is already in progress (prevents queue buildup)
            if *fetch_in_progress_timer.borrow() {
                return glib::ControlFlow::Continue;
            }
            
            // Mark fetch as in progress
            *fetch_in_progress_timer.borrow_mut() = true;
            
            // Clone data needed for async operation
            let labels_clone = rpm_labels_timer.clone();
            let baselines_clone = baseline_rpms_timer.clone();
            let last_rpms_clone = last_rpms_timer.clone();
            let fetch_done = fetch_in_progress_timer.clone();
            let _dialog_ref = dialog_ref; // Keep dialog alive during fetch
            
            // Spawn async task to fetch RPMs without blocking UI
            glib::spawn_future_local(async move {
                // Run blocking daemon call on thread pool
                let hw_result = gio::spawn_blocking(|| {
                    daemon_client::daemon_list_hardware()
                }).await;
                
                // Mark fetch as complete
                *fetch_done.borrow_mut() = false;
                
                // Process result on main thread
                let hw_data = match hw_result {
                    Ok(Ok(data)) => data,
                    Ok(Err(e)) => {
                        debug!("Failed to fetch hardware data: {}", e);
                        return;
                    }
                    Err(_) => {
                        debug!("Spawn blocking task was cancelled");
                        return;
                    }
                };
                
                // Build path -> RPM map from hardware data
                let mut rpm_map: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
                for chip in &hw_data.chips {
                    for fan in &chip.fans {
                        if let Some(rpm) = fan.rpm {
                            rpm_map.insert(fan.path.clone(), rpm);
                        }
                    }
                }
                
                // Update UI labels
                let labels = labels_clone.borrow();
                let baselines = baselines_clone.borrow();
                let mut last_rpms_map = last_rpms_clone.borrow_mut();
                
                for (path, label) in labels.iter() {
                    let current_rpm = rpm_map.get(path).copied();
                    
                    // Update last known value if we got a reading
                    if let Some(rpm) = current_rpm {
                        last_rpms_map.insert(path.clone(), rpm);
                    }
                    
                    // Use current reading, or fall back to last known value
                    let display_rpm = current_rpm.or_else(|| last_rpms_map.get(path).copied());

                    // Find baseline for this fan
                    let baseline = baselines.iter()
                        .find(|(p, _)| p == path)
                        .and_then(|(_, rpm)| *rpm);

                    // Update label
                    let rpm_text = display_rpm
                        .map(|r| format!("{} RPM", r))
                        .unwrap_or_else(|| "-- RPM".to_string());
                    label.set_label(&rpm_text);

                    // Check if RPM changed significantly (responding to PWM)
                    let is_responding = match (display_rpm, baseline) {
                        (Some(curr), Some(base)) => {
                            let diff = (curr as i32 - base as i32).abs();
                            let abs_threshold = if base > 2000 { 100 } else { 50 };
                            let rel_threshold = if base > 0 { (diff as f32 / base as f32) > 0.10 } else { false };
                            diff > abs_threshold || rel_threshold
                        }
                        _ => false,
                    };

                    // Apply/remove accent color highlighting
                    let currently_highlighted = label.has_css_class("accent");
                    
                    if is_responding && !currently_highlighted {
                        label.remove_css_class("dim-label");
                        label.add_css_class("accent");
                        label.add_css_class("fan-responding");
                    } else if !is_responding && currently_highlighted {
                        label.remove_css_class("accent");
                        label.remove_css_class("fan-responding");
                        label.add_css_class("dim-label");
                    }
                }
            });

            glib::ControlFlow::Continue
        });

        // Connect Cancel button - clear PWM override when closing
        let dialog_cancel = dialog.clone();
        let pwm_path_cancel = pwm.pwm_path.clone();
        cancel_btn.connect_clicked(move |_| {
            // Clear PWM override so daemon resumes automatic control
            if let Err(e) = daemon_client::daemon_clear_pwm_override(&pwm_path_cancel) {
                warn!("Failed to clear PWM override on cancel: {}", e);
            }
            dialog_cancel.close();
        });

        // Connect Save button - use tracked selection
        let dialog_save = dialog.clone();
        let pwm_uuid_save = pwm.pwm_uuid.clone();
        let pwm_path_save = pwm.pwm_path.clone();
        let selected_fan_save = selected_fan.clone();
        let state_save = state.clone();
        let pwm_list_save = pwm_list.cloned();
        save_btn.connect_clicked(move |_| {
            // Get friendly name
            let friendly = name_entry.text().to_string();
            let friendly_name = if friendly.trim().is_empty() { None } else { Some(friendly.clone()) };
            
            // Get selected fan from tracker: (fan_uuid, fan_path, fan_name)
            let (selected_fan_uuid, selected_fan_path, selected_fan_name) = selected_fan_save.borrow().clone();
            
            // Save the pairing with UUIDs
            if let Err(e) = Self::save_pairing(
                &pwm_uuid_save,
                &pwm_path_save,
                selected_fan_uuid.as_deref(),
                selected_fan_path.as_deref(),
                selected_fan_name.as_deref(),
                friendly_name.as_deref(),
            ) {
                warn!("Failed to save pairing: {}", e);
            } else {
                // Update in-memory state
                {
                    let mut state_ref = state_save.borrow_mut();
                    if let Some(pwm) = state_ref.pwm_controls.iter_mut()
                        .find(|p| p.pwm_uuid == pwm_uuid_save || p.pwm_path == pwm_path_save)
                    {
                        pwm.fan_uuid = selected_fan_uuid.clone();
                        pwm.fan_path = selected_fan_path.clone();
                        pwm.fan_name = selected_fan_name.clone();
                        pwm.friendly_name = friendly_name.clone();
                    }
                }
                
                // List will be rebuilt on dialog close to show manual PWM value
                
                // Show success toast - find ToastOverlay in widget hierarchy
                if let Some(root) = dialog_save.root() {
                    let toast = adw::Toast::builder()
                        .title("Fan pairing saved")
                        .timeout(2)
                        .build();
                    
                    // Try ApplicationWindow first
                    if let Ok(app_window) = root.clone().downcast::<adw::ApplicationWindow>() {
                        if let Some(overlay) = app_window.content().and_then(|c| c.downcast::<adw::ToastOverlay>().ok()) {
                            overlay.add_toast(toast);
                        }
                    } else if let Ok(window) = root.downcast::<adw::Window>() {
                        // For adw::Window, traverse to find ToastOverlay
                        if let Some(content) = window.content() {
                            if let Ok(overlay) = content.downcast::<adw::ToastOverlay>() {
                                overlay.add_toast(toast);
                            }
                        }
                    }
                }
            }
            
            // Clear PWM override so daemon resumes automatic control
            if let Err(e) = daemon_client::daemon_clear_pwm_override(&pwm_path_save) {
                warn!("Failed to clear PWM override on save: {}", e);
            }
            
            dialog_save.close();
        });

        // CRITICAL: Ensure PWM override is cleared when dialog closes by ANY means
        // (Escape key, clicking outside, window manager close, etc.)
        let pwm_path_close = pwm.pwm_path.clone();
        let pwm_list_close = pwm_list.cloned();
        let state_close = state.clone();
        dialog.connect_close_request(move |_| {
            // Clear PWM override so daemon IMMEDIATELY resumes automatic control
            if let Err(e) = daemon_client::daemon_clear_pwm_override(&pwm_path_close) {
                warn!("Failed to clear PWM override on dialog close: {}", e);
            }
            
            // Rebuild list to show manual PWM value in subtitle
            if let Some(ref list) = pwm_list_close {
                Self::rebuild_list_static(&state_close, list);
            }
            
            glib::Propagation::Proceed
        });
        
        // Add keyboard shortcuts
        let key_controller = gtk4::EventControllerKey::new();
        let dialog_key = dialog.clone();
        let save_btn_key = save_btn.clone();
        key_controller.connect_key_pressed(move |_, key, _, _| {
            match key {
                gtk4::gdk::Key::Escape => {
                    dialog_key.close();
                    glib::Propagation::Stop
                }
                gtk4::gdk::Key::Return | gtk4::gdk::Key::KP_Enter => {
                    if save_btn_key.is_sensitive() {
                        save_btn_key.activate();
                    }
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed
            }
        });
        dialog.add_controller(key_controller);

        // Present the dialog
        dialog.present();
    }

    fn show_autodetect_dialog(
        window: Option<&gtk4::Window>,
        state: &Rc<RefCell<FanPairingState>>,
        pwm_list: &ListBox,
    ) {
        let dialog = adw::Window::builder()
            .title("Auto Detect Fan Mappings")
            .default_width(500)
            .default_height(400)
            .modal(true)
            .build();

        if let Some(win) = window {
            dialog.set_transient_for(Some(win));
        }

        let content = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(0)
            .build();

        // Header bar (HIG: action buttons in header)
        let header = adw::HeaderBar::builder()
            .show_end_title_buttons(false)
            .build();
        
        let cancel_btn = Button::builder()
            .label("Cancel")
            .build();
        header.pack_start(&cancel_btn);
        
        let start_btn = Button::builder()
            .label("Start")
            .css_classes(["suggested-action"])
            .build();
        header.pack_end(&start_btn);
        
        content.append(&header);

        // Main content with proper spacing
        let main_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(24)
            .margin_start(24)
            .margin_end(24)
            .margin_top(12)
            .margin_bottom(24)
            .vexpand(true)
            .build();

        // Status/instructions group
        let status_group = adw::PreferencesGroup::builder()
            .description("This will cycle through PWM values to identify which fans respond to each controller.")
            .build();
        
        let progress_bar = gtk4::ProgressBar::builder()
            .show_text(true)
            .text("Ready to scan")
            .build();
        
        let progress_row = adw::ActionRow::builder()
            .title("Progress")
            .build();
        progress_row.add_suffix(&progress_bar);
        progress_bar.set_hexpand(true);
        progress_bar.set_valign(gtk4::Align::Center);
        status_group.add(&progress_row);
        
        let current_test_label = Label::builder()
            .label("Click Start to begin")
            .css_classes(["dim-label"])
            .halign(gtk4::Align::Start)
            .build();
        
        main_box.append(&status_group);
        main_box.append(&current_test_label);

        // Results list
        let results_group = adw::PreferencesGroup::builder()
            .title("Detected Mappings")
            .build();
        
        let results_list = ListBox::builder()
            .selection_mode(SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();
        
        // Empty state placeholder
        let empty_row = adw::ActionRow::builder()
            .title("No mappings detected yet")
            .css_classes(["dim-label"])
            .build();
        results_list.append(&empty_row);
        
        results_group.add(&results_list);
        main_box.append(&results_group);

        content.append(&main_box);
        dialog.set_content(Some(&content));

        // Track if detection is running
        let is_running: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));
        let timer_id: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
        
        // Cancel button stops detection and closes dialog
        let dialog_cancel = dialog.clone();
        let is_running_cancel = is_running.clone();
        let timer_id_cancel = timer_id.clone();
        cancel_btn.connect_clicked(move |_| {
            *is_running_cancel.borrow_mut() = false;
            if let Some(id) = timer_id_cancel.borrow_mut().take() {
                id.remove();
            }
            dialog_cancel.close();
        });

        // Start detection
        let dialog_ref = dialog.clone();
        let state_ref = state.clone();
        let pwm_list_ref = pwm_list.clone();
        start_btn.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            btn.set_label("Detecting...");
            
            let progress = progress_bar.clone();
            let test_label = current_test_label.clone();
            let results = results_list.clone();
            let state_clone = state_ref.clone();
            let pwm_list_clone = pwm_list_ref.clone();
            let _dialog_done = dialog_ref.clone();
            
            // Get PWM and fan data
            let state_borrow = state_clone.borrow();
            let pwm_controls: Vec<_> = state_borrow.pwm_controls.clone();
            let fan_sensors: Vec<_> = state_borrow.fan_sensors.clone();
            drop(state_borrow);
            
            let total_pwms = pwm_controls.len();
            if total_pwms == 0 {
                test_label.set_label("No PWM controls found");
                btn.set_label("No PWMs");
                return;
            }
            
            // Store detected mappings: (pwm_uuid, pwm_path, fan_uuid, fan_path, fan_name, pwm_name)
            let detected: Rc<RefCell<Vec<(String, String, String, String, String, String)>>> = Rc::new(RefCell::new(Vec::new()));
            
            // Run detection asynchronously
            *is_running.borrow_mut() = true;
            let is_running_timer = is_running.clone();
            let pwm_idx = Rc::new(RefCell::new(0usize));
            let phase = Rc::new(RefCell::new(0u8)); // 0=set high, 1=wait, 2=read baseline, 3=set low, 4=wait, 5=read result
            let baseline_rpms: Rc<RefCell<std::collections::HashMap<String, u32>>> = Rc::new(RefCell::new(std::collections::HashMap::new()));
            
            let pwm_idx_timer = pwm_idx.clone();
            let phase_timer = phase.clone();
            let detected_timer = detected.clone();
            let pwm_controls_timer = pwm_controls.clone();
            let fan_sensors_timer = fan_sensors.clone();
            let baseline_rpms_timer = baseline_rpms.clone();
            let timer_id_clear = timer_id.clone();
            
            let source_id = glib::timeout_add_local(Duration::from_millis(500), move || {
                // Check if cancelled
                if !*is_running_timer.borrow() {
                    // Clear the SourceId since the timer is stopping
                    timer_id_clear.borrow_mut().take();
                    return glib::ControlFlow::Break;
                }
                let idx = *pwm_idx_timer.borrow();
                let current_phase = *phase_timer.borrow();
                
                if idx >= pwm_controls_timer.len() {
                    // Done - show results
                    progress.set_fraction(1.0);
                    progress.set_text(Some("Complete"));
                    test_label.set_label("Detection complete!");
                    
                    // Save detected mappings
                    let detected_ref = detected_timer.borrow();
                    for (pwm_uuid, pwm_path, fan_uuid, fan_path, fan_name, _pwm_name) in detected_ref.iter() {
                        let _ = Self::save_pairing(
                            pwm_uuid,
                            pwm_path,
                            Some(fan_uuid.as_str()),
                            Some(fan_path.as_str()),
                            Some(fan_name.as_str()),
                            None,
                        );
                    }
                    
                    // Reload state and rebuild list
                    if let Ok(settings) = hf_core::load_settings() {
                        let mut state_mut = state_clone.borrow_mut();
                        for pairing in &settings.pwm_fan_pairings {
                            // Match by UUID first (stable), then path (fallback) - same as load_hardware_data
                            if let Some(pwm) = state_mut.pwm_controls.iter_mut()
                                .find(|p| pairing.pwm_uuid.as_ref().map_or(false, |uuid| &p.pwm_uuid == uuid)
                                    || p.pwm_path == pairing.pwm_path)
                            {
                                pwm.fan_uuid = pairing.fan_uuid.clone();
                                pwm.fan_path = pairing.fan_path.clone();
                                pwm.fan_name = pairing.fan_name.clone();
                                pwm.friendly_name = pairing.friendly_name.clone();
                            }
                        }
                    }
                    Self::rebuild_list_static(&state_clone, &pwm_list_clone);
                    
                    // Clear the SourceId since the timer is completing
                    timer_id_clear.borrow_mut().take();
                    return glib::ControlFlow::Break;
                }
                
                let pwm = &pwm_controls_timer[idx];
                let pwm_path = &pwm.pwm_path;
                let frac = (idx as f64 + current_phase as f64 / 4.0) / total_pwms as f64;
                progress.set_fraction(frac);
                
                match current_phase {
                    0 => {
                        // Set to high PWM via daemon
                        test_label.set_label(&format!("Testing: {} - Setting high PWM", pwm.pwm_name));
                        if let Err(e) = daemon_client::daemon_set_pwm_override(pwm_path, 255, 10000) {
                            warn!("Failed to set PWM override via daemon: {}", e);
                        }
                        *phase_timer.borrow_mut() = 1;
                    }
                    1 => {
                        // Wait for fans to stabilize
                        test_label.set_label(&format!("Testing: {} - Waiting for fans to stabilize", pwm.pwm_name));
                        *phase_timer.borrow_mut() = 2;
                    }
                    2 => {
                        // Read baseline RPMs (high PWM)
                        test_label.set_label(&format!("Testing: {} - Reading baseline RPMs", pwm.pwm_name));
                        if let Ok(hw_data) = daemon_client::daemon_list_hardware() {
                            for fan in &fan_sensors_timer {
                                if let Some(rpm) = hw_data.chips.iter()
                                    .flat_map(|chip| chip.fans.iter())
                                    .find(|f| f.path == fan.path)
                                    .and_then(|f| f.rpm)
                                {
                                    baseline_rpms_timer.borrow_mut().insert(fan.path.clone(), rpm);
                                }
                            }
                        }
                        *phase_timer.borrow_mut() = 3;
                    }
                    3 => {
                        // Set to low PWM
                        test_label.set_label(&format!("Testing: {} - Setting low PWM", pwm.pwm_name));
                        if let Err(e) = daemon_client::daemon_set_pwm_override(pwm_path, 30, 10000) {
                            warn!("Failed to set PWM override via daemon: {}", e);
                        }
                        *phase_timer.borrow_mut() = 4;
                    }
                    4 => {
                        // Wait for fans to slow down
                        test_label.set_label(&format!("Testing: {} - Waiting for fan response", pwm.pwm_name));
                        *phase_timer.borrow_mut() = 5;
                    }
                    5 => {
                        // Check which fans changed significantly
                        test_label.set_label(&format!("Testing: {} - Checking fan response", pwm.pwm_name));
                        
                        // Remove empty placeholder on first detection
                        if detected_timer.borrow().is_empty() {
                            while let Some(child) = results.first_child() {
                                results.remove(&child);
                            }
                        }
                        
                        if let Ok(hw_data) = daemon_client::daemon_list_hardware() {
                            let baselines = baseline_rpms_timer.borrow();
                            
                            for fan in &fan_sensors_timer {
                                if let Some(current_rpm) = hw_data.chips.iter()
                                    .flat_map(|chip| chip.fans.iter())
                                    .find(|f| f.path == fan.path)
                                    .and_then(|f| f.rpm)
                                {
                                    if let Some(&baseline_rpm) = baselines.get(&fan.path) {
                                        // Check if RPM dropped significantly
                                        let drop = (baseline_rpm as i32 - current_rpm as i32).max(0) as u32;
                                        let drop_percent = if baseline_rpm > 0 {
                                            (drop as f32 / baseline_rpm as f32) * 100.0
                                        } else {
                                            0.0
                                        };
                                        
                                        // Detect if drop > 200 RPM OR > 20%
                                        if drop > 200 || drop_percent > 20.0 {
                                            let fan_name = fan.label.clone().unwrap_or_else(|| fan.name.clone());
                                            detected_timer.borrow_mut().push((
                                                pwm.pwm_uuid.clone(),
                                                pwm_path.clone(),
                                                fan.uuid.clone(),
                                                fan.path.clone(),
                                                fan_name.clone(),
                                                pwm.pwm_name.clone(),
                                            ));
                                            
                                            // Add to results list
                                            let row = adw::ActionRow::builder()
                                                .title(&format!("{} → {}", pwm.pwm_name, fan_name))
                                                .subtitle(&format!("RPM: {} → {} (-{}%)", baseline_rpm, current_rpm, drop_percent as u32))
                                                .build();
                                            row.add_prefix(&gtk4::Image::builder()
                                                .icon_name("emblem-ok-symbolic")
                                                .css_classes(["success"])
                                                .build());
                                            results.append(&row);
                                            break; // One fan per PWM
                                        }
                                    }
                                }
                            }
                        }
                        
                        // Clear PWM override
                        let _ = daemon_client::daemon_clear_pwm_override(pwm_path);

                        // Move to next PWM
                        *pwm_idx_timer.borrow_mut() = idx + 1;
                        *phase_timer.borrow_mut() = 0;
                    }
                    _ => {}
                }
                
                glib::ControlFlow::Continue
            });
            
            // Store timer ID so cancel can stop it
            *timer_id.borrow_mut() = Some(source_id);
        });

        dialog.present();
    }

    fn save_pairing(
        pwm_uuid: &str,
        pwm_path: &str,
        fan_uuid: Option<&str>,
        fan_path: Option<&str>,
        fan_name: Option<&str>,
        friendly_name: Option<&str>,
    ) -> Result<(), String> {
        let mut settings = hf_core::load_settings()
            .map_err(|e| format!("Failed to load settings: {}", e))?;

        // Remove existing pairing for this PWM (by UUID first, then path)
        settings.pwm_fan_pairings.retain(|p| {
            p.pwm_uuid.as_deref() != Some(pwm_uuid) && p.pwm_path != pwm_path
        });

        // Extract hardware identification (CRITICAL for safe pairing across reboots)
        let pwm_hw = hf_core::extract_pwm_hardware_id(pwm_path);
        let fan_hw = fan_path.map(hf_core::extract_fan_hardware_id);
        
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .ok();

        // Always add a pairing entry with full hardware identification
        settings.pwm_fan_pairings.push(hf_core::PwmFanPairing {
            id: hf_core::generate_guid(),
            pwm_uuid: Some(pwm_uuid.to_string()),
            pwm_path: pwm_path.to_string(),
            fan_uuid: fan_uuid.map(String::from),
            fan_path: fan_path.map(String::from),
            fan_name: fan_name.map(String::from),
            friendly_name: friendly_name.map(String::from),
            // Hardware identification fields
            driver_name: pwm_hw.driver_name,
            device_path: pwm_hw.device_path,
            pwm_index: pwm_hw.pwm_index,
            fan_index: fan_hw.as_ref().and_then(|f| f.fan_index),
            pwm_label: pwm_hw.pwm_label,
            fan_label: fan_hw.as_ref().and_then(|f| f.fan_label.clone()),
            pci_address: pwm_hw.pci_address,
            pci_vendor_id: pwm_hw.pci_vendor_id,
            pci_device_id: pwm_hw.pci_device_id,
            modalias: pwm_hw.modalias,
            created_at: now,
            last_validated_at: now,
            validated_this_session: true,
            // GPU-specific fields
            gpu_vendor: pwm_hw.gpu_vendor,
            gpu_index: pwm_hw.gpu_index,
            gpu_fan_index: pwm_hw.gpu_fan_index,
            gpu_name: pwm_hw.gpu_name,
            gpu_controller_id: pwm_hw.gpu_controller_id,
            drm_card_number: pwm_hw.drm_card_number,
        });

        hf_core::save_settings(&settings)
            .map_err(|e| format!("Failed to save settings: {}", e))?;

        // Signal daemon to reload config
        if let Err(e) = hf_core::daemon_reload_config() {
            debug!("Failed to signal daemon reload: {}", e);
        }

        debug!(
            "Saved PWM-fan pairing with hardware ID: {} (driver: {:?}, device: {:?}) -> {:?}",
            pwm_path,
            settings.pwm_fan_pairings.last().and_then(|p| p.driver_name.as_ref()),
            settings.pwm_fan_pairings.last().and_then(|p| p.device_path.as_ref()),
            fan_path
        );
        Ok(())
    }

    pub fn widget(&self) -> &GtkBox {
        &self.container
    }
}

impl Default for FanPairingPage {
    fn default() -> Self {
        Self::new()
    }
}
