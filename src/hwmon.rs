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
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;
use std::sync::Mutex;
use std::collections::HashSet;
use thiserror::Error;
use lazy_static::lazy_static;
use serde_json::json;
use crate::logger;

#[derive(Debug, Clone)]
pub struct ChipReadings {
    pub name: String,
    pub temps: Vec<(String, f64)>, // Celsius
    pub fans: Vec<(String, u64)>,  // RPM
    pub pwms: Vec<(String, u64)>,  // raw 0-255 typically
}

/// Enumerate all fans, PWMs, and temperature sensors across all hwmon chips.
/// Provides full coverage independent of label formats.
pub fn enumerate_all_sensors() -> SensorInventory {
    let root = Path::new("/sys/class/hwmon");
    let mut fans: Vec<(String, usize, String)> = Vec::new();
    let mut pwms: Vec<(String, usize, String)> = Vec::new();
    let mut temps: Vec<(String, usize, String)> = Vec::new();

    if let Ok(entries) = fs::read_dir(root) {
        for ent in entries.flatten() {
            let path = ent.path();
            if !path.is_dir() { continue; }
            let dir = fs::canonicalize(&path).unwrap_or(path);
            let chip_base = read_trimmed(dir.join("name")).unwrap_or_else(|_| "unknown".into());
            let hwmon_tag = dir.file_name().and_then(|s| s.to_str()).unwrap_or("hwmon?");
            let chip_id = format!("{}@{}", chip_base, hwmon_tag);

            if let Ok(dir_iter) = fs::read_dir(&dir) {
                for file in dir_iter.flatten() {
                    let fname = file.file_name();
                    let fname = fname.to_string_lossy();
                    if fname.starts_with("fan") && fname.ends_with("_input") {
                        if let Some(idx) = extract_index(&fname, "fan", "_input") {
                            let label = read_trimmed(dir.join(format!("fan{}_label", idx)))
                                .unwrap_or_else(|_| format!("fan{}", idx));
                            fans.push((chip_id.clone(), idx, label));
                        }
                    } else if fname.starts_with("pwm") && !fname.contains('_') {
                        if let Some(idx) = extract_index(&fname, "pwm", "") {
                            let label = read_trimmed(dir.join(format!("pwm{}_label", idx)))
                                .unwrap_or_else(|_| format!("pwm{}", idx));
                            pwms.push((chip_id.clone(), idx, label));
                        }
                    } else if fname.starts_with("temp") && fname.ends_with("_input") {
                        if let Some(idx) = extract_index(&fname, "temp", "_input") {
                            let label = read_trimmed(dir.join(format!("temp{}_label", idx)))
                                .unwrap_or_else(|_| format!("temp{}", idx));
                            temps.push((chip_id.clone(), idx, label));
                        }
                    }
                }
            }
        }
    }

    SensorInventory { fans, pwms, temps }
}

#[derive(Debug, Clone)]
pub struct SensorInventory {
    pub fans: Vec<(String, usize, String)>,  // (chip@hwmonX, idx, label)
    pub pwms: Vec<(String, usize, String)>,  // (chip@hwmonX, idx, label)
    pub temps: Vec<(String, usize, String)>, // (chip@hwmonX, idx, label)
}

// Read hwmon chip update interval in milliseconds if available
fn read_update_interval(chip_name: &str) -> Option<u64> {
    let dir = resolve_chip_dir(chip_name)?;
    let paths = [
        dir.join("update_interval"),
        dir.join("device").join("update_interval"),
    ];
    for p in paths {
        if p.exists() {
            if let Ok(s) = read_trimmed(&p) {
                if let Ok(v) = s.parse::<u64>() { return Some(v); }
            }
        }
    }
    None
}

// Resolve the sysfs directory for a chip name
fn resolve_chip_dir(chip_selector: &str) -> Option<PathBuf> {
    // Support selectors of the form "name@hwmonX" or plain "name"
    let (want_name, tag_opt) = match chip_selector.split_once('@') {
        Some((n, tag)) => (n, Some(tag)),
        None => (chip_selector, None),
    };

    let root = Path::new("/sys/class/hwmon");
    let entries = fs::read_dir(root).ok()?;
    let mut fallback: Option<PathBuf> = None;
    for ent in entries.flatten() {
        let dir = ent.path();
        let file_tag = dir.file_name().and_then(|s| s.to_str()).unwrap_or("");

        // If a tag like hwmonX is provided, match it first
        if let Some(tag) = tag_opt {
            if file_tag == tag {
                return Some(dir);
            }
            if let Ok(canon) = fs::canonicalize(&dir) {
                let canon_tag = canon.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if canon_tag == tag || canon.as_path() == Path::new(tag) {
                    return Some(canon);
                }
            }
        }

        // Fallback: match by name file
        let name_path = dir.join("name");
        if let Ok(name) = read_trimmed(&name_path) {
            if name == want_name {
                if tag_opt.is_none() {
                    return Some(dir);
                }
                if fallback.is_none() { fallback = Some(dir); }
            }
        }
    }
    fallback
}

// Read current pwm value and enable mode (if available)
fn read_pwm_state(chip_name: &str, pwm_idx: usize) -> io::Result<(u32, Option<u8>)> {
    if let Some(dir) = resolve_chip_dir(chip_name) {
        let pwm_path = dir.join(format!("pwm{}", pwm_idx));
        let enable_path = dir.join(format!("pwm{}_enable", pwm_idx));
        let val = read_trimmed(&pwm_path)?.parse::<u64>().unwrap_or(0) as u32;
        let enable = if enable_path.exists() {
            Some(read_trimmed(&enable_path)?.parse::<u8>().unwrap_or(0))
        } else { None };
        Ok((val, enable))
    } else {
        Err(io::Error::new(io::ErrorKind::NotFound, "chip not found"))
    }
}

// Restore pwm value and enable mode to previous state
fn restore_pwm_state(chip_name: &str, pwm_idx: usize, value: u32, enable: Option<u8>) -> io::Result<()> {
    if let Some(dir) = resolve_chip_dir(chip_name) {
        let pwm_path = dir.join(format!("pwm{}", pwm_idx));
        let enable_path = dir.join(format!("pwm{}_enable", pwm_idx));
        // If enable was manual (1), ensure manual before writing value to avoid auto override
        if enable_path.exists() {
            if let Some(en) = enable {
                // If restoring to auto (2), write value first (while manual), then restore mode
                if en == 2 {
                    // set to manual to write value reliably
                    let _ = fs::write(&enable_path, "1");
                    let _ = fs::write(&pwm_path, value.to_string());
                    let _ = fs::write(&enable_path, en.to_string());
                } else {
                    let _ = fs::write(&enable_path, en.to_string());
                    let _ = fs::write(&pwm_path, value.to_string());
                }
                return Ok(());
            }
        }
        // No enable handling; just write value
        fs::write(&pwm_path, value.to_string())
    } else {
        Err(io::Error::new(io::ErrorKind::NotFound, "chip not found"))
    }
}

/// Try to resolve a PWM numeric index for a given chip and label shown in UI.
/// It checks pwmN_label contents for matches; if none found and the label looks like
/// "pwmN", it will return that N if the file exists.
pub fn find_pwm_index_by_label(chip_name: &str, label: &str) -> Option<usize> {
    // Support selectors like "name" and "name@hwmonX" by resolving to a concrete dir
    let dir = resolve_chip_dir(chip_name)?;
    if let Ok(dir_iter) = fs::read_dir(&dir) {
        for file in dir_iter.flatten() {
            let fname = file.file_name();
            let fname = fname.to_string_lossy();
            if fname.starts_with("pwm") && fname.ends_with("_label") {
                if let Some(idx) = extract_index(&fname, "pwm", "_label") {
                    if let Ok(lbl) = read_trimmed(file.path()) {
                        if lbl == label {
                            return Some(idx);
                        }
                    }
                }
            }
        }
    }
    // Fallback: label "pwmN"
    if let Some(idx) = label.strip_prefix("pwm").and_then(|s| s.parse::<usize>().ok()) {
        if dir.join(format!("pwm{}", idx)).exists() {
            return Some(idx);
        }
    }
    None
}

#[derive(Error, Debug)]
pub enum HwmonError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("Invalid data: {0}")]
    InvalidData(String),
    #[error("Permission denied - need root")]
    PermissionDenied,
}

pub fn read_all() -> Result<Vec<ChipReadings>, HwmonError> {
    let root = Path::new("/sys/class/hwmon");
    let mut out: Vec<ChipReadings> = Vec::new();

    let entries = match fs::read_dir(root) {
        Ok(it) => it,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e.into()),
    };

    for ent in entries.flatten() {
        let path = ent.path();
        if !path.is_dir() { continue; }

        // Resolve to the actual device dir in case of symlink
        let dir = match fs::canonicalize(&path) {
            Ok(p) => p,
            Err(_) => path.clone(),
        };

        // Build a unique chip identifier like "name@hwmonX" to disambiguate
        // multiple instances of the same driver. This improves downstream mapping
        // and ensures we can address ALL sensors across chips reliably.
        let base_name = read_trimmed(dir.join("name")).unwrap_or_else(|_| "unknown".into());
        let hwmon_tag = dir.file_name().and_then(|s| s.to_str()).unwrap_or("hwmon?");
        let name = format!("{}@{}", base_name, hwmon_tag);

        let mut temps: Vec<(String, f64)> = Vec::new();
        let mut fans: Vec<(String, u64)> = Vec::new();
        let mut pwms: Vec<(String, u64)> = Vec::new();

        let Ok(dir_iter) = fs::read_dir(&dir) else { continue };
        for file in dir_iter.flatten() {
            let fname = file.file_name();
            let fname = fname.to_string_lossy();
            let fpath = file.path();

            if fname.starts_with("temp") && fname.ends_with("_input") {
                if let Some(idx) = extract_index(&fname, "temp", "_input") {
                    let label = read_trimmed(dir.join(format!("temp{}_label", idx)))
                        .unwrap_or_else(|_| format!("temp{}", idx));
                    if let Ok(raw) = read_trimmed(&fpath) {
                        if let Ok(mc) = raw.parse::<i64>() { // millidegree C
                            let val_c = (mc as f64) / 1000.0;
                            temps.push((label, val_c));
                        }
                    }
                }
            } else if fname.starts_with("fan") && fname.ends_with("_input") {
                if let Some(idx) = extract_index(&fname, "fan", "_input") {
                    let label = read_trimmed(dir.join(format!("fan{}_label", idx)))
                        .unwrap_or_else(|_| format!("fan{}", idx));
                    if let Ok(raw) = read_trimmed(&fpath) {
                        if let Ok(rpm) = raw.parse::<u64>() {
                            // some sensors use 0 to indicate stopped/invalid
                            fans.push((label, rpm));
                        }
                    }
                }
            } else if fname.starts_with("pwm") && !fname.contains('_') {
                if let Some(idx) = extract_index(&fname, "pwm", "") {
                    let label = read_trimmed(dir.join(format!("pwm{}_label", idx)))
                        .unwrap_or_else(|_| format!("pwm{}", idx));
                    if let Ok(raw) = read_trimmed(&fpath) {
                        if let Ok(val) = raw.parse::<u64>() {
                            pwms.push((label, val));
                        }
                    }
                }
            }
        }

        // Keep only chips with at least some data, but include others as empty to show presence
        out.push(ChipReadings { name, temps, fans, pwms });
    }

    // Sort by name for stable display
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

fn read_trimmed<P: AsRef<Path>>(p: P) -> io::Result<String> {
    let mut s = String::new();
    fs::File::open(p)?.read_to_string(&mut s)?;
    Ok(s.trim().to_string())
}

pub fn extract_index(fname: &str, prefix: &str, suffix: &str) -> Option<usize> {
    if fname.starts_with(prefix) && fname.ends_with(suffix) {
        let mid = &fname[prefix.len()..fname.len() - suffix.len()];
        mid.parse().ok()
    } else {
        None
    }
}

pub fn write_pwm(chip_name: &str, pwm_idx: usize, value: u8) -> Result<(), HwmonError> {
    if let Some(dir) = resolve_chip_dir(chip_name) {
        let pwm_path = dir.join(format!("pwm{}", pwm_idx));
        let enable_path = dir.join(format!("pwm{}_enable", pwm_idx));
        let pwm_max_path = dir.join(format!("pwm{}_max", pwm_idx));
        // Set to manual mode (1) first
        let mut manual_forced = false;
        if enable_path.exists() {
            fs::write(&enable_path, "1")?;
            manual_forced = true;
        }
        // Determine scaled write value if pwm*_max is present
        let write_val = if pwm_max_path.exists() {
            match read_trimmed(&pwm_max_path)?.parse::<u64>() {
                Ok(maxv) if maxv > 0 => {
                    let scaled = (value as u64) * maxv / 255u64;
                    scaled.to_string()
                }
                _ => value.to_string(),
            }
        } else {
            value.to_string()
        };
        // Write PWM value
        fs::write(&pwm_path, &write_val)?;
        // Emit JSON log line (no-op if logger not initialized)
        logger::log_event(
            "pwm_write",
            json!({
                "chip": chip_name,
                "idx": pwm_idx,
                "requested_raw": value,
                "written": write_val,
                "manual_forced": manual_forced,
            }),
        );
        return Ok(());
    }
    Err(HwmonError::InvalidData(format!("Chip {} not found", chip_name)))
}

pub fn read_single_fan(chip_name: &str, fan_idx: usize) -> Result<u64, HwmonError> {
    if let Some(dir) = resolve_chip_dir(chip_name) {
        let fan_path = dir.join(format!("fan{}_input", fan_idx));
        if let Ok(raw) = read_trimmed(&fan_path) {
            if let Ok(rpm) = raw.parse::<u64>() {
                return Ok(rpm);
            }
        }
        return Err(HwmonError::InvalidData(format!("Fan {}:{} not found", chip_name, fan_idx)));
    }
    Err(HwmonError::InvalidData(format!("Chip {} not found", chip_name)))
}

#[derive(Debug, Clone)]
pub struct FanPwmPairing {
    pub fan_chip: String,
    pub fan_idx: usize,
    pub fan_label: String,
    pub pwm_chip: String,
    pub pwm_idx: usize,
    pub pwm_label: String,
    pub confidence: f64, // 0.0 to 1.0
}

lazy_static! {
    pub static ref AUTO_DETECT_PROGRESS: Mutex<f64> = Mutex::new(0.0);
    static ref AUTO_DETECT_CANCEL: Mutex<bool> = Mutex::new(false);
}

fn is_cancelled() -> bool {
    match AUTO_DETECT_CANCEL.lock() {
        Ok(guard) => *guard,
        Err(poisoned) => *poisoned.into_inner(),
    }
}

pub fn request_cancel_autodetect() {
    match AUTO_DETECT_CANCEL.lock() {
        Ok(mut guard) => *guard = true,
        Err(poisoned) => *poisoned.into_inner() = true,
    }
}

pub fn clear_cancel_autodetect() {
    match AUTO_DETECT_CANCEL.lock() {
        Ok(mut guard) => *guard = false,
        Err(poisoned) => *poisoned.into_inner() = false,
    }
}

fn log_autodetect_line(line: &str) {
    // Append auto-detect logs to a file to keep the TUI clean
    if let Ok(mut f) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/hyperfan_autodetect.log")
    {
        let _ = writeln!(f, "{}", line);
    }
    // Also emit to the structured logger if initialized
    logger::log_event("autodetect", json!({ "line": line }));
}

pub fn auto_detect_pairings_with_progress() -> Result<Vec<FanPwmPairing>, HwmonError> {
    // Reset progress
    match AUTO_DETECT_PROGRESS.lock() {
        Ok(mut guard) => *guard = 0.0,
        Err(poisoned) => *poisoned.into_inner() = 0.0,
    }
    // Clear any previous cancellation requests
    clear_cancel_autodetect();
    
    let result = auto_detect_pairings_internal();
    
    // Set progress to 100% when done
    match AUTO_DETECT_PROGRESS.lock() {
        Ok(mut guard) => *guard = 1.0,
        Err(poisoned) => *poisoned.into_inner() = 1.0,
    }
    
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;
    use std::io::Write;

    fn create_test_chip_readings() -> ChipReadings {
        ChipReadings {
            name: "test_chip@hwmon0".to_string(),
            temps: vec![
                ("temp1".to_string(), 45.5),
                ("temp2".to_string(), 38.2),
            ],
            fans: vec![
                ("fan1".to_string(), 1200),
                ("fan2".to_string(), 800),
            ],
            pwms: vec![
                ("pwm1".to_string(), 128),
                ("pwm2".to_string(), 200),
            ],
        }
    }

    fn create_test_sensor_inventory() -> SensorInventory {
        SensorInventory {
            fans: vec![
                ("chip1@hwmon0".to_string(), 1, "fan1".to_string()),
                ("chip1@hwmon0".to_string(), 2, "fan2".to_string()),
            ],
            pwms: vec![
                ("chip1@hwmon0".to_string(), 1, "pwm1".to_string()),
                ("chip1@hwmon0".to_string(), 2, "pwm2".to_string()),
            ],
            temps: vec![
                ("chip1@hwmon0".to_string(), 1, "temp1".to_string()),
                ("chip1@hwmon0".to_string(), 2, "temp2".to_string()),
            ],
        }
    }

    fn create_test_fan_pwm_pairing() -> FanPwmPairing {
        FanPwmPairing {
            fan_chip: "chip1@hwmon0".to_string(),
            fan_idx: 1,
            fan_label: "fan1".to_string(),
            pwm_chip: "chip1@hwmon0".to_string(),
            pwm_idx: 1,
            pwm_label: "pwm1".to_string(),
            confidence: 0.85,
        }
    }

    #[test]
    fn test_extract_index_valid() {
        assert_eq!(extract_index("fan1_input", "fan", "_input"), Some(1));
        assert_eq!(extract_index("fan12_input", "fan", "_input"), Some(12));
        assert_eq!(extract_index("pwm3", "pwm", ""), Some(3));
        assert_eq!(extract_index("temp2_label", "temp", "_label"), Some(2));
    }

    #[test]
    fn test_extract_index_invalid() {
        assert_eq!(extract_index("fan_input", "fan", "_input"), None);
        assert_eq!(extract_index("fan1_output", "fan", "_input"), None);
        assert_eq!(extract_index("temp1_input", "fan", "_input"), None);
        assert_eq!(extract_index("fanabc_input", "fan", "_input"), None);
        assert_eq!(extract_index("", "fan", "_input"), None);
    }

    #[test]
    fn test_extract_index_edge_cases() {
        assert_eq!(extract_index("fan0_input", "fan", "_input"), Some(0));
        assert_eq!(extract_index("fan999_input", "fan", "_input"), Some(999));
        assert_eq!(extract_index("pwm1", "pwm", ""), Some(1));
        assert_eq!(extract_index("pwm", "pwm", ""), None);
    }

    #[test]
    fn test_chip_readings_creation() {
        let readings = create_test_chip_readings();
        
        assert_eq!(readings.name, "test_chip@hwmon0");
        assert_eq!(readings.temps.len(), 2);
        assert_eq!(readings.fans.len(), 2);
        assert_eq!(readings.pwms.len(), 2);
        
        assert_eq!(readings.temps[0], ("temp1".to_string(), 45.5));
        assert_eq!(readings.fans[0], ("fan1".to_string(), 1200));
        assert_eq!(readings.pwms[0], ("pwm1".to_string(), 128));
    }

    #[test]
    fn test_sensor_inventory_creation() {
        let inventory = create_test_sensor_inventory();
        
        assert_eq!(inventory.fans.len(), 2);
        assert_eq!(inventory.pwms.len(), 2);
        assert_eq!(inventory.temps.len(), 2);
        
        assert_eq!(inventory.fans[0], ("chip1@hwmon0".to_string(), 1, "fan1".to_string()));
        assert_eq!(inventory.pwms[0], ("chip1@hwmon0".to_string(), 1, "pwm1".to_string()));
        assert_eq!(inventory.temps[0], ("chip1@hwmon0".to_string(), 1, "temp1".to_string()));
    }

    #[test]
    fn test_fan_pwm_pairing_creation() {
        let pairing = create_test_fan_pwm_pairing();
        
        assert_eq!(pairing.fan_chip, "chip1@hwmon0");
        assert_eq!(pairing.fan_idx, 1);
        assert_eq!(pairing.fan_label, "fan1");
        assert_eq!(pairing.pwm_chip, "chip1@hwmon0");
        assert_eq!(pairing.pwm_idx, 1);
        assert_eq!(pairing.pwm_label, "pwm1");
        assert_eq!(pairing.confidence, 0.85);
    }

    #[test]
    fn test_hwmon_error_display() {
        let io_err = HwmonError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "test"));
        assert!(format!("{}", io_err).contains("IO error"));
        
        let parse_err = HwmonError::Parse("test parse error".to_string());
        assert_eq!(format!("{}", parse_err), "Parse error: test parse error");
        
        let invalid_err = HwmonError::InvalidData("test invalid data".to_string());
        assert_eq!(format!("{}", invalid_err), "Invalid data: test invalid data");
        
        let perm_err = HwmonError::PermissionDenied;
        assert_eq!(format!("{}", perm_err), "Permission denied - need root");
    }

    #[test]
    fn test_hwmon_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "test");
        let hwmon_err: HwmonError = io_err.into();
        assert!(matches!(hwmon_err, HwmonError::Io(_)));
    }

    #[test]
    fn test_read_trimmed_mock() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.txt");
        
        // Write test data with whitespace
        let mut file = fs::File::create(&test_file).unwrap();
        writeln!(file, "  test content  ").unwrap();
        
        let result = read_trimmed(&test_file).unwrap();
        assert_eq!(result, "test content");
    }

    #[test]
    fn test_read_trimmed_empty_file() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("empty.txt");
        
        fs::File::create(&test_file).unwrap();
        
        let result = read_trimmed(&test_file).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_read_trimmed_nonexistent_file() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("nonexistent.txt");
        
        let result = read_trimmed(&test_file);
        assert!(result.is_err());
    }

    #[test]
    fn test_auto_detect_progress_functions() {
        // Test progress tracking
        {
            let mut guard = AUTO_DETECT_PROGRESS.lock().unwrap();
            *guard = 0.5;
        }
        assert_eq!(get_auto_detect_progress(), 0.5);
        
        // Test cancellation
        clear_cancel_autodetect();
        assert!(!is_cancelled());
        
        request_cancel_autodetect();
        assert!(is_cancelled());
        
        clear_cancel_autodetect();
        assert!(!is_cancelled());
    }

    // Note: Many hwmon functions like read_all(), write_pwm(), auto_detect_pairings()
    // are difficult to test in isolation because they interact with the /sys filesystem.
    // In a production test suite, these would require:
    // 1. Mock filesystem implementations
    // 2. Integration tests with actual hardware
    // 3. Dependency injection to make filesystem access testable
    // 4. Test containers with simulated hwmon interfaces

    #[test]
    fn test_find_pwm_index_by_label_fallback() {
        // Test the fallback logic for "pwmN" format labels
        // This test assumes the function handles the case where no chip is found gracefully
        let result = find_pwm_index_by_label("nonexistent_chip", "pwm1");
        assert_eq!(result, None);
        
        let result = find_pwm_index_by_label("nonexistent_chip", "invalid_label");
        assert_eq!(result, None);
    }

    #[test]
    fn test_read_single_fan_error_cases() {
        // Test error cases for non-existent chip
        let result = read_single_fan("nonexistent_chip", 1);
        assert!(result.is_err());
        
        match result.unwrap_err() {
            HwmonError::InvalidData(msg) => {
                assert!(msg.contains("Chip") && msg.contains("not found"));
            }
            _ => panic!("Expected InvalidData error"),
        }
    }

    #[test]
    fn test_write_pwm_error_cases() {
        // Test error cases for non-existent chip
        let result = write_pwm("nonexistent_chip", 1, 128);
        assert!(result.is_err());
        
        match result.unwrap_err() {
            HwmonError::InvalidData(msg) => {
                assert!(msg.contains("Chip") && msg.contains("not found"));
            }
            _ => panic!("Expected InvalidData error"),
        }
    }
}

pub fn get_auto_detect_progress() -> f64 {
    match AUTO_DETECT_PROGRESS.lock() {
        Ok(guard) => *guard,
        Err(poisoned) => *poisoned.into_inner(),
    }
}

fn auto_detect_pairings_internal() -> Result<Vec<FanPwmPairing>, HwmonError> {
    // Enumerate ALL sensors first (fans, PWMs, temps) for full coverage
    let inventory = enumerate_all_sensors();
    let all_fans = inventory.fans.clone();
    let all_pwms = inventory.pwms.clone();

    let pwm_controllers = all_pwms.clone();
    let total_pwms = pwm_controllers.len().max(1);

    // Debug: log discovered inventory to help diagnose missing PWMs
    log_autodetect_line(&format!(
        "auto-detect: discovered {} fan(s), {} pwm controller(s), {} temp sensor(s)",
        all_fans.len(), pwm_controllers.len(), inventory.temps.len()
    ));
    for (chip, idx, label) in &pwm_controllers {
        log_autodetect_line(&format!("auto-detect: PWM found: {}:{} (label='{}')", chip, idx, label));
    }

    // Capture original state for all PWMs and ramp them to 100% for safety during detection
    let mut global_originals: Vec<(String, usize, Option<(u32, Option<u8>)>)> = Vec::new();
    for (chip, idx, _label) in &pwm_controllers {
        let orig = read_pwm_state(chip, *idx).ok();
        match write_pwm(chip, *idx, 255) {
            Ok(_) => log_autodetect_line(&format!(
                "auto-detect: ramped {}:{} to 100% for baseline",
                chip, idx
            )),
            Err(e) => log_autodetect_line(&format!(
                "auto-detect: WARN failed to set {}:{} to 100%: {}",
                chip, idx, e
            )),
        }
        global_originals.push((chip.clone(), *idx, orig));
    }

    // Determine a conservative dwell based on available update interval info
    let mut dwell_ms_global: u64 = 2000;
    for (chip, _idx, _label) in &pwm_controllers {
        if let Some(ms) = read_update_interval(chip) {
            dwell_ms_global = dwell_ms_global.max(ms.saturating_mul(2).clamp(800, 4000));
        }
    }
    logger::log_event(
        "autodetect_dwell",
        json!({ "dwell_ms_global": dwell_ms_global, "pwm_count": pwm_controllers.len(), "fan_count": all_fans.len() }),
    );

    // Let fans spin up at full before measuring baseline
    if is_cancelled() { /* no-op */ } else { thread::sleep(Duration::from_millis(dwell_ms_global)); }

    // Baseline fan RPMs with all PWMs at 100%
    let mut baselines: Vec<((String, usize, String), u64)> = Vec::new();
    for (fan_chip, fan_idx, fan_label) in &all_fans {
        if is_cancelled() { break; }
        if let Ok(rpm) = read_single_fan(fan_chip, *fan_idx) {
            baselines.push(((fan_chip.clone(), *fan_idx, fan_label.clone()), rpm));
        }
    }
    logger::log_event("autodetect_baseline", json!({ "baseline_count": baselines.len() }));

    let mut pairings = Vec::new();
    // Collect all candidate edges across all PWM tests: (pwm_chip, pwm_idx, pwm_label, fan_chip, fan_idx, fan_label, confidence)
    let mut all_edges: Vec<(String, usize, String, String, usize, String, f64)> = Vec::new();

    // Test each PWM controller by pulling it to 0 and watching for drops from baseline
    'pwm_loop: for (test_idx, (pwm_chip, pwm_idx, pwm_label)) in pwm_controllers.iter().enumerate() {
        if is_cancelled() { break 'pwm_loop; }
        // Update progress at start of each PWM
        let progress = (test_idx as f64) / (total_pwms as f64);
        match AUTO_DETECT_PROGRESS.lock() {
            Ok(mut guard) => *guard = progress,
            Err(poisoned) => *poisoned.into_inner() = progress,
        }

        // Set this PWM to 0 while leaving others at 100
        log_autodetect_line(&format!(
            "auto-detect: TEST begin PWM {}:{} (label='{}')",
            pwm_chip, pwm_idx, pwm_label
        ));
        if let Err(e) = write_pwm(pwm_chip, *pwm_idx, 0) {
            log_autodetect_line(&format!(
                "auto-detect: ERROR failed to write 0 to PWM {}:{}: {}",
                pwm_chip, pwm_idx, e
            ));
            continue;
        }
        if is_cancelled() { break 'pwm_loop; }
        thread::sleep(Duration::from_millis(dwell_ms_global.max(3000)));

        // Snapshot all fans and compute drops vs baseline
        let mut fan_responses = Vec::new();
        for (fan_chip, fan_idx, fan_label) in &all_fans {
            if is_cancelled() { break 'pwm_loop; }
            if let Ok(curr) = read_single_fan(fan_chip, *fan_idx) {
                // Find baseline for this fan
                if let Some((_, base)) = baselines.iter().find(|((c, i, _), _)| c == fan_chip && *i == *fan_idx) {
                    let basef = (*base as f64).max(1.0);
                    let drop_ratio = ((*base as f64) - (curr as f64)) / basef;
                    // Require at least 20% drop and some absolute change to avoid jitter
                    if drop_ratio >= 0.20 && (*base as i64 - curr as i64).abs() >= 200 {
                        let mut confidence = (drop_ratio * 1.5).min(1.0); // scale into 0..1
                        // same-chip small bonus
                        let pwm_base = pwm_chip.split('@').next().unwrap_or("");
                        let fan_base = fan_chip.split('@').next().unwrap_or("");
                        if pwm_base == fan_base { confidence = (confidence + 0.05).min(1.0); }
                        fan_responses.push((fan_chip.clone(), *fan_idx, fan_label.clone(), confidence));
                        log_autodetect_line(&format!(
                            "auto-detect: PWM {}:{}=0 -> Fan {}:{} base={} curr={} drop={:.0}% conf={:.2}",
                            pwm_chip, pwm_idx, fan_chip, fan_idx, *base, curr, drop_ratio * 100.0, confidence
                        ));
                    }
                }
            }
        }

        // Verify low-confidence candidates by waiting longer at 0 and re-sampling
        let mut verified: Vec<(String, usize, String, f64)> = Vec::new();
        for (fan_chip, fan_idx, fan_label, mut confidence) in fan_responses.into_iter() {
            if confidence < 0.80 {
                if is_cancelled() { break 'pwm_loop; }
                thread::sleep(Duration::from_millis(dwell_ms_global.max(3000)));
                if let Some((_, base)) = baselines.iter().find(|((c, i, _), _)| c == &fan_chip && *i == fan_idx) {
                    if let Ok(curr2) = read_single_fan(&fan_chip, fan_idx) {
                        let basef = (*base as f64).max(1.0);
                        let drop_ratio2 = ((*base as f64) - (curr2 as f64)) / basef;
                        if drop_ratio2 >= 0.20 && (*base as i64 - curr2 as i64).abs() >= 200 {
                            let conf2 = (drop_ratio2 * 1.8).min(1.0);
                            confidence = confidence.max(conf2);
                            log_autodetect_line(&format!(
                                "auto-detect: VERIFY PWM {}:{}=0 -> Fan {}:{} base={} curr2={} drop2={:.0}% conf->={:.2}",
                                pwm_chip, pwm_idx, fan_chip, fan_idx, *base, curr2, drop_ratio2 * 100.0, confidence
                            ));
                        } else {
                            // Discard weak candidate
                            continue;
                        }
                    }
                }
            }
            verified.push((fan_chip, fan_idx, fan_label, confidence));
        }

        // Restore this PWM to 100 before proceeding
        if let Err(e) = write_pwm(pwm_chip, *pwm_idx, 255) {
            log_autodetect_line(&format!(
                "auto-detect: WARN failed to restore PWM {}:{} to 100%: {}",
                pwm_chip, pwm_idx, e
            ));
        }
        log_autodetect_line(&format!(
            "auto-detect: TEST end PWM {}:{} (label='{}')",
            pwm_chip, pwm_idx, pwm_label
        ));

        // Store edges
        for (fan_chip, fan_idx, fan_label, confidence) in verified.into_iter() {
            if confidence > 0.20 {
                all_edges.push((
                    pwm_chip.clone(), *pwm_idx, pwm_label.clone(),
                    fan_chip, fan_idx, fan_label,
                    confidence,
                ));
            }
        }
    }

    // If cancelled, restore all PWM states immediately before computing final mapping
    if is_cancelled() {
        for (chip, idx, orig) in &global_originals {
            if let Some((val, en)) = orig {
                let _ = restore_pwm_state(chip, *idx, *val, *en);
            }
        }
    }

    // Fallback: if we detected nothing, relax thresholds and run a targeted pulse verify
    if all_edges.is_empty() && !is_cancelled() {
        log_autodetect_line("auto-detect: fallback engaged: relaxing thresholds and pulsing");
        for (pwm_chip, pwm_idx, pwm_label) in &pwm_controllers {
            if is_cancelled() { break; }
            // Pull to 0 again
            let _ = write_pwm(pwm_chip, *pwm_idx, 0);
            thread::sleep(Duration::from_millis(dwell_ms_global.saturating_mul(2).max(3000)));
            for (fan_chip, fan_idx, fan_label) in &all_fans {
                if is_cancelled() { break; }
                if let Some((_, base)) = baselines.iter().find(|((c, i, _), _)| c == fan_chip && *i == *fan_idx) {
                    if let Ok(curr) = read_single_fan(fan_chip, *fan_idx) {
                        let basef = (*base as f64).max(1.0);
                        let drop_ratio = ((*base as f64) - (curr as f64)) / basef;
                        // Relaxed thresholds
                        if drop_ratio >= 0.10 && (*base as i64 - curr as i64).abs() >= 100 {
                            // Pulse 0→100→0 just for this PWM to confirm
                            let _ = write_pwm(pwm_chip, *pwm_idx, 255);
                            thread::sleep(Duration::from_millis(dwell_ms_global.max(3000)));
                            let _ = write_pwm(pwm_chip, *pwm_idx, 0);
                            thread::sleep(Duration::from_millis(dwell_ms_global.max(3000)));
                            if let Ok(curr2) = read_single_fan(fan_chip, *fan_idx) {
                                let drop_ratio2 = ((*base as f64) - (curr2 as f64)) / basef;
                                if drop_ratio2 >= 0.10 && (*base as i64 - curr2 as i64).abs() >= 100 {
                                    let mut confidence = ((drop_ratio + drop_ratio2) * 0.9).min(1.0);
                                    let pwm_base = pwm_chip.split('@').next().unwrap_or("");
                                    let fan_base = fan_chip.split('@').next().unwrap_or("");
                                    if pwm_base == fan_base { confidence = (confidence + 0.05).min(1.0); }
                                    log_autodetect_line(&format!(
                                        "auto-detect: FALLBACK accepted PWM {}:{} -> Fan {}:{} base={} curr={} curr2={} drop={:.0}%/{:.0}% conf={:.2}",
                                        pwm_chip, pwm_idx, fan_chip, fan_idx, *base, curr, curr2, drop_ratio*100.0, drop_ratio2*100.0, confidence
                                    ));
                                    all_edges.push((
                                        pwm_chip.clone(), *pwm_idx, pwm_label.clone(),
                                        fan_chip.clone(), *fan_idx, fan_label.clone(),
                                        confidence,
                                    ));
                                }
                            }
                        }
                    }
                }
            }
            // Return PWM to 100 between tests
            let _ = write_pwm(pwm_chip, *pwm_idx, 255);
        }
    }

    // Global greedy maximum-weight matching to ensure one-to-one PWM<->Fan mapping
    if !all_edges.is_empty() {
        // Sort edges by confidence descending
        all_edges.sort_by(|a, b| b.6.partial_cmp(&a.6).unwrap_or(std::cmp::Ordering::Equal));
        use std::collections::HashSet;
        let mut used_pwms: HashSet<(String, usize)> = HashSet::new();
        let mut used_fans: HashSet<(String, usize)> = HashSet::new();

        for (pwm_chip, pwm_idx, pwm_label, fan_chip, fan_idx, fan_label, confidence) in all_edges {
            let pwm_key = (pwm_chip.clone(), pwm_idx);
            let fan_key = (fan_chip.clone(), fan_idx);
            if used_pwms.contains(&pwm_key) || used_fans.contains(&fan_key) { continue; }
            // final acceptance threshold and minimal margin not directly needed in greedy; rely on confidence cutoff
            if confidence > 0.25 {
                pairings.push(FanPwmPairing {
                    fan_chip: fan_chip.clone(),
                    fan_idx,
                    fan_label: fan_label.clone(),
                    pwm_chip: pwm_chip.clone(),
                    pwm_idx,
                    pwm_label: pwm_label.clone(),
                    confidence,
                });
                used_pwms.insert(pwm_key);
                used_fans.insert(fan_key);
            }
        }
    }

    // Secondary probing for any unmatched PWMs: pulse them with longer dwell and find strongest responding fan
    // Build sets of already paired keys
    let used_pwm_keys: HashSet<(String, usize)> = pairings.iter().map(|p| (p.pwm_chip.clone(), p.pwm_idx)).collect();
    let used_fan_keys: HashSet<(String, usize)> = pairings.iter().map(|p| (p.fan_chip.clone(), p.fan_idx)).collect();

    // Unmatched controllers
    let unmatched_pwms: Vec<(String, usize, String)> = pwm_controllers
        .iter()
        .filter(|(c, i, _)| !used_pwm_keys.contains(&(c.clone(), *i)))
        .cloned()
        .collect();

    if !unmatched_pwms.is_empty() {
        log_autodetect_line(&format!(
            "auto-detect: entering secondary probing for {} unmatched PWM(s)",
            unmatched_pwms.len()
        ));
    }

    // Take a fresh baseline at 100%
    let mut base_map: Vec<((String, usize, String), u64)> = Vec::new();
    for (fan_chip, fan_idx, fan_label) in &all_fans {
        if let Ok(rpm) = read_single_fan(fan_chip, *fan_idx) {
            base_map.push(((fan_chip.clone(), *fan_idx, fan_label.clone()), rpm));
        }
    }

    for (secondary_idx, (pwm_chip, pwm_idx, pwm_label)) in unmatched_pwms.iter().enumerate() {
        if is_cancelled() { 
            log_autodetect_line("auto-detect: SECONDARY cancelled by user");
            break; 
        }
        // Update progress for secondary pass (continue from where main pass left off)
        let secondary_progress = (total_pwms as f64 + secondary_idx as f64) / (total_pwms as f64 + unmatched_pwms.len() as f64);
        match AUTO_DETECT_PROGRESS.lock() {
            Ok(mut guard) => *guard = secondary_progress,
            Err(poisoned) => *poisoned.into_inner() = secondary_progress,
        }
        
        // Skip if PWM got paired meanwhile (defensive)
        if pairings.iter().any(|p| p.pwm_chip == *pwm_chip && p.pwm_idx == *pwm_idx) { continue; }

        log_autodetect_line(&format!(
            "auto-detect: SECONDARY testing PWM {}:{} (label='{}')",
            pwm_chip, pwm_idx, pwm_label
        ));
        
        // Pulse 0/100 for a few cycles with generous dwell
        let cycles = 2usize;  // Reduced from 3 to speed up
        let dwell_low = dwell_ms_global.saturating_mul(2).max(3000).min(4000);  // Cap at 4s
        let dwell_high = dwell_ms_global.max(1500).min(2500);  // Cap at 2.5s

        // Track strongest drop per fan across cycles
        let mut best_fan: Option<(String, usize, String, f64, i64)> = None; // chip, idx, label, drop_ratio, abs_drop

        for cycle in 0..cycles {
            if is_cancelled() { 
                log_autodetect_line(&format!("auto-detect: SECONDARY cancelled during cycle {} for PWM {}:{}", cycle, pwm_chip, pwm_idx));
                break; 
            }
            // Pull low
            if let Err(e) = write_pwm(&pwm_chip, *pwm_idx, 0) {
                log_autodetect_line(&format!(
                    "auto-detect: SECONDARY failed to write 0 to PWM {}:{}: {}",
                    pwm_chip, pwm_idx, e
                ));
                break;
            }
            thread::sleep(Duration::from_millis(dwell_low));
            // Sample drops
            for (fan_chip, fan_idx, fan_label) in &all_fans {
                if used_fan_keys.contains(&(fan_chip.clone(), *fan_idx)) { continue; }
                if let Some((_, base)) = base_map.iter().find(|((c, i, _), _)| c == fan_chip && *i == *fan_idx) {
                    if let Ok(curr) = read_single_fan(fan_chip, *fan_idx) {
                        let abs_drop = *base as i64 - curr as i64;
                        let basef = (*base as f64).max(1.0);
                        let drop_ratio = ((*base as f64) - (curr as f64)) / basef;
                        // Relaxed thresholds for secondary pass
                        if drop_ratio >= 0.08 && abs_drop >= 80 {
                            match &mut best_fan {
                                Some((bf_c, bf_i, bf_l, bf_r, bf_abs)) => {
                                    if drop_ratio > *bf_r {
                                        *bf_c = fan_chip.clone();
                                        *bf_i = *fan_idx;
                                        *bf_l = fan_label.clone();
                                        *bf_r = drop_ratio;
                                        *bf_abs = abs_drop;
                                    }
                                }
                                None => {
                                    best_fan = Some((fan_chip.clone(), *fan_idx, fan_label.clone(), drop_ratio, abs_drop));
                                }
                            }
                        }
                    }
                }
            }
            // Return high
            if let Err(e) = write_pwm(&pwm_chip, *pwm_idx, 255) {
                log_autodetect_line(&format!(
                    "auto-detect: SECONDARY failed to restore PWM {}:{}: {}",
                    pwm_chip, pwm_idx, e
                ));
            }
            if cycle < cycles - 1 {  // Don't sleep after last cycle
                thread::sleep(Duration::from_millis(dwell_high));
            }
        }

        if let Some((fan_chip, fan_idx, fan_label, drop_ratio, abs_drop)) = best_fan {
            // Only accept if not already used and still meets a modest bar
            if !pairings.iter().any(|p| p.fan_chip == fan_chip && p.fan_idx == fan_idx) {
                // Confidence scaled from drop ratio, small same-chip bonus
                let mut confidence = (drop_ratio * 1.2).min(1.0);
                let pwm_base = pwm_chip.split('@').next().unwrap_or("");
                let fan_base = fan_chip.split('@').next().unwrap_or("");
                if pwm_base == fan_base { confidence = (confidence + 0.05).min(1.0); }
                log_autodetect_line(&format!(
                    "auto-detect: SECONDARY accepted PWM {}:{} -> Fan {}:{} drop={:.0}% ({} RPM) conf={:.2}",
                    pwm_chip, *pwm_idx, fan_chip, fan_idx, drop_ratio * 100.0, abs_drop, confidence
                ));
                pairings.push(FanPwmPairing {
                    fan_chip,
                    fan_idx,
                    fan_label,
                    pwm_chip: pwm_chip.clone(),
                    pwm_idx: *pwm_idx,
                    pwm_label: pwm_label.clone(),
                    confidence,
                });
            }
        } else {
            log_autodetect_line(&format!(
                "auto-detect: SECONDARY no response for PWM {}:{} (label='{}')",
                pwm_chip, *pwm_idx, pwm_label
            ));
        }
    }
    
    // Final progress update
    match AUTO_DETECT_PROGRESS.lock() {
        Ok(mut guard) => *guard = 1.0,
        Err(poisoned) => *poisoned.into_inner() = 1.0,
    }

    // Always restore PWM states to their original values/modes before returning
    for (chip, idx, orig) in &global_originals {
        if let Some((val, en)) = orig {
            let _ = restore_pwm_state(chip, *idx, *val, *en);
        }
    }

    // Final summary event for autodetect
    logger::log_event(
        "autodetect_result",
        json!({
            "pairings": pairings.iter().map(|p| json!({
                "fan_chip": p.fan_chip,
                "fan_idx": p.fan_idx,
                "pwm_chip": p.pwm_chip,
                "pwm_idx": p.pwm_idx,
                "confidence": p.confidence,
            })).collect::<Vec<_>>(),
        }),
    );

    Ok(pairings)
}

// Keep the original function for backward compatibility
pub fn auto_detect_pairings() -> Result<Vec<FanPwmPairing>, HwmonError> {
    auto_detect_pairings_internal()
}

#[allow(dead_code)]
pub mod pwm {
    use super::*;

    pub fn list_pwms(_chip_dir: &Path) -> Vec<PathBuf> {
        // List pwmN files. Writing to pwmN and pwmN_enable can control fans.
        Vec::new()
    }
}
