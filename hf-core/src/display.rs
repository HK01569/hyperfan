//! Display Formatting Helpers
//!
//! Centralized functions for formatting temperatures and fan values
//! based on user settings (°C/°F and %/PWM).
//!
//! These are GUI-agnostic formatting utilities that can be used by
//! any frontend (GTK, KDE, CLI, etc.).
//!
//! PERFORMANCE: Uses cached settings to avoid disk I/O on every format call.

use crate::constants::pwm;
use crate::settings::get_cached_settings;

/// Format a temperature value according to user settings
///
/// # Arguments
/// * `temp_celsius` - Temperature in Celsius
///
/// # Returns
/// Formatted string like "45°C" or "113°F"
pub fn format_temp(temp_celsius: f32) -> String {
    let settings = get_cached_settings();
    format_temp_with_unit(temp_celsius, &settings.display.temperature_unit)
}

/// Format temperature with explicit unit
pub fn format_temp_with_unit(temp_celsius: f32, unit: &str) -> String {
    if unit == "fahrenheit" {
        let fahrenheit = celsius_to_fahrenheit(temp_celsius);
        format!("{:.0}°F", fahrenheit)
    } else {
        format!("{:.0}°C", temp_celsius)
    }
}

/// Format temperature with one decimal place
pub fn format_temp_precise(temp_celsius: f32) -> String {
    let settings = get_cached_settings();
    format_temp_precise_with_unit(temp_celsius, &settings.display.temperature_unit)
}

/// Format temperature with one decimal place and explicit unit
pub fn format_temp_precise_with_unit(temp_celsius: f32, unit: &str) -> String {
    if unit == "fahrenheit" {
        let fahrenheit = celsius_to_fahrenheit(temp_celsius);
        format!("{:.1}°F", fahrenheit)
    } else {
        format!("{:.1}°C", temp_celsius)
    }
}

/// Get the temperature unit suffix based on settings
pub fn temp_unit_suffix() -> &'static str {
    let settings = get_cached_settings();
    if settings.display.temperature_unit == "fahrenheit" {
        "°F"
    } else {
        "°C"
    }
}

/// Convert Celsius to Fahrenheit
pub fn celsius_to_fahrenheit(celsius: f32) -> f32 {
    celsius * 9.0 / 5.0 + 32.0
}

/// Convert Fahrenheit to Celsius
pub fn fahrenheit_to_celsius(fahrenheit: f32) -> f32 {
    (fahrenheit - 32.0) * 5.0 / 9.0
}

/// Format a fan speed value according to user settings
///
/// # Arguments
/// * `percent` - Fan speed as percentage (0-100)
///
/// # Returns
/// Formatted string like "50%" or "128 PWM"
pub fn format_fan_speed(percent: u32) -> String {
    let settings = get_cached_settings();
    format_fan_speed_with_metric(percent, &settings.display.fan_control_metric)
}

/// Format fan speed with explicit metric
pub fn format_fan_speed_with_metric(percent: u32, metric: &str) -> String {
    if metric == "pwm" {
        let pwm = percent_to_pwm(percent);
        format!("{} PWM", pwm)
    } else {
        format!("{}%", percent)
    }
}

/// Format a fan speed from f32 percentage
pub fn format_fan_speed_f32(percent: f32) -> String {
    let settings = get_cached_settings();
    format_fan_speed_f32_with_metric(percent, &settings.display.fan_control_metric)
}

/// Format fan speed from f32 with explicit metric
pub fn format_fan_speed_f32_with_metric(percent: f32, metric: &str) -> String {
    if metric == "pwm" {
        let pwm_value = pwm::from_percent(percent) as u32;
        format!("{} PWM", pwm_value)
    } else {
        format!("{:.0}%", percent)
    }
}

/// Convert percentage (0-100) to PWM value (0-255)
pub fn percent_to_pwm(percent: u32) -> u32 {
    pwm::from_percent(percent as f32) as u32
}

/// Convert PWM value (0-255) to percentage (0-100)
pub fn pwm_to_percent(pwm_value: u32) -> u32 {
    pwm::to_percent(pwm_value as u8).round() as u32
}

/// Convert PWM value (0-255) to percentage (0-100) as f32
pub fn pwm_to_percent_f32(pwm_value: u8) -> f32 {
    pwm::to_percent(pwm_value)
}

/// Convert percentage (0-100) to PWM value (0-255) as u8
pub fn percent_to_pwm_u8(percent: f32) -> u8 {
    pwm::from_percent(percent)
}

/// Get the fan metric suffix based on settings
pub fn fan_metric_suffix() -> &'static str {
    let settings = get_cached_settings();
    if settings.display.fan_control_metric == "pwm" {
        " PWM"
    } else {
        "%"
    }
}

/// Check if using PWM metric
pub fn is_pwm_metric() -> bool {
    let settings = get_cached_settings();
    settings.display.fan_control_metric == "pwm"
}

/// Check if using Fahrenheit
pub fn is_fahrenheit() -> bool {
    let settings = get_cached_settings();
    settings.display.temperature_unit == "fahrenheit"
}

/// Format PWM value with subtitle showing both PWM and percentage
pub fn format_pwm_subtitle(pwm_value: u8, pwm_percent: f32) -> String {
    let settings = get_cached_settings();
    if settings.display.fan_control_metric == "pwm" {
        format!("PWM: {} ({}%)", pwm_value, pwm_percent as u32)
    } else {
        format!("{}% (PWM: {})", pwm_percent as u32, pwm_value)
    }
}

/// Format RPM value
pub fn format_rpm(rpm: u32) -> String {
    format!("{} RPM", rpm)
}

/// Format RPM value with optional suffix
pub fn format_rpm_optional(rpm: Option<u32>) -> String {
    match rpm {
        Some(r) => format!("{} RPM", r),
        None => "-- RPM".to_string(),
    }
}

/// Format power in watts
pub fn format_power(watts: f32) -> String {
    format!("{:.1}W", watts)
}

/// Format memory usage
pub fn format_memory_mb(used_mb: u32, total_mb: u32) -> String {
    format!("{}/{} MB", used_mb, total_mb)
}

/// Format utilization percentage
pub fn format_utilization(percent: u32) -> String {
    format!("{}%", percent)
}
