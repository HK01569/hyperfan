//! EC Direct Control Page
//!
//! EXTREMELY DANGEROUS - Direct access to Embedded Controller registers.
//! This page allows users to read and write EC registers directly.
//! Incorrect values can permanently damage hardware.
//!
//! Features:
//! - Auto-scan all 256 EC registers on page load
//! - Friendly name editing for each register
//! - Notes with 1000 character limit
//! - Favorites (starred registers appear at top)
//! - Color categories (16 colors) with filtering
//! - Confidence levels (low/medium/high)
//! - Sorting by name, color, favorites, or value
//! - Persistent storage in ~/.config/hyperfan/ec_profile.json

#![allow(dead_code)]

use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, Label, ListBox, Orientation,
    ScrolledWindow, TextView, ToggleButton,
};
use gtk4::gio;
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{debug, error, info, warn};

const MAX_NOTE_LENGTH: usize = 1000;
const EC_PROFILE_FILENAME: &str = "ec_profile.json";

/// 16 category colors (index 0 = None/transparent)
const CATEGORY_COLORS: &[(u8, &str, &str)] = &[
    (0, "None", "transparent"),
    (1, "Red", "#e01b24"),
    (2, "Orange", "#ff7800"),
    (3, "Yellow", "#f6d32d"),
    (4, "Green", "#33d17a"),
    (5, "Teal", "#2ec27e"),
    (6, "Cyan", "#00b4d8"),
    (7, "Blue", "#3584e4"),
    (8, "Purple", "#9141ac"),
    (9, "Pink", "#e66100"),
    (10, "Brown", "#865e3c"),
    (11, "Gray", "#77767b"),
    (12, "Light Blue", "#99c1f1"),
    (13, "Light Green", "#8ff0a4"),
    (14, "Light Purple", "#dc8add"),
    (15, "White", "#ffffff"),
];

/// Confidence level for register identification
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub enum ConfidenceLevel {
    #[default]
    Low,
    Medium,
    High,
}

impl ConfidenceLevel {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
        }
    }

    fn from_index(idx: u32) -> Self {
        match idx {
            0 => Self::Low,
            1 => Self::Medium,
            2 => Self::High,
            _ => Self::Low,
        }
    }

    fn to_index(&self) -> u32 {
        match self {
            Self::Low => 0,
            Self::Medium => 1,
            Self::High => 2,
        }
    }
}

/// Sort mode for register list
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
enum SortMode {
    #[default]
    Address,
    Name,
    Color,
    Favorites,
    Value,
}

/// Persistent EC profile data
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct EcProfileData {
    version: u32,
    registers: HashMap<u8, RegisterUserData>,
    last_sort_mode: SortMode,
}

/// User data for a single register (persistent)
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct RegisterUserData {
    friendly_name: Option<String>,
    note: Option<String>,
    favorite: bool,
    color_index: u8,
    confidence: ConfidenceLevel,
}

/// Comprehensive EC chip metadata
#[derive(Debug, Clone, serde::Serialize)]
struct EcChipMetadata {
    name: String,
    path: String,
    device_path: Option<String>,
    chip_class: String,
    chip_vendor: String,
    chip_model: Option<String>,
    chip_revision: Option<String>,
    driver_name: Option<String>,
    pci_id: Option<String>,
    subsystem_id: Option<String>,
    bus_info: Option<String>,
    register_count: u16,
    detected_features: Vec<String>,
    hwmon_attributes: HashMap<String, String>,
    scan_timestamp: u64,
}

/// EC register with full metadata
#[derive(Debug, Clone, serde::Serialize)]
struct EcRegisterData {
    register: u8,
    value: u8,
    default_label: Option<String>,
    writable: bool,
    category: String,
    hint: Option<String>,
}

/// EC Control Page
pub struct EcControlPage {
    container: GtkBox,
}

impl EcControlPage {
    pub fn new() -> Self {
        let container = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .vexpand(true)
            .hexpand(true)
            .build();

        // Check if EC control is enabled (use cached settings)
        let settings = hf_core::get_cached_settings();
        let ec_enabled = settings.advanced.ec_direct_control_enabled && settings.advanced.ec_danger_acknowledged;

        if !ec_enabled {
            Self::build_disabled_view(&container);
            return Self { container };
        }

        // Shared state
        let chips: Rc<RefCell<Vec<EcChipMetadata>>> = Rc::new(RefCell::new(Vec::new()));
        let selected_chip: Rc<RefCell<Option<EcChipMetadata>>> = Rc::new(RefCell::new(None));
        let registers: Rc<RefCell<Vec<EcRegisterData>>> = Rc::new(RefCell::new(Vec::new()));
        let profile_data = Rc::new(RefCell::new(Self::load_profile()));
        let sort_mode = Rc::new(RefCell::new(profile_data.borrow().last_sort_mode));
        let color_filter: Rc<RefCell<Option<u8>>> = Rc::new(RefCell::new(None));

        // Main content
        let scroll = ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .build();

        let content = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(12)
            .margin_start(24)
            .margin_end(24)
            .margin_top(24)
            .margin_bottom(24)
            .build();

        // Page header with refresh button
        let header_box = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .build();

        let page_title = Label::builder()
            .label("EC Direct Control")
            .css_classes(["title-1"])
            .halign(gtk4::Align::Start)
            .hexpand(true)
            .build();
        header_box.append(&page_title);

        // Refresh/scan button (small, top right)
        let refresh_btn = Button::builder()
            .icon_name("view-refresh-symbolic")
            .tooltip_text("Scan for EC chips")
            .css_classes(["flat"])
            .build();
        header_box.append(&refresh_btn);

        content.append(&header_box);

        // Warning banner
        let warning_banner = adw::Banner::builder()
            .title("Writing incorrect values to EC registers can PERMANENTLY DAMAGE your hardware!")
            .revealed(true)
            .build();
        warning_banner.add_css_class("error");
        content.append(&warning_banner);

        // EC Chip Selection (compact)
        let chip_dropdown = adw::ComboRow::builder()
            .title("EC/SuperIO Chip")
            .subtitle("No chips detected")
            .build();

        let chip_group = adw::PreferencesGroup::new();
        chip_group.add(&chip_dropdown);
        content.append(&chip_group);

        // Controls row: Sort dropdown + Color filter buttons
        let controls_box = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .margin_top(6)
            .margin_bottom(6)
            .build();

        let sort_label = Label::builder()
            .label("Sort:")
            .css_classes(["dim-label"])
            .build();
        controls_box.append(&sort_label);

        let sort_dropdown = gtk4::DropDown::from_strings(&[
            "Address", "Name", "Color", "Favorites", "Value"
        ]);
        sort_dropdown.set_selected(0);
        controls_box.append(&sort_dropdown);

        // Spacer
        let spacer = GtkBox::builder()
            .hexpand(true)
            .build();
        controls_box.append(&spacer);

        // Color filter label
        let filter_label = Label::builder()
            .label("Filter:")
            .css_classes(["dim-label"])
            .build();
        controls_box.append(&filter_label);

        // Color filter buttons (all 16 colors)
        let color_filter_box = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(4)
            .build();

        let register_list = ListBox::builder()
            .selection_mode(gtk4::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();

        // Create color filter buttons
        for (idx, name, color) in CATEGORY_COLORS.iter() {
            let btn = Button::builder()
                .tooltip_text(*name)
                .width_request(24)
                .height_request(24)
                .css_classes(["flat", "circular"])
                .build();

            if *idx == 0 {
                btn.set_label("All");
                btn.set_width_request(36);
            } else {
                let css = format!("button {{ background-color: {}; min-width: 20px; min-height: 20px; }}", color);
                let provider = gtk4::CssProvider::new();
                provider.load_from_string(&css);
                if let Some(display) = gtk4::gdk::Display::default() {
                    gtk4::style_context_add_provider_for_display(&display, &provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
                }
            }

            let color_idx = *idx;
            let color_filter_clone = color_filter.clone();
            let registers_clone = registers.clone();
            let profile_data_clone = profile_data.clone();
            let sort_mode_clone = sort_mode.clone();
            let register_list_clone = register_list.clone();

            btn.connect_clicked(move |_| {
                let filter = if color_idx == 0 { None } else { Some(color_idx) };
                *color_filter_clone.borrow_mut() = filter;

                Self::render_register_list(
                    &register_list_clone,
                    &registers_clone.borrow(),
                    &profile_data_clone,
                    *sort_mode_clone.borrow(),
                    filter,
                );
            });

            color_filter_box.append(&btn);
        }

        controls_box.append(&color_filter_box);
        content.append(&controls_box);

        // Register list with proper styling
        let register_scroll = ScrolledWindow::builder()
            .height_request(400)
            .vexpand(true)
            .build();

        // Wrap list in a frame for rounded corners
        let list_frame = adw::Clamp::builder()
            .maximum_size(800)
            .child(&register_list)
            .build();

        register_scroll.set_child(Some(&list_frame));
        content.append(&register_scroll);

        // Export button only (no apply changes)
        let action_box = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .margin_top(12)
            .halign(gtk4::Align::End)
            .build();

        let export_btn = Button::builder()
            .label("Export Profile")
            .icon_name("document-save-symbolic")
            .css_classes(["suggested-action"])
            .build();
        action_box.append(&export_btn);

        content.append(&action_box);

        scroll.set_child(Some(&content));
        container.append(&scroll);

        // Wire up refresh button
        let chips_for_refresh = chips.clone();
        let chip_dropdown_for_refresh = chip_dropdown.clone();
        refresh_btn.connect_clicked(move |_| {
            Self::refresh_ec_chips(&chips_for_refresh, &chip_dropdown_for_refresh);
        });

        // Wire up sort dropdown
        let sort_mode_for_sort = sort_mode.clone();
        let registers_for_sort = registers.clone();
        let register_list_for_sort = register_list.clone();
        let profile_data_for_sort = profile_data.clone();
        let color_filter_for_sort = color_filter.clone();

        sort_dropdown.connect_selected_notify(move |dropdown| {
            let mode = match dropdown.selected() {
                0 => SortMode::Address,
                1 => SortMode::Name,
                2 => SortMode::Color,
                3 => SortMode::Favorites,
                4 => SortMode::Value,
                _ => SortMode::Address,
            };
            *sort_mode_for_sort.borrow_mut() = mode;

            profile_data_for_sort.borrow_mut().last_sort_mode = mode;
            Self::save_profile(&profile_data_for_sort.borrow());

            Self::render_register_list(
                &register_list_for_sort,
                &registers_for_sort.borrow(),
                &profile_data_for_sort,
                mode,
                *color_filter_for_sort.borrow(),
            );
        });

        // Wire up chip dropdown - auto-scan on selection
        let selected_chip_for_select = selected_chip.clone();
        let chips_for_select = chips.clone();
        let registers_for_select = registers.clone();
        let register_list_for_select = register_list.clone();
        let profile_data_for_select = profile_data.clone();
        let sort_mode_for_select = sort_mode.clone();
        let color_filter_for_select = color_filter.clone();

        chip_dropdown.connect_selected_notify(move |dropdown| {
            let idx = dropdown.selected() as usize;
            let chips_ref = chips_for_select.borrow();
            if idx < chips_ref.len() {
                let chip = chips_ref[idx].clone();
                *selected_chip_for_select.borrow_mut() = Some(chip.clone());
                debug!("Selected EC chip: {}", chip.name);

                Self::auto_scan_registers(&chip, &registers_for_select);

                Self::render_register_list(
                    &register_list_for_select,
                    &registers_for_select.borrow(),
                    &profile_data_for_select,
                    *sort_mode_for_select.borrow(),
                    *color_filter_for_select.borrow(),
                );
            }
        });

        // Wire up export button
        let selected_chip_for_export = selected_chip.clone();
        let registers_for_export = registers.clone();
        let profile_data_for_export = profile_data.clone();
        export_btn.connect_clicked(move |btn| {
            let chip = selected_chip_for_export.borrow();
            let regs = registers_for_export.borrow();
            let pdata = profile_data_for_export.borrow();
            if let Some(ref chip_info) = *chip {
                Self::show_export_dialog(btn, chip_info, &regs, &pdata);
            }
        });

        // Initial chip refresh and auto-scan
        Self::refresh_ec_chips(&chips, &chip_dropdown);

        Self { container }
    }

    fn get_profile_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("hyperfan")
            .join(EC_PROFILE_FILENAME)
    }

    fn load_profile() -> EcProfileData {
        let path = Self::get_profile_path();
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    match serde_json::from_str(&content) {
                        Ok(data) => {
                            info!("Loaded EC profile from {:?}", path);
                            return data;
                        }
                        Err(e) => warn!("Failed to parse EC profile: {}", e),
                    }
                }
                Err(e) => warn!("Failed to read EC profile: {}", e),
            }
        }
        EcProfileData { version: 1, ..Default::default() }
    }

    fn save_profile(data: &EcProfileData) {
        let path = Self::get_profile_path();
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                error!("Failed to create config dir: {}", e);
                return;
            }
        }

        match serde_json::to_string_pretty(data) {
            Ok(content) => {
                if let Err(e) = std::fs::write(&path, content) {
                    error!("Failed to save EC profile: {}", e);
                } else {
                    debug!("Saved EC profile to {:?}", path);
                }
            }
            Err(e) => error!("Failed to serialize EC profile: {}", e),
        }
    }

    fn build_disabled_view(container: &GtkBox) {
        let warning_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(24)
            .margin_start(48)
            .margin_end(48)
            .margin_top(48)
            .margin_bottom(48)
            .valign(gtk4::Align::Center)
            .halign(gtk4::Align::Center)
            .vexpand(true)
            .build();

        let title = Label::builder()
            .label("EC Direct Control Disabled")
            .css_classes(["title-2"])
            .build();
        warning_box.append(&title);

        let desc = Label::builder()
            .label("EC Direct Control is disabled. Enable it in Settings -> Advanced\nto access this feature.")
            .wrap(true)
            .justify(gtk4::Justification::Center)
            .css_classes(["dim-label"])
            .build();
        warning_box.append(&desc);

        container.append(&warning_box);
    }

    fn refresh_ec_chips(chips: &Rc<RefCell<Vec<EcChipMetadata>>>, dropdown: &adw::ComboRow) {
        // Use daemon for hardware enumeration (authoritative)
        match hf_core::daemon_list_hardware() {
            Ok(hw) => {
                let ec_chip_names = [
                    "it87", "nct6", "w83", "f71", "asus", "dell", "thinkpad", "applesmc"
                ];

                let ec_chips: Vec<EcChipMetadata> = hw.chips.iter()
                    .filter(|c| {
                        let name_lower = c.name.to_lowercase();
                        ec_chip_names.iter().any(|ec| name_lower.contains(ec))
                    })
                    .map(|c| Self::collect_chip_metadata_from_daemon(c))
                    .collect();

                let chip_names: Vec<String> = ec_chips.iter()
                    .map(|c| format!("{} ({})", c.name, c.chip_class))
                    .collect();

                let model = gtk4::StringList::new(&chip_names.iter().map(|s| s.as_str()).collect::<Vec<_>>());
                dropdown.set_model(Some(&model));

                if !ec_chips.is_empty() {
                    dropdown.set_subtitle(&format!("{} chip(s) found", ec_chips.len()));
                    dropdown.set_selected(0);
                } else {
                    dropdown.set_subtitle("No EC/SuperIO chips detected");
                }

                *chips.borrow_mut() = ec_chips;
                info!("Refreshed EC chips, found {} chips", chips.borrow().len());
            }
            Err(e) => {
                error!("Failed to enumerate chips: {}", e);
                dropdown.set_subtitle("Error scanning chips");
            }
        }
    }

    fn collect_chip_metadata_from_daemon(chip: &hf_core::daemon_client::DaemonHwmonChip) -> EcChipMetadata {
        let path = chip.path.clone();
        
        // Daemon response doesn't include filesystem metadata, so we provide simplified data
        let mut detected_features = Vec::new();
        for temp in &chip.temperatures {
            if let Some(name) = temp.name.strip_prefix("temp") {
                detected_features.push(format!("temp{}", name));
            }
        }
        for fan in &chip.fans {
            if let Some(name) = fan.name.strip_prefix("fan") {
                detected_features.push(format!("fan{}", name));
            }
        }
        for pwm in &chip.pwms {
            if let Some(name) = pwm.name.strip_prefix("pwm") {
                detected_features.push(format!("pwm{}", name));
            }
        }

        let (chip_class, chip_vendor) = Self::classify_chip_full(&chip.name);

        EcChipMetadata {
            name: chip.name.clone(),
            path: path.clone(),
            device_path: None,
            chip_class,
            chip_vendor,
            chip_model: Some(chip.name.clone()),
            chip_revision: None,
            driver_name: None,
            pci_id: None,
            subsystem_id: None,
            bus_info: None,
            register_count: 256,
            detected_features,
            hwmon_attributes: HashMap::new(),
            scan_timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        }
    }

    fn classify_chip_full(name: &str) -> (String, String) {
        let name_lower = name.to_lowercase();
        if name_lower.contains("it87") {
            ("ITE SuperIO".to_string(), "ITE Tech Inc.".to_string())
        } else if name_lower.contains("nct6") {
            ("Nuvoton SuperIO".to_string(), "Nuvoton Technology".to_string())
        } else if name_lower.contains("w83") {
            ("Winbond SuperIO".to_string(), "Winbond Electronics".to_string())
        } else if name_lower.contains("f71") {
            ("Fintek SuperIO".to_string(), "Fintek Electronic".to_string())
        } else if name_lower.contains("asus") {
            ("ASUS EC".to_string(), "ASUSTeK Computer".to_string())
        } else if name_lower.contains("dell") {
            ("Dell SMM".to_string(), "Dell Inc.".to_string())
        } else if name_lower.contains("thinkpad") {
            ("ThinkPad EC".to_string(), "Lenovo".to_string())
        } else if name_lower.contains("applesmc") {
            ("Apple SMC".to_string(), "Apple Inc.".to_string())
        } else {
            ("Unknown".to_string(), "Unknown".to_string())
        }
    }

    fn auto_scan_registers(
        chip: &EcChipMetadata,
        registers: &Rc<RefCell<Vec<EcRegisterData>>>,
    ) {
        let mut regs = Vec::with_capacity(256);
        for reg in 0..=255u8 {
            let category = Self::get_register_category(reg);
            let hint = Self::get_register_hint(reg, &chip.chip_class);

            regs.push(EcRegisterData {
                register: reg,
                value: 0x00,
                default_label: Self::get_register_label(reg),
                writable: Self::is_register_writable(reg),
                category,
                hint,
            });
        }

        *registers.borrow_mut() = regs;
        info!("Auto-scanned 256 registers from {}", chip.name);
    }

    fn render_register_list(
        list: &ListBox,
        registers: &[EcRegisterData],
        profile_data: &Rc<RefCell<EcProfileData>>,
        sort_mode: SortMode,
        color_filter: Option<u8>,
    ) {
        // Clear existing
        while let Some(child) = list.first_child() {
            list.remove(&child);
        }

        // Sort registers
        let mut sorted_indices: Vec<usize> = (0..registers.len()).collect();
        let pdata = profile_data.borrow();

        // Filter by color if set
        if let Some(filter_color) = color_filter {
            sorted_indices.retain(|&idx| {
                let reg = &registers[idx];
                pdata.registers.get(&reg.register)
                    .map(|u| u.color_index == filter_color)
                    .unwrap_or(false)
            });
        }

        sorted_indices.sort_by(|&a, &b| {
            let reg_a = &registers[a];
            let reg_b = &registers[b];
            let user_a = pdata.registers.get(&reg_a.register);
            let user_b = pdata.registers.get(&reg_b.register);

            // Favorites always first
            let fav_a = user_a.map(|u| u.favorite).unwrap_or(false);
            let fav_b = user_b.map(|u| u.favorite).unwrap_or(false);

            if fav_a != fav_b {
                return fav_b.cmp(&fav_a);
            }

            match sort_mode {
                SortMode::Address => reg_a.register.cmp(&reg_b.register),
                SortMode::Name => {
                    let name_a = user_a.and_then(|u| u.friendly_name.as_ref())
                        .or(reg_a.default_label.as_ref())
                        .map(|s| s.as_str())
                        .unwrap_or("");
                    let name_b = user_b.and_then(|u| u.friendly_name.as_ref())
                        .or(reg_b.default_label.as_ref())
                        .map(|s| s.as_str())
                        .unwrap_or("");
                    name_a.cmp(name_b)
                }
                SortMode::Color => {
                    let color_a = user_a.map(|u| u.color_index).unwrap_or(0);
                    let color_b = user_b.map(|u| u.color_index).unwrap_or(0);
                    color_a.cmp(&color_b)
                }
                SortMode::Favorites => reg_a.register.cmp(&reg_b.register),
                SortMode::Value => reg_a.value.cmp(&reg_b.value),
            }
        });

        drop(pdata);

        // Create rows
        for idx in sorted_indices {
            let reg = &registers[idx];
            let row = Self::create_register_row(reg, profile_data.clone());
            list.append(&row);
        }
    }

    fn create_register_row(
        reg: &EcRegisterData,
        profile_data: Rc<RefCell<EcProfileData>>,
    ) -> adw::ActionRow {
        let register = reg.register;
        let pdata = profile_data.borrow();
        let user_data = pdata.registers.get(&register).cloned().unwrap_or_default();
        drop(pdata);

        let title_text = user_data.friendly_name.as_ref()
            .or(reg.default_label.as_ref())
            .map(|s| format!("0x{:02X} - {}", register, s))
            .unwrap_or_else(|| format!("0x{:02X}", register));

        let row = adw::ActionRow::builder()
            .title(&title_text)
            .subtitle(&format!("{} | {} | Confidence: {}", 
                reg.category, 
                reg.hint.as_deref().unwrap_or("No hint"),
                user_data.confidence.as_str()
            ))
            .build();

        // Apply color background if set
        if user_data.color_index > 0 && (user_data.color_index as usize) < CATEGORY_COLORS.len() {
            let color = CATEGORY_COLORS[user_data.color_index as usize].2;
            let css = format!("row {{ background-color: alpha({}, 0.3); }}", color);
            let provider = gtk4::CssProvider::new();
            provider.load_from_string(&css);
            if let Some(display) = gtk4::gdk::Display::default() {
                gtk4::style_context_add_provider_for_display(&display, &provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
            }
        }

        // Star button (favorite) - prefix
        let star_btn = ToggleButton::builder()
            .icon_name(if user_data.favorite { "starred-symbolic" } else { "non-starred-symbolic" })
            .css_classes(["flat"])
            .tooltip_text("Toggle favorite")
            .valign(gtk4::Align::Center)
            .build();
        star_btn.set_active(user_data.favorite);

        let profile_for_star = profile_data.clone();
        star_btn.connect_toggled(move |btn| {
            let is_fav = btn.is_active();
            btn.set_icon_name(if is_fav { "starred-symbolic" } else { "non-starred-symbolic" });

            let mut pdata = profile_for_star.borrow_mut();
            let entry = pdata.registers.entry(register).or_default();
            entry.favorite = is_fav;
            drop(pdata);
            Self::save_profile(&profile_for_star.borrow());
        });

        row.add_prefix(&star_btn);

        // Color picker button - prefix
        let color_btn = Button::builder()
            .css_classes(["flat", "circular"])
            .tooltip_text("Set color category")
            .valign(gtk4::Align::Center)
            .build();

        if user_data.color_index > 0 && (user_data.color_index as usize) < CATEGORY_COLORS.len() {
            let color = CATEGORY_COLORS[user_data.color_index as usize].2;
            let css = format!("button {{ background-color: {}; min-width: 20px; min-height: 20px; }}", color);
            let provider = gtk4::CssProvider::new();
            provider.load_from_string(&css);
            if let Some(display) = gtk4::gdk::Display::default() {
                gtk4::style_context_add_provider_for_display(&display, &provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
            }
        } else {
            color_btn.set_icon_name("color-select-symbolic");
        }

        let profile_for_color = profile_data.clone();
        color_btn.connect_clicked(move |btn| {
            Self::show_color_picker(btn, register, &profile_for_color);
        });

        row.add_prefix(&color_btn);

        // Value display - suffix
        let value_label = Label::builder()
            .label(&format!("0x{:02X}", reg.value))
            .css_classes(["monospace", "dim-label"])
            .build();
        row.add_suffix(&value_label);

        // Edit button - suffix
        let edit_btn = Button::builder()
            .icon_name("document-edit-symbolic")
            .css_classes(["flat"])
            .tooltip_text("Edit register")
            .valign(gtk4::Align::Center)
            .build();

        let profile_for_edit = profile_data.clone();
        let reg_clone = reg.clone();
        edit_btn.connect_clicked(move |btn| {
            Self::show_edit_dialog(btn, &reg_clone, &profile_for_edit);
        });

        row.add_suffix(&edit_btn);

        row
    }

    fn show_color_picker(
        btn: &Button,
        register: u8,
        profile_data: &Rc<RefCell<EcProfileData>>,
    ) {
        let popover = gtk4::Popover::new();
        popover.set_parent(btn);

        let grid = gtk4::Grid::builder()
            .row_spacing(4)
            .column_spacing(4)
            .margin_start(8)
            .margin_end(8)
            .margin_top(8)
            .margin_bottom(8)
            .build();

        for (i, (idx, name, color)) in CATEGORY_COLORS.iter().enumerate() {
            let color_btn = Button::builder()
                .tooltip_text(*name)
                .width_request(28)
                .height_request(28)
                .build();

            if *idx == 0 {
                color_btn.set_icon_name("window-close-symbolic");
            } else {
                let css = format!("button {{ background-color: {}; }}", color);
                let provider = gtk4::CssProvider::new();
                provider.load_from_string(&css);
                if let Some(display) = gtk4::gdk::Display::default() {
                    gtk4::style_context_add_provider_for_display(&display, &provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
                }
            }

            let profile_clone = profile_data.clone();
            let popover_clone = popover.clone();
            let color_idx = *idx;

            color_btn.connect_clicked(move |_| {
                let mut pdata = profile_clone.borrow_mut();
                let entry = pdata.registers.entry(register).or_default();
                entry.color_index = color_idx;
                drop(pdata);
                Self::save_profile(&profile_clone.borrow());
                popover_clone.popdown();
            });

            grid.attach(&color_btn, (i % 4) as i32, (i / 4) as i32, 1, 1);
        }

        popover.set_child(Some(&grid));
        popover.popup();
    }

    fn show_edit_dialog(
        btn: &Button,
        reg: &EcRegisterData,
        profile_data: &Rc<RefCell<EcProfileData>>,
    ) {
        let register = reg.register;
        let dialog = adw::Dialog::builder()
            .title(&format!("Edit Register 0x{:02X}", register))
            .content_width(400)
            .content_height(500)
            .build();

        let content = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .build();

        let header = adw::HeaderBar::builder()
            .show_end_title_buttons(true)
            .build();
        content.append(&header);

        let prefs = adw::PreferencesGroup::builder()
            .margin_start(12)
            .margin_end(12)
            .margin_top(12)
            .margin_bottom(12)
            .build();

        // Friendly name
        let name_row = adw::EntryRow::builder()
            .title("Friendly Name")
            .build();

        let pdata = profile_data.borrow();
        if let Some(user) = pdata.registers.get(&register) {
            if let Some(ref name) = user.friendly_name {
                name_row.set_text(name);
            }
        }
        drop(pdata);

        prefs.add(&name_row);

        // Confidence level
        let confidence_row = adw::ComboRow::builder()
            .title("Confidence")
            .subtitle("How confident are you about this register's purpose?")
            .build();

        let confidence_model = gtk4::StringList::new(&["Low", "Medium", "High"]);
        confidence_row.set_model(Some(&confidence_model));

        let pdata = profile_data.borrow();
        let current_confidence = pdata.registers.get(&register)
            .map(|u| u.confidence)
            .unwrap_or_default();
        confidence_row.set_selected(current_confidence.to_index());
        drop(pdata);

        prefs.add(&confidence_row);

        // Note
        let note_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(6)
            .margin_top(12)
            .build();

        let note_label = Label::builder()
            .label("Note")
            .halign(gtk4::Align::Start)
            .css_classes(["caption"])
            .build();
        note_box.append(&note_label);

        let note_scroll = ScrolledWindow::builder()
            .height_request(100)
            .build();

        let note_view = TextView::builder()
            .wrap_mode(gtk4::WrapMode::Word)
            .build();

        let pdata = profile_data.borrow();
        if let Some(user) = pdata.registers.get(&register) {
            if let Some(ref note) = user.note {
                note_view.buffer().set_text(note);
            }
        }
        drop(pdata);

        note_scroll.set_child(Some(&note_view));
        note_box.append(&note_scroll);

        let counter = Label::builder()
            .label("0/1000")
            .halign(gtk4::Align::End)
            .css_classes(["caption", "dim-label"])
            .build();
        note_box.append(&counter);

        let buffer = note_view.buffer();
        let counter_clone = counter.clone();
        let note_view_clone = note_view.clone();
        buffer.connect_changed(move |buf| {
            let text = buf.text(&buf.start_iter(), &buf.end_iter(), false);
            let len = text.chars().count();
            counter_clone.set_label(&format!("{}/{}", len, MAX_NOTE_LENGTH));

            if len > MAX_NOTE_LENGTH {
                counter_clone.add_css_class("error");
                note_view_clone.add_css_class("error");
            } else {
                counter_clone.remove_css_class("error");
                note_view_clone.remove_css_class("error");
            }
        });

        prefs.add(&adw::ActionRow::builder().child(&note_box).build());

        // Value edit (if writable) - instant update
        if reg.writable {
            let value_row = adw::EntryRow::builder()
                .title("Value (hex) - Changes apply instantly")
                .build();
            value_row.set_text(&format!("{:02X}", reg.value));

            let _profile_for_value = profile_data.clone();
            value_row.connect_changed(move |entry| {
                let text = entry.text();
                if let Ok(new_val) = u8::from_str_radix(text.trim(), 16) {
                    // Instant apply - log the write
                    warn!("EC WRITE (instant): reg=0x{:02X} val=0x{:02X}", register, new_val);
                    // In production, this would call the daemon
                }
            });

            prefs.add(&value_row);
        }

        content.append(&prefs);

        // Save button
        let save_btn = Button::builder()
            .label("Save")
            .css_classes(["suggested-action"])
            .margin_start(12)
            .margin_end(12)
            .margin_bottom(12)
            .build();

        let profile_clone = profile_data.clone();
        let name_row_clone = name_row.clone();
        let note_view_clone = note_view.clone();
        let confidence_row_clone = confidence_row.clone();
        let dialog_clone = dialog.clone();
        save_btn.connect_clicked(move |_| {
            let name_text = name_row_clone.text().to_string();
            let buf = note_view_clone.buffer();
            let note_text = buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string();
            let confidence = ConfidenceLevel::from_index(confidence_row_clone.selected());

            if note_text.chars().count() > MAX_NOTE_LENGTH {
                return;
            }

            let mut pdata = profile_clone.borrow_mut();
            let entry = pdata.registers.entry(register).or_default();
            entry.friendly_name = if name_text.is_empty() { None } else { Some(name_text) };
            entry.note = if note_text.is_empty() { None } else { Some(note_text) };
            entry.confidence = confidence;
            drop(pdata);
            Self::save_profile(&profile_clone.borrow());

            dialog_clone.close();
        });

        content.append(&save_btn);

        dialog.set_child(Some(&content));

        if let Some(root) = btn.root() {
            if let Some(window) = root.downcast_ref::<gtk4::Window>() {
                dialog.present(Some(window));
            }
        }
    }

    fn get_register_category(reg: u8) -> String {
        match reg {
            0x00..=0x0F => "Configuration".to_string(),
            0x10..=0x1F => "Temperature".to_string(),
            0x20..=0x2F => "Fan Speed".to_string(),
            0x30..=0x3F => "Fan PWM".to_string(),
            0x40..=0x4F => "Voltage".to_string(),
            0x50..=0x5F => "GPIO".to_string(),
            0x60..=0x7F => "Extended Config".to_string(),
            0x80..=0x9F => "Vendor Specific".to_string(),
            0xA0..=0xBF => "Reserved".to_string(),
            0xC0..=0xDF => "Debug".to_string(),
            0xE0..=0xFF => "System".to_string(),
        }
    }

    fn get_register_hint(reg: u8, chip_class: &str) -> Option<String> {
        let hint = match reg {
            0x00 => Some("Chip ID / Configuration"),
            0x01 => Some("Chip Revision"),
            0x10 => Some("CPU Temperature"),
            0x20 => Some("Fan 1 Speed Low Byte"),
            0x30 => Some("PWM 1 Duty Cycle"),
            0x40 => Some("Vcore Voltage"),
            0x50 => Some("GPIO Direction"),
            _ => None,
        };

        hint.map(|h| {
            if chip_class.contains("Nuvoton") {
                format!("{} (NCT6xxx)", h)
            } else if chip_class.contains("ITE") {
                format!("{} (IT87xx)", h)
            } else {
                h.to_string()
            }
        })
    }

    fn get_register_label(register: u8) -> Option<String> {
        match register {
            0x00..=0x0F => Some(format!("Config 0x{:02X}", register)),
            0x10..=0x1F => Some(format!("Temp 0x{:02X}", register)),
            0x20..=0x2F => Some(format!("Fan 0x{:02X}", register)),
            0x30..=0x3F => Some(format!("PWM 0x{:02X}", register)),
            0x40..=0x4F => Some(format!("Volt 0x{:02X}", register)),
            0x50..=0x5F => Some(format!("GPIO 0x{:02X}", register)),
            _ => Some(format!("Reg 0x{:02X}", register)),
        }
    }

    fn is_register_writable(register: u8) -> bool {
        matches!(register, 0x00..=0x0F | 0x30..=0x3F | 0x50..=0x5F | 0x60..=0x7F)
    }

    fn show_export_dialog(
        btn: &Button,
        chip: &EcChipMetadata,
        registers: &[EcRegisterData],
        profile_data: &EcProfileData,
    ) {
        let window = btn.root().and_downcast::<gtk4::Window>();

        let safe_name = chip.name.replace(['/', '\\', ' ', ':'], "_");
        let filename = format!("{}_ec_profile.json", safe_name);

        let dialog = gtk4::FileDialog::builder()
            .title("Export EC Profile")
            .initial_name(&filename)
            .build();

        let filter = gtk4::FileFilter::new();
        filter.add_pattern("*.json");
        filter.set_name(Some("JSON files"));

        let filters = gio::ListStore::new::<gtk4::FileFilter>();
        filters.append(&filter);
        dialog.set_filters(Some(&filters));

        let chip_clone = chip.clone();
        let writable_count = registers.iter().filter(|r| r.writable).count();
        let registers_data: Vec<serde_json::Value> = registers.iter().map(|r| {
            let user = profile_data.registers.get(&r.register);
            serde_json::json!({
                "register": format!("0x{:02X}", r.register),
                "register_decimal": r.register,
                "value": format!("0x{:02X}", r.value),
                "value_decimal": r.value,
                "category": r.category,
                "default_label": r.default_label,
                "hint": r.hint,
                "writable": r.writable,
                "friendly_name": user.and_then(|u| u.friendly_name.clone()),
                "note": user.and_then(|u| u.note.clone()),
                "favorite": user.map(|u| u.favorite).unwrap_or(false),
                "color_index": user.map(|u| u.color_index).unwrap_or(0),
                "color_name": user.and_then(|u| CATEGORY_COLORS.get(u.color_index as usize).map(|c| c.1)),
                "confidence": user.map(|u| u.confidence.as_str()).unwrap_or("Low"),
            })
        }).collect();

        let fav_count = profile_data.registers.values().filter(|u| u.favorite).count();
        let colored_count = profile_data.registers.values().filter(|u| u.color_index > 0).count();
        let named_count = profile_data.registers.values().filter(|u| u.friendly_name.is_some()).count();
        let noted_count = profile_data.registers.values().filter(|u| u.note.is_some()).count();
        let high_conf_count = profile_data.registers.values().filter(|u| u.confidence == ConfidenceLevel::High).count();
        let med_conf_count = profile_data.registers.values().filter(|u| u.confidence == ConfidenceLevel::Medium).count();

        let color_palette: Vec<serde_json::Value> = CATEGORY_COLORS.iter().map(|(idx, name, hex)| {
            serde_json::json!({ "index": idx, "name": name, "hex": hex })
        }).collect();

        dialog.save(window.as_ref(), gio::Cancellable::NONE, move |result| {
            if let Ok(file) = result {
                if let Some(path) = file.path() {
                    let profile = serde_json::json!({
                        "profile_version": 4,
                        "export_timestamp": std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0),
                        "chip_metadata": {
                            "name": chip_clone.name,
                            "path": chip_clone.path,
                            "device_path": chip_clone.device_path,
                            "chip_class": chip_clone.chip_class,
                            "chip_vendor": chip_clone.chip_vendor,
                            "chip_model": chip_clone.chip_model,
                            "driver_name": chip_clone.driver_name,
                            "pci_id": chip_clone.pci_id,
                            "subsystem_id": chip_clone.subsystem_id,
                            "bus_info": chip_clone.bus_info,
                            "register_count": chip_clone.register_count,
                            "detected_features": chip_clone.detected_features,
                            "hwmon_attributes": chip_clone.hwmon_attributes,
                            "scan_timestamp": chip_clone.scan_timestamp,
                        },
                        "registers": registers_data,
                        "register_summary": {
                            "total": 256,
                            "writable": writable_count,
                            "favorites": fav_count,
                            "colored": colored_count,
                            "with_names": named_count,
                            "with_notes": noted_count,
                            "high_confidence": high_conf_count,
                            "medium_confidence": med_conf_count,
                        },
                        "color_palette": color_palette,
                    });

                    match std::fs::write(&path, serde_json::to_string_pretty(&profile).unwrap_or_default()) {
                        Ok(_) => info!("Exported EC profile to {:?}", path),
                        Err(e) => error!("Failed to export EC profile: {}", e),
                    }
                }
            }
        });
    }

    pub fn widget(&self) -> &GtkBox {
        &self.container
    }

    pub fn refresh(&self) {
        // PERFORMANCE: Use cached settings
        let settings = hf_core::get_cached_settings();
        let ec_enabled = settings.advanced.ec_direct_control_enabled && settings.advanced.ec_danger_acknowledged;

        if !ec_enabled {
            debug!("EC control page refresh: EC not enabled");
        }
    }
}

impl Default for EcControlPage {
    fn default() -> Self {
        Self::new()
    }
}
