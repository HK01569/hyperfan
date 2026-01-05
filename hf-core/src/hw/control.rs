//! PWM and sensor control functions
//!
//! Low-level read/write operations for fan speed control.
//!
//! # PWM Values
//!
//! PWM (Pulse Width Modulation) values range from 0 to 255:
//! - 0 = fan off (or minimum speed on some fans)
//! - 255 = full speed
//!
//! # Temperature Values
//!
//! Linux hwmon reports temperatures in millidegrees Celsius.
//! We convert to standard Celsius for user-facing values.

use crate::error::Result;
use std::fs;
use std::path::Path;

use crate::constants::{pwm, temperature};

/// Set PWM value directly (0-255)
///
/// # Arguments
/// * `pwm_path` - Path to the PWM control file (e.g., /sys/class/hwmon/hwmon0/pwm1)
/// * `value` - PWM value from 0 (off/min) to 255 (full speed)
pub fn set_pwm_value(pwm_path: &Path, value: u8) -> Result<()> {
    fs::write(pwm_path, value.to_string())
        .map_err(|e| crate::error::HyperfanError::PwmWrite { path: pwm_path.to_path_buf(), reason: format!("Failed to write PWM value {}: {}", value, e) })
}

/// Set PWM as percentage (0.0-100.0)
///
/// Converts percentage to PWM value using the formula: PWM = (percent / 100) * 255
pub fn set_pwm_percent(pwm_path: &Path, percent: f32) -> Result<()> {
    let pwm_value = pwm::from_percent(percent);
    set_pwm_value(pwm_path, pwm_value)
}

/// Enable manual PWM control mode
///
/// Writes "1" to the PWM enable file, which tells the hardware to accept
/// software-controlled PWM values instead of using automatic thermal control.
///
/// PWM enable modes:
/// - 0 = disabled (no PWM output)
/// - 1 = manual (software control)
/// - 2 = automatic (hardware thermal control)
pub fn enable_manual_pwm(enable_path: &Path) -> Result<()> {
    if enable_path.exists() {
        let manual_mode = pwm::enable::MANUAL.to_string();
        fs::write(enable_path, &manual_mode)
            .map_err(|e| crate::error::HyperfanError::PwmWrite { path: enable_path.to_path_buf(), reason: format!("Failed to enable manual PWM control: {}", e) })
    } else {
        Ok(()) // No enable file means manual control is always active
    }
}

/// Read current PWM value (0-255)
pub fn read_pwm_value(pwm_path: &Path) -> Result<u8> {
    let content = fs::read_to_string(pwm_path)
        .map_err(|e| crate::error::HyperfanError::PwmRead { path: pwm_path.to_path_buf(), reason: format!("Failed to read: {}", e) })?;

    content
        .trim()
        .parse::<u8>()
        .map_err(|e| crate::error::HyperfanError::PwmRead { path: pwm_path.to_path_buf(), reason: format!("Failed to parse '{}': {}", content.trim(), e) })
}

/// Read current fan speed in RPM
pub fn read_fan_rpm(fan_path: &Path) -> Result<u32> {
    let content = fs::read_to_string(fan_path)
        .map_err(|e| crate::error::HyperfanError::FanRead { path: fan_path.to_path_buf(), reason: format!("Failed to read: {}", e) })?;

    content
        .trim()
        .parse::<u32>()
        .map_err(|e| crate::error::HyperfanError::FanRead { path: fan_path.to_path_buf(), reason: format!("Failed to parse '{}': {}", content.trim(), e) })
}

/// Read temperature sensor value in degrees Celsius
///
/// Linux hwmon reports temperatures in millidegrees (e.g., 45000 = 45.0°C).
/// This function handles the conversion automatically.
pub fn read_temperature(temp_path: &Path) -> Result<f32> {
    let content = fs::read_to_string(temp_path)
        .map_err(|e| crate::error::HyperfanError::TemperatureRead { path: temp_path.to_path_buf(), reason: format!("Failed to read: {}", e) })?;

    let millidegrees = content
        .trim()
        .parse::<i32>()
        .map_err(|e| crate::error::HyperfanError::TemperatureRead { path: temp_path.to_path_buf(), reason: format!("Failed to parse '{}': {}", content.trim(), e) })?;

    // Convert millidegrees to degrees Celsius
    Ok(millidegrees as f32 / temperature::MILLIDEGREE_DIVISOR)
}
