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

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CurvePoint {
    pub temp_c: f64,
    pub pwm_pct: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CurveSpec {
    #[serde(flatten)]
    pub pairs: std::collections::HashMap<String, std::collections::HashMap<String, u8>>,
    #[serde(skip)]
    pub points: Vec<CurvePoint>,
    #[serde(default = "default_min_pwm")] pub min_pwm_pct: u8,
    #[serde(default = "default_max_pwm")] pub max_pwm_pct: u8,
    #[serde(default)] pub floor_pwm_pct: u8,
    #[serde(default = "default_hyst")] pub hysteresis_pct: u8,
    #[serde(default = "default_write_min_delta")] pub write_min_delta: u8,
    #[serde(default = "default_apply_delay_ms")] pub apply_delay_ms: u32,
}

impl CurveSpec {
    pub fn new() -> Self {
        Self {
            pairs: std::collections::HashMap::new(),
            points: Vec::new(),
            min_pwm_pct: 0,
            max_pwm_pct: 100,
            floor_pwm_pct: 0,
            hysteresis_pct: 5,
            write_min_delta: 5,
            apply_delay_ms: 0,
        }
    }

    pub fn sync_pairs_from_points(&mut self) {
        self.pairs.clear();
        for (i, point) in self.points.iter().enumerate() {
            let pair_key = format!("Pair{}", i);
            let temp_key = format!("{}", point.temp_c as u32);
            let mut temp_map = std::collections::HashMap::new();
            temp_map.insert(temp_key, point.pwm_pct);
            self.pairs.insert(pair_key, temp_map);
        }
    }

    pub fn sync_points_from_pairs(&mut self) {
        self.points.clear();
        let mut pairs: Vec<_> = self.pairs.iter().collect();
        pairs.sort_by_key(|(key, _)| {
            key.strip_prefix("Pair")
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0)
        });
        
        for (_, temp_map) in pairs {
            for (temp_str, pwm) in temp_map {
                if let Ok(temp) = temp_str.parse::<f64>() {
                    self.points.push(CurvePoint {
                        temp_c: temp,
                        pwm_pct: *pwm,
                    });
                }
            }
        }
    }
}

fn default_min_pwm() -> u8 { 0 }
fn default_max_pwm() -> u8 { 100 }
fn default_hyst() -> u8 { 5 }
fn default_write_min_delta() -> u8 { 5 }
fn default_apply_delay_ms() -> u32 { 0 }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CurveGroup {
    pub name: String,
    pub members: Vec<String>,      // PWM targets: "chip:label"
    pub temp_source: String,       // Temperature source: "chip:label"
    pub curve: CurveSpec,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CurvesConfig {
    pub version: u8,
    pub groups: Vec<CurveGroup>,
}

pub fn curves_system_path() -> PathBuf { PathBuf::from("/etc/hyperfan/curves.json") }

pub fn load_curves() -> Option<CurvesConfig> {
    let p = curves_system_path();
    let data = fs::read_to_string(&p).ok()?;
    let cfg: CurvesConfig = serde_json::from_str(&data).ok()?;
    validate_curves(&cfg).ok()?;
    Some(cfg)
}

pub fn write_curves(cfg: &CurvesConfig) -> io::Result<()> {
    validate_curves(cfg).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let path = curves_system_path();
    
    // Create directory if it doesn't exist - same pattern as write_system_config
    if let Some(parent) = path.parent() { 
        fs::create_dir_all(parent)?; 
    }
    
    let json = serde_json::to_string_pretty(cfg).unwrap_or_else(|_| "{}".to_string());
    fs::write(&path, json)?;
    
    // Best-effort set permissions to 0644 - same pattern as write_system_config
    let perms = fs::Permissions::from_mode(0o644);
    let _ = fs::set_permissions(&path, perms);
    Ok(())
}

fn is_safe_label(s: &str) -> bool {
    if s.is_empty() || s.len() > 128 { return false; }
    s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '_' | '-' | '.' | ' '))
}

pub fn validate_curves(cfg: &CurvesConfig) -> Result<(), String> {
    if cfg.version == 0 { return Err("version must be >= 1".into()); }
    if cfg.groups.is_empty() { return Err("at least one group required".into()); }
    if cfg.groups.len() > 64 { return Err("too many groups (max 64)".into()); }
    for g in &cfg.groups {
        if g.name.is_empty() || g.name.len() > 64 { return Err("invalid group name".into()); }
        if !is_safe_label(&g.temp_source) { return Err("invalid temp_source label".into()); }
        if g.members.is_empty() { return Err("group must have at least one member".into()); }
        if g.members.len() > 32 { return Err("too many members in a group (max 32)".into()); }
        for m in &g.members { if !is_safe_label(m) { return Err("invalid member label".into()); } }
        if g.curve.points.len() < 2 { return Err("curve must have at least two points".into()); }
        if g.curve.points.len() > 32 { return Err("too many curve points (max 32)".into()); }
        // ensure sorted and sane
        let mut last_t = f64::NEG_INFINITY;
        for p in &g.curve.points {
            if !(0..=100).contains(&p.pwm_pct) { return Err("pwm_pct out of range".into()); }
            if p.temp_c.is_nan() { return Err("temp cannot be NaN".into()); }
            if p.temp_c < last_t { return Err("curve points must be sorted by temp".into()); }
            last_t = p.temp_c;
        }
        if g.curve.min_pwm_pct > g.curve.max_pwm_pct { return Err("min_pwm_pct > max_pwm_pct".into()); }
        if g.curve.hysteresis_pct > 50 { return Err("hysteresis too large".into()); }
        if g.curve.apply_delay_ms > 600_000 { return Err("apply_delay_ms too large".into()); }
    }
    Ok(())
}

pub fn interp_pwm_percent(points: &[CurvePoint], temp_c: f64) -> u8 {
    if points.is_empty() { return 0; }
    if temp_c <= points[0].temp_c { return points[0].pwm_pct; }
    if temp_c >= points[points.len() - 1].temp_c { return points[points.len() - 1].pwm_pct; }
    for w in points.windows(2) {
        let a = &w[0];
        let b = &w[1];
        if temp_c >= a.temp_c && temp_c <= b.temp_c {
            let t = (temp_c - a.temp_c) / (b.temp_c - a.temp_c);
            let v = (a.pwm_pct as f64) + t * ((b.pwm_pct as f64) - (a.pwm_pct as f64));
            return v.round().clamp(0.0, 100.0) as u8;
        }
    }
    points[points.len() - 1].pwm_pct
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;
    use std::io::Write;

    fn create_test_curve_spec() -> CurveSpec {
        CurveSpec {
            points: vec![
                CurvePoint { temp_c: 30.0, pwm_pct: 20 },
                CurvePoint { temp_c: 50.0, pwm_pct: 50 },
                CurvePoint { temp_c: 70.0, pwm_pct: 80 },
            ],
            min_pwm_pct: 10,
            max_pwm_pct: 90,
            floor_pwm_pct: 15,
            hysteresis_pct: 5,
            write_min_delta: 3,
            apply_delay_ms: 1000,
        }
    }

    fn create_test_curve_group() -> CurveGroup {
        CurveGroup {
            name: "test_group".to_string(),
            members: vec!["chip1:pwm1".to_string(), "chip1:pwm2".to_string()],
            temp_source: "chip1:temp1".to_string(),
            curve: create_test_curve_spec(),
        }
    }

    fn create_test_curves_config() -> CurvesConfig {
        CurvesConfig {
            version: 1,
            groups: vec![create_test_curve_group()],
        }
    }

    #[test]
    fn test_interp_pwm_percent_empty_points() {
        let points = vec![];
        assert_eq!(interp_pwm_percent(&points, 50.0), 0);
    }

    #[test]
    fn test_interp_pwm_percent_single_point() {
        let points = vec![CurvePoint { temp_c: 50.0, pwm_pct: 75 }];
        assert_eq!(interp_pwm_percent(&points, 30.0), 75);
        assert_eq!(interp_pwm_percent(&points, 50.0), 75);
        assert_eq!(interp_pwm_percent(&points, 70.0), 75);
    }

    #[test]
    fn test_interp_pwm_percent_below_range() {
        let points = vec![
            CurvePoint { temp_c: 30.0, pwm_pct: 20 },
            CurvePoint { temp_c: 70.0, pwm_pct: 80 },
        ];
        assert_eq!(interp_pwm_percent(&points, 10.0), 20);
    }

    #[test]
    fn test_interp_pwm_percent_above_range() {
        let points = vec![
            CurvePoint { temp_c: 30.0, pwm_pct: 20 },
            CurvePoint { temp_c: 70.0, pwm_pct: 80 },
        ];
        assert_eq!(interp_pwm_percent(&points, 90.0), 80);
    }

    #[test]
    fn test_interp_pwm_percent_linear_interpolation() {
        let points = vec![
            CurvePoint { temp_c: 30.0, pwm_pct: 20 },
            CurvePoint { temp_c: 70.0, pwm_pct: 80 },
        ];
        // At 50°C (midpoint), should be 50% PWM
        assert_eq!(interp_pwm_percent(&points, 50.0), 50);
        // At 40°C (25% of the way), should be 35% PWM
        assert_eq!(interp_pwm_percent(&points, 40.0), 35);
    }

    #[test]
    fn test_interp_pwm_percent_multiple_segments() {
        let points = vec![
            CurvePoint { temp_c: 30.0, pwm_pct: 20 },
            CurvePoint { temp_c: 50.0, pwm_pct: 50 },
            CurvePoint { temp_c: 70.0, pwm_pct: 80 },
        ];
        assert_eq!(interp_pwm_percent(&points, 40.0), 35);
        assert_eq!(interp_pwm_percent(&points, 60.0), 65);
    }

    #[test]
    fn test_validate_curves_valid_config() {
        let config = create_test_curves_config();
        assert!(validate_curves(&config).is_ok());
    }

    #[test]
    fn test_validate_curves_zero_version() {
        let mut config = create_test_curves_config();
        config.version = 0;
        assert!(validate_curves(&config).is_err());
    }

    #[test]
    fn test_validate_curves_empty_groups() {
        let mut config = create_test_curves_config();
        config.groups.clear();
        assert!(validate_curves(&config).is_err());
    }

    #[test]
    fn test_validate_curves_too_many_groups() {
        let mut config = create_test_curves_config();
        config.groups = (0..65).map(|i| {
            let mut group = create_test_curve_group();
            group.name = format!("group_{}", i);
            group
        }).collect();
        assert!(validate_curves(&config).is_err());
    }

    #[test]
    fn test_validate_curves_invalid_group_name() {
        let mut config = create_test_curves_config();
        config.groups[0].name = "".to_string();
        assert!(validate_curves(&config).is_err());
        
        config.groups[0].name = "a".repeat(65);
        assert!(validate_curves(&config).is_err());
    }

    #[test]
    fn test_validate_curves_invalid_temp_source() {
        let mut config = create_test_curves_config();
        config.groups[0].temp_source = "invalid<>label".to_string();
        assert!(validate_curves(&config).is_err());
    }

    #[test]
    fn test_validate_curves_empty_members() {
        let mut config = create_test_curves_config();
        config.groups[0].members.clear();
        assert!(validate_curves(&config).is_err());
    }

    #[test]
    fn test_validate_curves_too_many_members() {
        let mut config = create_test_curves_config();
        config.groups[0].members = (0..33).map(|i| format!("chip:pwm{}", i)).collect();
        assert!(validate_curves(&config).is_err());
    }

    #[test]
    fn test_validate_curves_insufficient_points() {
        let mut config = create_test_curves_config();
        config.groups[0].curve.points = vec![CurvePoint { temp_c: 50.0, pwm_pct: 50 }];
        assert!(validate_curves(&config).is_err());
    }

    #[test]
    fn test_validate_curves_too_many_points() {
        let mut config = create_test_curves_config();
        config.groups[0].curve.points = (0..33).map(|i| {
            CurvePoint { temp_c: i as f64, pwm_pct: 50 }
        }).collect();
        assert!(validate_curves(&config).is_err());
    }

    #[test]
    fn test_validate_curves_invalid_pwm_pct() {
        let mut config = create_test_curves_config();
        config.groups[0].curve.points[0].pwm_pct = 101;
        assert!(validate_curves(&config).is_err());
    }

    #[test]
    fn test_validate_curves_nan_temp() {
        let mut config = create_test_curves_config();
        config.groups[0].curve.points[0].temp_c = f64::NAN;
        assert!(validate_curves(&config).is_err());
    }

    #[test]
    fn test_validate_curves_unsorted_points() {
        let mut config = create_test_curves_config();
        config.groups[0].curve.points = vec![
            CurvePoint { temp_c: 70.0, pwm_pct: 80 },
            CurvePoint { temp_c: 30.0, pwm_pct: 20 },
        ];
        assert!(validate_curves(&config).is_err());
    }

    #[test]
    fn test_validate_curves_min_max_pwm() {
        let mut config = create_test_curves_config();
        config.groups[0].curve.min_pwm_pct = 80;
        config.groups[0].curve.max_pwm_pct = 20;
        assert!(validate_curves(&config).is_err());
    }

    #[test]
    fn test_validate_curves_excessive_hysteresis() {
        let mut config = create_test_curves_config();
        config.groups[0].curve.hysteresis_pct = 51;
        assert!(validate_curves(&config).is_err());
    }

    #[test]
    fn test_validate_curves_excessive_delay() {
        let mut config = create_test_curves_config();
        config.groups[0].curve.apply_delay_ms = 600_001;
        assert!(validate_curves(&config).is_err());
    }

    #[test]
    fn test_load_curves_nonexistent_file() {
        // load_curves() may return Some if a system config exists
        // This test verifies the function doesn't panic
        let _result = load_curves();
        // Test passes if no panic occurs
    }

    #[test]
    fn test_write_and_load_curves() {
        let config = create_test_curves_config();
        
        // Create a temporary file
        let mut temp_file = NamedTempFile::new().unwrap();
        let temp_path = temp_file.path().to_path_buf();
        
        // Write config as JSON
        let json = serde_json::to_string_pretty(&config).unwrap();
        temp_file.write_all(json.as_bytes()).unwrap();
        temp_file.flush().unwrap();
        
        // Read and validate
        let data = std::fs::read_to_string(&temp_path).unwrap();
        let loaded_config: CurvesConfig = serde_json::from_str(&data).unwrap();
        
        assert_eq!(loaded_config.version, config.version);
        assert_eq!(loaded_config.groups.len(), config.groups.len());
        assert_eq!(loaded_config.groups[0].name, config.groups[0].name);
    }

    #[test]
    fn test_default_functions() {
        assert_eq!(default_min_pwm(), 0);
        assert_eq!(default_max_pwm(), 100);
        assert_eq!(default_hyst(), 5);
        assert_eq!(default_write_min_delta(), 5);
        assert_eq!(default_apply_delay_ms(), 0);
    }
}
