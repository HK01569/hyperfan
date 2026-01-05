//! Input validation and sanitization for Hyperfan
//!
//! Provides security-focused validation for all user inputs and file paths.
//!
//! # Security Considerations
//!
//! - **Path Traversal Protection**: All paths are validated to be within allowed directories
//! - **Size Limits**: File sizes are checked to prevent memory exhaustion attacks
//! - **Input Sanitization**: User-provided names are filtered for safe characters only
//!
//! Always validate untrusted input before using it in file operations or storing it.

use std::path::{Path, PathBuf};

use crate::constants::{limits, pwm};
use crate::error::{HyperfanError, Result};

/// Validates that a PWM value is within the valid range (0-255)
pub fn validate_pwm_value(value: u16) -> Result<u8> {
    if value > pwm::MAX_VALUE as u16 {
        return Err(HyperfanError::InvalidPwmValue { value });
    }
    Ok(value as u8)
}

/// Validates that a percentage is within the valid range (0.0-100.0)
pub fn validate_percentage(value: f32) -> Result<f32> {
    if !(0.0..=100.0).contains(&value) {
        return Err(HyperfanError::InvalidPercentage { value });
    }
    Ok(value)
}

/// Validates and sanitizes a PWM path to prevent path traversal attacks
pub fn validate_pwm_path(path: &Path) -> Result<PathBuf> {
    let canonical = validate_hwmon_path(path)?;

    let filename = canonical
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| HyperfanError::invalid_path(&canonical, "invalid filename"))?;

    if !filename.starts_with("pwm") {
        return Err(HyperfanError::invalid_path(
            &canonical,
            "not a PWM control file",
        ));
    }

    Ok(canonical)
}

/// Validates and sanitizes a temperature sensor path
pub fn validate_temp_path(path: &Path) -> Result<PathBuf> {
    let canonical = validate_hwmon_path(path)?;

    let filename = canonical
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| HyperfanError::invalid_path(&canonical, "invalid filename"))?;

    if !filename.starts_with("temp") || !filename.contains("_input") {
        return Err(HyperfanError::invalid_path(
            &canonical,
            "not a temperature sensor file",
        ));
    }

    Ok(canonical)
}

/// Validates and sanitizes a fan sensor path
pub fn validate_fan_path(path: &Path) -> Result<PathBuf> {
    let canonical = validate_hwmon_path(path)?;

    let filename = canonical
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| HyperfanError::invalid_path(&canonical, "invalid filename"))?;

    if !filename.starts_with("fan") || !filename.contains("_input") {
        return Err(HyperfanError::invalid_path(
            &canonical,
            "not a fan sensor file",
        ));
    }

    Ok(canonical)
}

/// Validates that a path is within the hwmon directory structure
fn validate_hwmon_path(path: &Path) -> Result<PathBuf> {
    let canonical = path.canonicalize().map_err(|e| {
        HyperfanError::invalid_path(path, format!("path resolution failed: {}", e))
    })?;

    let path_str = canonical.to_string_lossy();
    if !path_str.starts_with("/sys/class/hwmon") && !path_str.starts_with("/sys/devices") {
        return Err(HyperfanError::invalid_path(
            &canonical,
            "path must be under /sys/class/hwmon or /sys/devices",
        ));
    }

    if path_str.contains("..") {
        return Err(HyperfanError::invalid_path(
            &canonical,
            "path traversal detected",
        ));
    }

    Ok(canonical)
}

/// Validates a sensor name for storage
pub fn validate_sensor_name(name: &str) -> Result<String> {
    let trimmed = name.trim();

    if trimmed.is_empty() {
        return Err(HyperfanError::config("sensor name cannot be empty"));
    }

    if trimmed.len() > limits::MAX_SENSOR_NAME_LEN {
        return Err(HyperfanError::config(format!(
            "sensor name exceeds maximum length of {} characters",
            limits::MAX_SENSOR_NAME_LEN
        )));
    }

    let sanitized: String = trimmed
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_')
        .collect();

    if sanitized.is_empty() {
        return Err(HyperfanError::config(
            "sensor name contains no valid characters",
        ));
    }

    Ok(sanitized)
}

/// Validates curve points for consistency
pub fn validate_curve_points(points: &[crate::data::CurvePoint]) -> Result<()> {
    if points.is_empty() {
        return Err(HyperfanError::config("curve must have at least one point"));
    }

    if points.len() > limits::MAX_CURVE_POINTS {
        return Err(HyperfanError::config(format!(
            "curve exceeds maximum of {} points",
            limits::MAX_CURVE_POINTS
        )));
    }

    for (point_index, point) in points.iter().enumerate() {
        // Validate temperature is within reasonable range
        if !(0.0..=limits::MAX_CURVE_TEMPERATURE).contains(&point.temperature) {
            return Err(HyperfanError::config(format!(
                "curve point {} has invalid temperature: {:.1}°C (must be 0-{}°C)",
                point_index, point.temperature, limits::MAX_CURVE_TEMPERATURE
            )));
        }

        // Validate fan percentage
        validate_percentage(point.fan_percent)?;
    }

    for window in points.windows(2) {
        if window[0].temperature >= window[1].temperature {
            return Err(HyperfanError::config(
                "curve points must be sorted by ascending temperature",
            ));
        }
    }

    Ok(())
}

/// Validates profile configuration file size
pub fn validate_file_size(path: &Path) -> Result<()> {
    let metadata = std::fs::metadata(path).map_err(|e| {
        HyperfanError::config(format!("cannot read file metadata: {}", e))
    })?;

    if metadata.len() > limits::MAX_PROFILE_SIZE {
        return Err(HyperfanError::config(format!(
            "profile file exceeds maximum size of {} bytes",
            limits::MAX_PROFILE_SIZE
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_pwm_value() {
        assert!(validate_pwm_value(0).is_ok());
        assert!(validate_pwm_value(128).is_ok());
        assert!(validate_pwm_value(255).is_ok());
        assert!(validate_pwm_value(256).is_err());
        assert!(validate_pwm_value(1000).is_err());
    }

    #[test]
    fn test_validate_percentage() {
        assert!(validate_percentage(0.0).is_ok());
        assert!(validate_percentage(50.0).is_ok());
        assert!(validate_percentage(100.0).is_ok());
        assert!(validate_percentage(-1.0).is_err());
        assert!(validate_percentage(101.0).is_err());
    }

    #[test]
    fn test_validate_sensor_name() {
        assert!(validate_sensor_name("CPU Fan").is_ok());
        assert!(validate_sensor_name("gpu-temp_1").is_ok());
        assert!(validate_sensor_name("").is_err());
        assert!(validate_sensor_name("   ").is_err());
    }
}
