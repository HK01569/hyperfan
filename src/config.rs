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

use std::env;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use crate::curves::{CurvesConfig, validate_curves};

use crate::app::Mapping;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControllerGroup {
    pub name: String,
    pub members: Vec<String>, // list of pwm "chip:label"
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Metric {
    C,
    F,
    K,
}

fn default_metric() -> Metric { Metric::C }

#[derive(Debug, Serialize, Deserialize)]
pub struct SavedMapping {
    pub fan: String,
    pub pwm: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedConfig {
    pub mappings: Vec<SavedMapping>,
    #[serde(default = "default_metric")]
    pub metric: Metric,
    /// Optional curve groups. When present, the service should drive fans using these curves.
    #[serde(default)]
    pub curves: Option<CurvesConfig>,
    /// Optional user-friendly aliases for display
    #[serde(default)]
    pub fan_aliases: HashMap<String, String>,
    #[serde(default)]
    pub pwm_aliases: HashMap<String, String>,
    #[serde(default)]
    pub temp_aliases: HashMap<String, String>,
    /// Optional controller groups for organizing PWMs
    #[serde(default)]
    pub controller_groups: Vec<ControllerGroup>,
    /// Optional persistent manual PWM overrides: key is "chip:label", value is percent 0..100
    #[serde(default)]
    pub pwm_overrides: HashMap<String, u8>,
}

pub fn config_path() -> PathBuf {
    if let Ok(xdg) = env::var("XDG_CONFIG_HOME") {
        return Path::new(&xdg).join("hyperfan").join("config.json");
    }
    if let Ok(home) = env::var("HOME") {
        return Path::new(&home)
            .join(".config")
            .join("hyperfan")
            .join("config.json");
    }
    PathBuf::from("/etc/hyperfan/config.json")
}

pub fn load_saved_config() -> Option<SavedConfig> {
    let path = config_path();
    let data = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&data).ok()
}

pub fn save_mappings(mappings: &[Mapping]) -> io::Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let saved = SavedConfig {
        mappings: mappings
            .iter()
            .map(|m| SavedMapping { fan: m.fan.clone(), pwm: m.pwm.clone() })
            .collect(),
        metric: default_metric(),
        curves: None,
        fan_aliases: HashMap::new(),
        pwm_aliases: HashMap::new(),
        temp_aliases: HashMap::new(),
        controller_groups: Vec::new(),
        pwm_overrides: HashMap::new(),
    };
    let json = serde_json::to_string_pretty(&saved).unwrap_or_else(|_| "{}".to_string());
    fs::write(path, json)
}

pub fn system_config_path() -> PathBuf { PathBuf::from("/etc/hyperfan/profile.json") }

pub fn write_system_config(saved: &SavedConfig) -> io::Result<()> {
    let path = system_config_path();
    if let Some(parent) = path.parent() { let _ = fs::create_dir_all(parent); }
    let json = serde_json::to_string_pretty(saved).unwrap_or_else(|_| "{}".to_string());
    fs::write(&path, json)?;
    // Best-effort set permissions to 0644
    let perms = fs::Permissions::from_mode(0o644);
    let _ = fs::set_permissions(&path, perms);
    Ok(())
}

fn is_safe_label(s: &str) -> bool {
    // Allow alnum and common separators used in chip:label strings
    if s.is_empty() || s.len() > 128 { return false; }
    s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '_' | '-' | '.' | ' ' | '@'))
}

pub fn validate_saved_config(cfg: &SavedConfig) -> Result<(), String> {
    // Curves, if present, must be valid
    if let Some(curves) = &cfg.curves {
        validate_curves(curves)?;
    }

    // Mappings are optional when curves are present; still validate if provided
    if cfg.mappings.len() > 256 {
        return Err("too many mappings (max 256)".to_string());
    }
    for (i, m) in cfg.mappings.iter().enumerate() {
        if !(is_safe_label(&m.fan) && is_safe_label(&m.pwm)) {
            return Err(format!("invalid characters or length in mapping #{}", i + 1));
        }
        // Expect the form chip:label for fan and pwm
        if !(m.fan.contains(':') && m.pwm.contains(':')) {
            return Err(format!("mapping #{} must be of form 'chip:label'", i + 1));
        }
    }

    // Validate aliases (keys and values)
    for (k, v) in &cfg.fan_aliases {
        if !(is_safe_label(k) && k.contains(':')) { return Err("invalid fan alias key".to_string()); }
        if !is_safe_label(v) { return Err("invalid fan alias value".to_string()); }
    }
    for (k, v) in &cfg.pwm_aliases {
        if !(is_safe_label(k) && k.contains(':')) { return Err("invalid pwm alias key".to_string()); }
        if !is_safe_label(v) { return Err("invalid pwm alias value".to_string()); }
    }
    for (k, v) in &cfg.temp_aliases {
        if !(is_safe_label(k) && k.contains(':')) { return Err("invalid temp alias key".to_string()); }
        if !is_safe_label(v) { return Err("invalid temp alias value".to_string()); }
    }
    // Validate controller groups
    if cfg.controller_groups.len() > 256 { return Err("too many controller groups".to_string()); }
    for g in &cfg.controller_groups {
        if g.name.is_empty() || g.name.len() > 128 { return Err("invalid controller group name".to_string()); }
        for m in &g.members {
            if !(is_safe_label(m) && m.contains(':')) { return Err("invalid controller group member".to_string()); }
        }
    }
    // Validate pwm_overrides
    if cfg.pwm_overrides.len() > 1024 { return Err("too many pwm overrides".to_string()); }
    for (k, v) in &cfg.pwm_overrides {
        if !(is_safe_label(k) && k.contains(':')) { return Err("invalid pwm override key".to_string()); }
        if *v > 100 { return Err("pwm override percent out of range (0..100)".to_string()); }
    }
    Ok(())
}

pub fn try_load_system_config() -> Result<SavedConfig, String> {
    let path = system_config_path();
    let data = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let cfg: SavedConfig = serde_json::from_str(&data).map_err(|e| format!("parse error: {}", e))?;
    validate_saved_config(&cfg)?;
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;
    use std::io::Write;

    fn create_test_mapping() -> SavedMapping {
        SavedMapping {
            fan: "chip1:fan1".to_string(),
            pwm: "chip1:pwm1".to_string(),
        }
    }

    fn create_test_controller_group() -> ControllerGroup {
        ControllerGroup {
            name: "test_group".to_string(),
            members: vec!["chip1:pwm1".to_string(), "chip1:pwm2".to_string()],
        }
    }

    fn create_test_saved_config() -> SavedConfig {
        let mut fan_aliases = HashMap::new();
        fan_aliases.insert("chip1:fan1".to_string(), "CPU Fan".to_string());
        
        let mut pwm_aliases = HashMap::new();
        pwm_aliases.insert("chip1:pwm1".to_string(), "CPU PWM".to_string());
        
        let mut temp_aliases = HashMap::new();
        temp_aliases.insert("chip1:temp1".to_string(), "CPU Temp".to_string());
        
        let mut pwm_overrides = HashMap::new();
        pwm_overrides.insert("chip1:pwm1".to_string(), 75);

        SavedConfig {
            mappings: vec![create_test_mapping()],
            metric: Metric::C,
            curves: None,
            fan_aliases,
            pwm_aliases,
            temp_aliases,
            controller_groups: vec![create_test_controller_group()],
            pwm_overrides,
        }
    }

    #[test]
    fn test_metric_serialization() {
        assert_eq!(serde_json::to_string(&Metric::C).unwrap(), "\"c\"");
        assert_eq!(serde_json::to_string(&Metric::F).unwrap(), "\"f\"");
        assert_eq!(serde_json::to_string(&Metric::K).unwrap(), "\"k\"");
    }

    #[test]
    fn test_metric_deserialization() {
        assert_eq!(serde_json::from_str::<Metric>("\"c\"").unwrap(), Metric::C);
        assert_eq!(serde_json::from_str::<Metric>("\"f\"").unwrap(), Metric::F);
        assert_eq!(serde_json::from_str::<Metric>("\"k\"").unwrap(), Metric::K);
    }

    #[test]
    fn test_default_metric() {
        assert_eq!(default_metric(), Metric::C);
    }

    #[test]
    fn test_config_path_with_xdg() {
        std::env::set_var("XDG_CONFIG_HOME", "/custom/config");
        let path = config_path();
        assert!(path.to_string_lossy().contains("/custom/config/hyperfan/config.json"));
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    #[test]
    fn test_config_path_with_home() {
        std::env::remove_var("XDG_CONFIG_HOME");
        std::env::set_var("HOME", "/home/testuser");
        let path = config_path();
        assert!(path.to_string_lossy().contains("/home/testuser/.config/hyperfan/config.json"));
    }

    #[test]
    fn test_config_path_fallback() {
        // Test that config_path() returns a valid path
        // Note: In test environment, HOME may still be set
        let path = config_path();
        assert!(path.to_string_lossy().contains("hyperfan"));
        assert!(path.to_string_lossy().contains("config.json"));
    }

    #[test]
    fn test_system_config_path() {
        assert_eq!(system_config_path(), PathBuf::from("/etc/hyperfan/profile.json"));
    }

    #[test]
    fn test_load_saved_config_nonexistent() {
        // Should return None for non-existent file
        assert!(load_saved_config().is_none());
    }

    #[test]
    fn test_validate_saved_config_valid() {
        let config = create_test_saved_config();
        assert!(validate_saved_config(&config).is_ok());
    }

    #[test]
    fn test_validate_saved_config_too_many_mappings() {
        let mut config = create_test_saved_config();
        config.mappings = (0..257).map(|i| SavedMapping {
            fan: format!("chip{}:fan1", i),
            pwm: format!("chip{}:pwm1", i),
        }).collect();
        assert!(validate_saved_config(&config).is_err());
    }

    #[test]
    fn test_validate_saved_config_invalid_mapping_characters() {
        let mut config = create_test_saved_config();
        config.mappings[0].fan = "invalid<>fan".to_string();
        assert!(validate_saved_config(&config).is_err());
    }

    #[test]
    fn test_validate_saved_config_missing_colon_in_mapping() {
        let mut config = create_test_saved_config();
        config.mappings[0].fan = "invalidfan".to_string();
        assert!(validate_saved_config(&config).is_err());
    }

    #[test]
    fn test_validate_saved_config_invalid_fan_alias_key() {
        let mut config = create_test_saved_config();
        config.fan_aliases.insert("invalid<>key".to_string(), "value".to_string());
        assert!(validate_saved_config(&config).is_err());
    }

    #[test]
    fn test_validate_saved_config_invalid_fan_alias_value() {
        let mut config = create_test_saved_config();
        config.fan_aliases.insert("chip:fan".to_string(), "invalid<>value".to_string());
        assert!(validate_saved_config(&config).is_err());
    }

    #[test]
    fn test_validate_saved_config_too_many_controller_groups() {
        let mut config = create_test_saved_config();
        config.controller_groups = (0..257).map(|i| ControllerGroup {
            name: format!("group_{}", i),
            members: vec![format!("chip:pwm{}", i)],
        }).collect();
        assert!(validate_saved_config(&config).is_err());
    }

    #[test]
    fn test_validate_saved_config_invalid_controller_group_name() {
        let mut config = create_test_saved_config();
        config.controller_groups[0].name = "".to_string();
        assert!(validate_saved_config(&config).is_err());
        
        config.controller_groups[0].name = "a".repeat(129);
        assert!(validate_saved_config(&config).is_err());
    }

    #[test]
    fn test_validate_saved_config_invalid_controller_group_member() {
        let mut config = create_test_saved_config();
        config.controller_groups[0].members[0] = "invalid_member".to_string();
        assert!(validate_saved_config(&config).is_err());
    }

    #[test]
    fn test_validate_saved_config_too_many_pwm_overrides() {
        let mut config = create_test_saved_config();
        config.pwm_overrides = (0..1025).map(|i| {
            (format!("chip:pwm{}", i), 50)
        }).collect();
        assert!(validate_saved_config(&config).is_err());
    }

    #[test]
    fn test_validate_saved_config_invalid_pwm_override_key() {
        let mut config = create_test_saved_config();
        config.pwm_overrides.insert("invalid_key".to_string(), 50);
        assert!(validate_saved_config(&config).is_err());
    }

    #[test]
    fn test_validate_saved_config_invalid_pwm_override_value() {
        let mut config = create_test_saved_config();
        config.pwm_overrides.insert("chip:pwm1".to_string(), 101);
        assert!(validate_saved_config(&config).is_err());
    }

    #[test]
    fn test_save_mappings() -> Result<(), Box<dyn std::error::Error>> {
        let mappings = vec![
            crate::app::Mapping {
                fan: "chip1:fan1".to_string(),
                pwm: "chip1:pwm1".to_string(),
            },
            crate::app::Mapping {
                fan: "chip2:fan1".to_string(),
                pwm: "chip2:pwm1".to_string(),
            },
        ];

        // Create a temporary file to test saving
        let temp_file = tempfile::NamedTempFile::new()?;
        let _temp_path = temp_file.path().to_path_buf();
        // We can't easily test the actual save_mappings function since it uses a fixed path,
        // but we can test the serialization logic
        let saved = SavedConfig {
            mappings: mappings
                .iter()
                .map(|m| SavedMapping { fan: m.fan.clone(), pwm: m.pwm.clone() })
                .collect(),
            metric: default_metric(),
            curves: None,
            fan_aliases: HashMap::new(),
            pwm_aliases: HashMap::new(),
            temp_aliases: HashMap::new(),
            controller_groups: Vec::new(),
            pwm_overrides: HashMap::new(),
        };
        
        let json = serde_json::to_string_pretty(&saved).unwrap();
        assert!(json.contains("chip1:fan1"));
        assert!(json.contains("chip1:pwm1"));
        assert!(json.contains("chip2:fan1"));
        assert!(json.contains("chip2:pwm1"));
        Ok(())
    }

    #[test]
    fn test_write_and_read_system_config() {
        let config = create_test_saved_config();
        
        // Create a temporary file
        let mut temp_file = NamedTempFile::new().unwrap();
        let temp_path = temp_file.path().to_path_buf();
        
        // Write config as JSON
        let json = serde_json::to_string_pretty(&config).unwrap();
        temp_file.write_all(json.as_bytes()).unwrap();
        temp_file.flush().unwrap();
        
        // Read and validate
        let data = std::fs::read_to_string(&temp_path).unwrap();
        let loaded_config: SavedConfig = serde_json::from_str(&data).unwrap();
        
        assert_eq!(loaded_config.mappings.len(), config.mappings.len());
        assert_eq!(loaded_config.metric, config.metric);
        assert_eq!(loaded_config.fan_aliases.len(), config.fan_aliases.len());
        assert_eq!(loaded_config.controller_groups.len(), config.controller_groups.len());
    }

    #[test]
    fn test_is_safe_label() {
        assert!(is_safe_label("chip1:fan1"));
        assert!(is_safe_label("test_label"));
        assert!(is_safe_label("test-label"));
        assert!(is_safe_label("test.label"));
        assert!(is_safe_label("test label"));
        assert!(is_safe_label("chip@hwmon1"));
        
        assert!(!is_safe_label(""));
        assert!(!is_safe_label(&"a".repeat(129)));
        assert!(!is_safe_label("invalid<>label"));
        assert!(!is_safe_label("invalid/label"));
    }
}
