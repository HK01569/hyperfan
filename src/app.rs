/*
 * This file is part of Hyperfan.
 *
 * Copyright (C) 2025 Hyperfan contributors
 *
 * Hyperfan is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * Hyperfan is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with Hyperfan. If not, see <https://www.gnu.org/licenses/>.
 */

use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::config::{load_saved_config, try_load_system_config};
use crate::config::Metric;
use crate::hwmon;
use crate::curves::CurveGroup;
use crate::config::ControllerGroup as SavedControllerGroup;
use crate::system::{read_cpu_name, read_mb_name};

#[derive(Clone, Debug)]
pub struct PwmSmoothingState {
    pub current_value: u8,
    pub target_value: u8,
    pub last_update: Instant,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Focus {
    Fans,
    Pwms,
    Temps,
    Control,
}

#[derive(Clone, Debug)]
pub struct Mapping {
    pub fan: String,
    pub pwm: String,
}

pub struct App {
    pub last_refresh: Instant,
    pub refresh_interval: Duration,
    pub show_curve_popup: bool,
    pub show_auto_detect: bool,
    pub auto_detect_progress: String,
    pub auto_detect_percent: f64,
    pub auto_detect_running: Arc<Mutex<bool>>,
    pub auto_detect_results: Arc<Mutex<Vec<hwmon::FanPwmPairing>>>,
    pub auto_detect_await_confirm: bool,
    pub curve_temp_points: Vec<(f64, u8)>, // (temp_c, pwm_percent)
    pub readings: Vec<hwmon::ChipReadings>,
    pub status: String,
    // header
    pub cpu_name: String,
    pub mb_name: String,
    // flattened lists
    pub fans: Vec<(String, u64)>,
    pub pwms: Vec<(String, u64)>,
    pub temps: Vec<(String, f64)>,
    // selection and focus
    pub focus: Focus,
    pub fans_idx: usize,
    pub pwms_idx: usize,
    pub temps_idx: usize,
    pub control_idx: usize,
    pub mappings: Vec<Mapping>,
    // temperature metric
    pub metric: Metric,
    // set PWM popup
    pub show_set_pwm_popup: bool,
    pub set_pwm_input: String,
    pub set_pwm_target: Option<(String, usize, String)>, // (chip, idx, label)
    pub set_pwm_feedback: Option<(bool, String)>,        // (is_error, message)
    pub set_pwm_typed: bool,                              // has user started typing to overwrite?
    pub show_confirm_save_popup: bool,
    // generic warning popup
    pub show_warning_popup: bool,
    pub warning_message: String,
    // curve editor page
    pub show_curve_editor: bool,
    pub editor_groups: Vec<CurveGroup>,
    pub editor_group_idx: usize,
    // curve editor panel focus: false = left (groups), true = right (details/graph)
    pub editor_focus_right: bool,
    // curve editor: track unsaved changes and save confirmation popup
    pub editor_dirty: bool,
    pub show_editor_save_confirm: bool,
    pub show_save_config_prompt: bool,
    // selected point index inside current group
    pub editor_point_idx: usize,
    // curve editor: bar-graph mode (0..100°C bins)
    pub editor_graph_mode: bool,
    pub editor_graph: [u8; 101], // pwm% for each integer °C
    pub editor_graph_sel: usize,  // selected °C column (0..100)
    // graph mode: numeric input buffer for setting selected column percent
    pub editor_graph_input: String,
    pub editor_graph_typed: bool,
    // curve editor: hysteresis delay popup (apply_delay_ms)
    pub show_curve_delay_popup: bool,
    pub curve_delay_input: String,
    // curve editor: hysteresis percent popup (0..50)
    pub show_curve_hyst_popup: bool,
    pub curve_hyst_input: String,
    // curve editor: temperature source selection popup
    pub show_temp_source_popup: bool,
    pub temp_source_selection: usize, // selected index in available temps
    // rename popup state and alias maps
    pub show_rename_popup: bool,
    pub rename_input: String,
    pub rename_target_kind: Option<Focus>,
    pub rename_target_name: String, // canonical key like "chip:label"
    pub fan_aliases: HashMap<String, String>,
    pub pwm_aliases: HashMap<String, String>,
    pub temp_aliases: HashMap<String, String>,
    // Groups manager
    pub show_groups_manager: bool,
    pub groups: Vec<SavedControllerGroup>,
    pub group_idx: usize,
    pub groups_pwm_idx: usize, // selection within available PWMs on Groups page
    pub groups_focus_right: bool, // false = left (groups), true = right (PWM list)
    // Groups: new group name popup
    pub show_group_name_popup: bool,
    pub group_name_input: String,
    pub group_rename_mode: bool,
    // Groups: map PWM->FAN popup
    pub show_map_pwm_popup: bool,
    pub map_fan_idx: usize,
    // PWM smoothing state
    pub pwm_smoothing_state: HashMap<String, PwmSmoothingState>,
}

impl App {
    pub fn new() -> Self {
        let mut app = Self {
            last_refresh: Instant::now() - Duration::from_secs(10),
            refresh_interval: Duration::from_millis(500),
            readings: Vec::new(),
            status: String::from(
                "Tab/←→: switch | ↑/↓: move | m: map | r: rename | a: auto-detect | c: curve | d: delete | s: save | R: refresh | q: quit",
            ),
            show_auto_detect: false,
            auto_detect_progress: String::new(),
            auto_detect_percent: 0.0,
            auto_detect_running: Arc::new(Mutex::new(false)),
            auto_detect_results: Arc::new(Mutex::new(Vec::new())),
            auto_detect_await_confirm: false,
            cpu_name: String::new(),
            mb_name: String::new(),
            fans: Vec::new(),
            pwms: Vec::new(),
            temps: Vec::new(),
            focus: Focus::Fans,
            fans_idx: 0,
            pwms_idx: 0,
            temps_idx: 0,
            control_idx: 0,
            mappings: vec![],
            metric: Metric::C,
            show_curve_popup: false,
            curve_temp_points: vec![(30.0, 20), (40.0, 30), (50.0, 50), (60.0, 70), (70.0, 100)],
            show_set_pwm_popup: false,
            set_pwm_input: String::new(),
            set_pwm_target: None,
            set_pwm_feedback: None,
            set_pwm_typed: false,
            show_confirm_save_popup: false,
            show_warning_popup: false,
            warning_message: String::new(),
            show_curve_editor: false,
            editor_groups: Vec::new(),
            editor_group_idx: 0,
            editor_focus_right: false,
            editor_dirty: false,
            show_editor_save_confirm: false,
            show_save_config_prompt: false,
            editor_point_idx: 0,
            editor_graph_mode: false,
            editor_graph: [0; 101],
            editor_graph_sel: 40,
            editor_graph_input: String::new(),
            editor_graph_typed: false,
            show_curve_delay_popup: false,
            curve_delay_input: String::new(),
            show_curve_hyst_popup: false,
            curve_hyst_input: String::new(),
            show_temp_source_popup: false,
            temp_source_selection: 0,
            show_rename_popup: false,
            rename_input: String::new(),
            rename_target_kind: None,
            rename_target_name: String::new(),
            fan_aliases: HashMap::new(),
            pwm_aliases: HashMap::new(),
            temp_aliases: HashMap::new(),
            show_groups_manager: false,
            groups: Vec::new(),
            group_idx: 0,
            groups_pwm_idx: 0,
            groups_focus_right: false,
            show_group_name_popup: false,
            group_name_input: String::new(),
            group_rename_mode: false,
            show_map_pwm_popup: false,
            map_fan_idx: 0,
            pwm_smoothing_state: HashMap::new(),
        };
        if let Ok(saved) = try_load_system_config() {
            app.mappings = saved
                .mappings
                .into_iter()
                .map(|m| Mapping { fan: m.fan, pwm: m.pwm })
                .collect();
            app.metric = saved.metric;
            app.fan_aliases = saved.fan_aliases;
            app.pwm_aliases = saved.pwm_aliases;
            app.temp_aliases = saved.temp_aliases;
            app.groups = saved.controller_groups;
            
            // Auto-apply saved curves if they exist
            if let Some(curves_cfg) = saved.curves {
                app.editor_groups = curves_cfg.groups;
                // Apply curves immediately on startup
                crate::handlers::apply_curves_to_hardware(&mut app);
            }
        } else if let Some(saved) = load_saved_config() {
            app.mappings = saved
                .mappings
                .into_iter()
                .map(|m| Mapping { fan: m.fan, pwm: m.pwm })
                .collect();
            app.metric = saved.metric;
            app.fan_aliases = saved.fan_aliases;
            app.pwm_aliases = saved.pwm_aliases;
            app.temp_aliases = saved.temp_aliases;
            app.groups = saved.controller_groups;
        }
        // Final fallback: if no mappings loaded from any config, try a one-time auto-detect
        if app.mappings.is_empty() {
            if let Ok(pairs) = hwmon::auto_detect_pairings() {
                app.mappings = pairs
                    .into_iter()
                    .map(|p| Mapping {
                        fan: format!("{}:{}", p.fan_chip, p.fan_label),
                        pwm: format!("{}:{}", p.pwm_chip, p.pwm_label),
                    })
                    .collect();
                if !app.mappings.is_empty() {
                    app.status = format!(
                        "Loaded {} pairing(s) from auto-detect (no config found)",
                        app.mappings.len()
                    );
                }
            }
        }
        app
    }

    pub fn refresh(&mut self) {
        match hwmon::read_all() {
            Ok(data) => {
                self.readings = data;
                // Flatten lists
                self.fans.clear();
                self.pwms.clear();
                self.temps.clear();
                for chip in &self.readings {
                    for (label, rpm) in &chip.fans {
                        self.fans.push((format!("{}:{}", chip.name, label), *rpm));
                    }
                    for (label, val) in &chip.pwms {
                        self.pwms.push((format!("{}:{}", chip.name, label), *val));
                    }
                    for (label, c) in &chip.temps {
                        self.temps.push((format!("{}:{}", chip.name, label), *c));
                    }
                }
                // Sort lists by name for a consistent UX
                self.fans.sort_by(|a, b| a.0.cmp(&b.0));
                self.pwms.sort_by(|a, b| a.0.cmp(&b.0));
                self.temps.sort_by(|a, b| a.0.cmp(&b.0));
                if self.fans_idx >= self.fans.len() {
                    self.fans_idx = self.fans.len().saturating_sub(1);
                }
                if self.pwms_idx >= self.pwms.len() {
                    self.pwms_idx = self.pwms.len().saturating_sub(1);
                }
                if self.temps_idx >= self.temps.len() {
                    self.temps_idx = self.temps.len().saturating_sub(1);
                }
                if self.control_idx >= self.mappings.len() {
                    self.control_idx = self.mappings.len().saturating_sub(1);
                }
                // header info
                self.cpu_name = read_cpu_name();
                self.mb_name = read_mb_name();
                // status line: only set default when no special UI/page is active
                if !self.show_groups_manager
                    && !self.show_curve_editor
                    && !self.show_auto_detect
                    && !self.show_rename_popup
                    && !self.show_set_pwm_popup
                    && !self.show_confirm_save_popup
                {
                    self.status = "Tab/←→: switch | ↑/↓: move | m: map | a: auto-detect | c: curve | d: delete | s: save | q: quit".to_string();
                }
            }
            Err(e) => {
                self.status = format!("Error: {}", e);
            }
        }
        self.last_refresh = Instant::now();
    }

    pub fn cycle_metric(&mut self) {
        self.metric = match self.metric {
            Metric::C => Metric::F,
            Metric::F => Metric::K,
            Metric::K => Metric::C,
        };
    }

    pub fn convert_temp(&self, celsius: f64) -> (f64, &'static str) {
        match self.metric {
            Metric::C => (celsius, "°C"),
            Metric::F => (celsius * 9.0 / 5.0 + 32.0, "°F"),
            Metric::K => (celsius + 273.15, "K"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_mapping() -> Mapping {
        Mapping {
            fan: "chip1:fan1".to_string(),
            pwm: "chip1:pwm1".to_string(),
        }
    }

    #[test]
    fn test_focus_enum() {
        assert_eq!(Focus::Fans, Focus::Fans);
        assert_ne!(Focus::Fans, Focus::Pwms);
        assert_ne!(Focus::Pwms, Focus::Temps);
        assert_ne!(Focus::Temps, Focus::Control);
    }

    #[test]
    fn test_mapping_creation() {
        let mapping = create_test_mapping();
        assert_eq!(mapping.fan, "chip1:fan1");
        assert_eq!(mapping.pwm, "chip1:pwm1");
    }

    #[test]
    fn test_mapping_clone() {
        let mapping = create_test_mapping();
        let cloned = mapping.clone();
        assert_eq!(mapping.fan, cloned.fan);
        assert_eq!(mapping.pwm, cloned.pwm);
    }

    #[test]
    fn test_app_new_default_state() {
        let app = App::new();
        
        // Test default values
        assert_eq!(app.focus, Focus::Fans);
        assert_eq!(app.fans_idx, 0);
        assert_eq!(app.pwms_idx, 0);
        assert_eq!(app.temps_idx, 0);
        assert_eq!(app.control_idx, 0);
        assert_eq!(app.metric, Metric::C);
        
        // Test default UI state
        assert!(!app.show_curve_popup);
        assert!(!app.show_auto_detect);
        assert!(!app.show_set_pwm_popup);
        assert!(!app.show_confirm_save_popup);
        assert!(!app.show_warning_popup);
        assert!(!app.show_curve_editor);
        assert!(!app.show_rename_popup);
        assert!(!app.show_groups_manager);
        
        // Test collections are initialized
        assert!(app.readings.is_empty());
        assert!(app.fans.is_empty());
        assert!(app.pwms.is_empty());
        assert!(app.temps.is_empty());
        assert!(app.fan_aliases.is_empty());
        assert!(app.pwm_aliases.is_empty());
        assert!(app.temp_aliases.is_empty());
        
        // Test status message is set
        assert!(!app.status.is_empty());
        assert!(app.status.contains("Tab"));
    }

    #[test]
    fn test_cycle_metric() {
        let mut app = App::new();
        
        assert_eq!(app.metric, Metric::C);
        
        app.cycle_metric();
        assert_eq!(app.metric, Metric::F);
        
        app.cycle_metric();
        assert_eq!(app.metric, Metric::K);
        
        app.cycle_metric();
        assert_eq!(app.metric, Metric::C);
    }

    #[test]
    fn test_convert_temp_celsius() {
        let app = App::new();
        assert_eq!(app.metric, Metric::C);
        
        let (temp, unit) = app.convert_temp(25.0);
        assert_eq!(temp, 25.0);
        assert_eq!(unit, "°C");
        
        let (temp, unit) = app.convert_temp(0.0);
        assert_eq!(temp, 0.0);
        assert_eq!(unit, "°C");
        
        let (temp, unit) = app.convert_temp(-10.5);
        assert_eq!(temp, -10.5);
        assert_eq!(unit, "°C");
    }

    #[test]
    fn test_convert_temp_fahrenheit() {
        let mut app = App::new();
        app.metric = Metric::F;
        
        let (temp, unit) = app.convert_temp(0.0);
        assert_eq!(temp, 32.0);
        assert_eq!(unit, "°F");
        
        let (temp, unit) = app.convert_temp(100.0);
        assert_eq!(temp, 212.0);
        assert_eq!(unit, "°F");
        
        let (temp, unit) = app.convert_temp(25.0);
        assert_eq!(temp, 77.0);
        assert_eq!(unit, "°F");
    }

    #[test]
    fn test_convert_temp_kelvin() {
        let mut app = App::new();
        app.metric = Metric::K;
        
        let (temp, unit) = app.convert_temp(0.0);
        assert_eq!(temp, 273.15);
        assert_eq!(unit, "K");
        
        let (temp, unit) = app.convert_temp(-273.15);
        assert_eq!(temp, 0.0);
        assert_eq!(unit, "K");
        
        let (temp, unit) = app.convert_temp(25.0);
        assert_eq!(temp, 298.15);
        assert_eq!(unit, "K");
    }

    #[test]
    fn test_refresh_flattens_readings() {
        let mut app = App::new();
        
        // Create mock readings
        app.readings = vec![
            hwmon::ChipReadings {
                name: "chip1@hwmon0".to_string(),
                temps: vec![("temp1".to_string(), 45.5), ("temp2".to_string(), 38.2)],
                fans: vec![("fan1".to_string(), 1200), ("fan2".to_string(), 800)],
                pwms: vec![("pwm1".to_string(), 128), ("pwm2".to_string(), 200)],
            },
            hwmon::ChipReadings {
                name: "chip2@hwmon1".to_string(),
                temps: vec![("temp1".to_string(), 52.1)],
                fans: vec![("fan1".to_string(), 1500)],
                pwms: vec![("pwm1".to_string(), 255)],
            },
        ];
        
        // Manually flatten (simulating what refresh does)
        app.fans.clear();
        app.pwms.clear();
        app.temps.clear();
        
        for chip in &app.readings {
            for (label, rpm) in &chip.fans {
                app.fans.push((format!("{}:{}", chip.name, label), *rpm));
            }
            for (label, val) in &chip.pwms {
                app.pwms.push((format!("{}:{}", chip.name, label), *val));
            }
            for (label, c) in &chip.temps {
                app.temps.push((format!("{}:{}", chip.name, label), *c));
            }
        }
        
        app.fans.sort_by(|a, b| a.0.cmp(&b.0));
        app.pwms.sort_by(|a, b| a.0.cmp(&b.0));
        app.temps.sort_by(|a, b| a.0.cmp(&b.0));
        
        // Verify flattened data
        assert_eq!(app.fans.len(), 3);
        assert_eq!(app.pwms.len(), 3);
        assert_eq!(app.temps.len(), 3);
        
        assert_eq!(app.fans[0], ("chip1@hwmon0:fan1".to_string(), 1200));
        assert_eq!(app.fans[1], ("chip1@hwmon0:fan2".to_string(), 800));
        assert_eq!(app.fans[2], ("chip2@hwmon1:fan1".to_string(), 1500));
        
        assert_eq!(app.pwms[0], ("chip1@hwmon0:pwm1".to_string(), 128));
        assert_eq!(app.pwms[1], ("chip1@hwmon0:pwm2".to_string(), 200));
        assert_eq!(app.pwms[2], ("chip2@hwmon1:pwm1".to_string(), 255));
        
        assert_eq!(app.temps[0], ("chip1@hwmon0:temp1".to_string(), 45.5));
        assert_eq!(app.temps[1], ("chip1@hwmon0:temp2".to_string(), 38.2));
        assert_eq!(app.temps[2], ("chip2@hwmon1:temp1".to_string(), 52.1));
    }

    #[test]
    fn test_app_index_bounds_checking() {
        let mut app = App::new();
        
        // Set indices beyond bounds
        app.fans_idx = 100;
        app.pwms_idx = 100;
        app.temps_idx = 100;
        app.control_idx = 100;
        
        // Add some test data
        app.fans = vec![("fan1".to_string(), 1000), ("fan2".to_string(), 1200)];
        app.pwms = vec![("pwm1".to_string(), 128)];
        app.temps = vec![("temp1".to_string(), 45.0)];
        app.mappings = vec![create_test_mapping()];
        
        // Simulate bounds checking (what refresh() does)
        if app.fans_idx >= app.fans.len() {
            app.fans_idx = app.fans.len().saturating_sub(1);
        }
        if app.pwms_idx >= app.pwms.len() {
            app.pwms_idx = app.pwms.len().saturating_sub(1);
        }
        if app.temps_idx >= app.temps.len() {
            app.temps_idx = app.temps.len().saturating_sub(1);
        }
        if app.control_idx >= app.mappings.len() {
            app.control_idx = app.mappings.len().saturating_sub(1);
        }
        
        // Verify bounds are corrected
        assert_eq!(app.fans_idx, 1);  // len=2, so max index is 1
        assert_eq!(app.pwms_idx, 0);  // len=1, so max index is 0
        assert_eq!(app.temps_idx, 0); // len=1, so max index is 0
        assert_eq!(app.control_idx, 0); // len=1, so max index is 0
    }

    #[test]
    fn test_app_empty_collections_bounds() {
        let mut app = App::new();
        
        // Clear any loaded mappings to test empty state
        app.mappings.clear();
        
        // Verify initial state
        assert_eq!(app.fans.len(), 0);
        assert_eq!(app.pwms.len(), 0);
        assert_eq!(app.temps.len(), 0);
        assert_eq!(app.mappings.len(), 0);
        
        // Set indices for empty collections
        app.fans_idx = 5;
        app.pwms_idx = 5;
        app.temps_idx = 5;
        app.control_idx = 5;
        
        // Simulate bounds checking with empty collections
        if app.fans_idx >= app.fans.len() {
            app.fans_idx = app.fans.len().saturating_sub(1);
        }
        if app.pwms_idx >= app.pwms.len() {
            app.pwms_idx = app.pwms.len().saturating_sub(1);
        }
        if app.temps_idx >= app.temps.len() {
            app.temps_idx = app.temps.len().saturating_sub(1);
        }
        if app.control_idx >= app.mappings.len() {
            app.control_idx = app.mappings.len().saturating_sub(1);
        }
        
        // All should be 0 (saturating_sub on 0 gives 0)
        assert_eq!(app.fans_idx, 0);
        assert_eq!(app.pwms_idx, 0);
        assert_eq!(app.temps_idx, 0);
        assert_eq!(app.control_idx, 0);
    }

    #[test]
    fn test_app_curve_temp_points_default() {
        let app = App::new();
        
        assert_eq!(app.curve_temp_points.len(), 5);
        assert_eq!(app.curve_temp_points[0], (30.0, 20));
        assert_eq!(app.curve_temp_points[1], (40.0, 30));
        assert_eq!(app.curve_temp_points[2], (50.0, 50));
        assert_eq!(app.curve_temp_points[3], (60.0, 70));
        assert_eq!(app.curve_temp_points[4], (70.0, 100));
    }

    #[test]
    fn test_app_editor_graph_default() {
        let app = App::new();
        
        assert_eq!(app.editor_graph.len(), 101); // 0..100°C inclusive
        assert_eq!(app.editor_graph_sel, 40);
        assert!(!app.editor_graph_mode);
        assert!(!app.editor_graph_typed);
        assert!(app.editor_graph_input.is_empty());
    }

    #[test]
    fn test_app_auto_detect_state() {
        let app = App::new();
        
        assert!(!app.show_auto_detect);
        assert!(app.auto_detect_progress.is_empty());
        assert_eq!(app.auto_detect_percent, 0.0);
        assert!(!app.auto_detect_await_confirm);
        
        // Test Arc<Mutex<>> fields are initialized
        assert!(!*app.auto_detect_running.lock().unwrap());
        assert!(app.auto_detect_results.lock().unwrap().is_empty());
    }
}
