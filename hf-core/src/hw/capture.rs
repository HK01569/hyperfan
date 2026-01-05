//! Raw controller data capture
//!
//! Functions for capturing snapshots of all hardware sensor data.
//! Used for debugging, diagnostics, and exporting sensor information.

use crate::error::Result;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, trace};

use crate::constants::{pwm, temperature};
use crate::data::{
    RawChipData, RawControllerSnapshot, RawFanReading, RawPwmReading, RawTempReading,
};
use crate::hw::enumerate_hwmon_chips;

/// Capture a complete snapshot of all controller data
pub fn capture_raw_snapshot() -> Result<RawControllerSnapshot> {
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let chips = enumerate_hwmon_chips()?;
    let mut raw_chips = Vec::with_capacity(chips.len());

    for chip in &chips {
        let raw_chip = capture_chip_data(&chip.path, &chip.name)?;
        raw_chips.push(raw_chip);
    }

    debug!(
        chips = raw_chips.len(),
        timestamp = timestamp_ms,
        "Captured raw controller snapshot"
    );

    Ok(RawControllerSnapshot {
        timestamp_ms,
        chips: raw_chips,
    })
}

/// Capture raw data from a single chip path
pub fn capture_chip_data(chip_path: &Path, chip_name: &str) -> Result<RawChipData> {
    let mut temperatures = Vec::new();
    let mut fans = Vec::new();
    let mut pwms = Vec::new();

    let entries = fs::read_dir(chip_path)?;
    let mut files: Vec<String> = Vec::new();

    for entry in entries {
        if let Ok(entry) = entry {
            files.push(entry.file_name().to_string_lossy().to_string());
        }
    }

    // Capture temperature sensors
    for file in &files {
        if file.starts_with("temp") && file.ends_with("_input") {
            if let Some(reading) = capture_temp_reading(chip_path, file)? {
                temperatures.push(reading);
            }
        }
    }

    // Capture fan sensors
    for file in &files {
        if file.starts_with("fan") && file.ends_with("_input") {
            if let Some(reading) = capture_fan_reading(chip_path, file)? {
                fans.push(reading);
            }
        }
    }

    // Capture PWM controllers
    for file in &files {
        if file.starts_with("pwm") && !file.contains('_') {
            if let Some(reading) = capture_pwm_reading(chip_path, file)? {
                pwms.push(reading);
            }
        }
    }

    trace!(
        chip = chip_name,
        temps = temperatures.len(),
        fans = fans.len(),
        pwms = pwms.len(),
        "Captured chip data"
    );

    Ok(RawChipData {
        chip_name: chip_name.to_string(),
        chip_path: chip_path.to_path_buf(),
        temperatures,
        fans,
        pwms,
    })
}

fn capture_temp_reading(chip_path: &Path, input_file: &str) -> Result<Option<RawTempReading>> {
    let input_path = chip_path.join(input_file);
    let base_name = input_file.replace("_input", "");
    let label_path = chip_path.join(format!("{}_label", base_name));

    let label = if label_path.exists() {
        fs::read_to_string(&label_path).ok().map(|s| s.trim().to_string())
    } else {
        None
    };

    let raw_value = fs::read_to_string(&input_path)
        .ok()
        .and_then(|s| s.trim().parse::<i32>().ok());

    // Convert from millidegrees to degrees Celsius
    let celsius = raw_value.map(|millidegrees| millidegrees as f32 / temperature::MILLIDEGREE_DIVISOR);

    Ok(Some(RawTempReading {
        sensor_name: base_name,
        sensor_path: input_path,
        label,
        raw_value,
        celsius,
    }))
}

fn capture_fan_reading(chip_path: &Path, input_file: &str) -> Result<Option<RawFanReading>> {
    let input_path = chip_path.join(input_file);
    let base_name = input_file.replace("_input", "");
    let label_path = chip_path.join(format!("{}_label", base_name));

    let label = if label_path.exists() {
        fs::read_to_string(&label_path).ok().map(|s| s.trim().to_string())
    } else {
        None
    };

    let rpm = fs::read_to_string(&input_path)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok());

    Ok(Some(RawFanReading {
        sensor_name: base_name,
        sensor_path: input_path,
        label,
        rpm,
    }))
}

fn capture_pwm_reading(chip_path: &Path, pwm_file: &str) -> Result<Option<RawPwmReading>> {
    let pwm_path = chip_path.join(pwm_file);
    let enable_path = chip_path.join(format!("{}_enable", pwm_file));
    let label_path = chip_path.join(format!("{}_label", pwm_file));

    if !pwm_path.exists() {
        return Ok(None);
    }

    let label = if label_path.exists() {
        fs::read_to_string(&label_path).ok().map(|s| s.trim().to_string())
    } else {
        None
    };

    let pwm_value = fs::read_to_string(&pwm_path)
        .ok()
        .and_then(|s| s.trim().parse::<u8>().ok());

    let enable_mode = if enable_path.exists() {
        fs::read_to_string(&enable_path)
            .ok()
            .and_then(|s| s.trim().parse::<u8>().ok())
    } else {
        None
    };

    let percent = pwm_value.map(|v| pwm::to_percent(v));

    Ok(Some(RawPwmReading {
        controller_name: pwm_file.to_string(),
        pwm_path,
        enable_path,
        label,
        pwm_value,
        enable_mode,
        percent,
    }))
}

/// Export snapshot as JSON string
pub fn snapshot_to_json(snapshot: &RawControllerSnapshot) -> Result<String> {
    Ok(serde_json::to_string_pretty(snapshot)?)
}

/// Export snapshot as compact JSON string
pub fn snapshot_to_json_compact(snapshot: &RawControllerSnapshot) -> Result<String> {
    Ok(serde_json::to_string(snapshot)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capture_snapshot() {
        // This test will pass even on systems without hwmon
        let result = capture_raw_snapshot();
        assert!(result.is_ok());
    }
}
