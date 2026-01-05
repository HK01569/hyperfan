//! Core data types for Hyperfan
//!
//! Defines all the primary data structures used throughout the application.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// System information summary
#[derive(Debug, Serialize)]
pub struct SystemSummary {
    pub hostname: String,
    pub kernel_version: String,
    pub cpu_model: String,
    pub cpu_cores: u32,
    pub memory_total_mb: u32,
    pub memory_available_mb: u32,
    pub motherboard_name: String,
}

/// Hardware monitoring chip with its sensors
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HwmonChip {
    pub name: String,
    pub path: PathBuf,
    pub temperatures: Vec<TemperatureSensor>,
    pub fans: Vec<FanSensor>,
    pub pwms: Vec<PwmController>,
}

/// Temperature sensor data
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TemperatureSensor {
    pub name: String,
    pub input_path: PathBuf,
    pub label: Option<String>,
    pub current_temp: Option<f32>,
}

/// Fan sensor data
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FanSensor {
    pub name: String,
    pub input_path: PathBuf,
    pub label: Option<String>,
    pub current_rpm: Option<u32>,
}

/// PWM controller for fan speed control
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PwmController {
    pub name: String,
    pub pwm_path: PathBuf,
    pub enable_path: PathBuf,
    pub label: Option<String>,
    pub current_value: Option<u8>,
    pub current_percent: Option<f32>,
}

/// Mapping between a fan and its PWM controller
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FanMapping {
    pub fan_name: String,
    pub pwm_name: String,
    pub confidence: f32,
    pub temp_sources: Vec<TempSource>,
    pub response_time_ms: Option<u32>,
    pub min_pwm: Option<u8>,
    pub max_rpm: Option<u32>,
}

/// Temperature source for fan curve control
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TempSource {
    pub sensor_path: PathBuf,
    pub sensor_name: String,
    pub sensor_label: Option<String>,
    pub current_temp: Option<f32>,
    pub chip_name: String,
}

/// Result from probing a PWM-fan relationship
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProbeResult {
    pub pwm_path: PathBuf,
    pub fan_path: PathBuf,
    pub baseline_rpm: u32,
    pub test_rpm: u32,
    pub rpm_delta: i32,
    pub response_time_ms: u32,
    pub confidence: f32,
}


/// A point on a fan curve
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CurvePoint {
    pub temperature: f32,
    pub fan_percent: f32,
}

/// Raw snapshot of all controller data at a point in time
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RawControllerSnapshot {
    /// Timestamp in milliseconds since Unix epoch
    pub timestamp_ms: u64,
    /// All chip data
    pub chips: Vec<RawChipData>,
}

/// Raw data from a single hwmon chip
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RawChipData {
    pub chip_name: String,
    pub chip_path: PathBuf,
    pub temperatures: Vec<RawTempReading>,
    pub fans: Vec<RawFanReading>,
    pub pwms: Vec<RawPwmReading>,
}

/// Raw temperature sensor reading
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RawTempReading {
    pub sensor_name: String,
    pub sensor_path: PathBuf,
    pub label: Option<String>,
    /// Temperature in millidegrees Celsius (raw from sysfs)
    pub raw_value: Option<i32>,
    /// Temperature in degrees Celsius
    pub celsius: Option<f32>,
}

/// Raw fan sensor reading
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RawFanReading {
    pub sensor_name: String,
    pub sensor_path: PathBuf,
    pub label: Option<String>,
    /// RPM value
    pub rpm: Option<u32>,
}

/// Raw PWM controller reading
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RawPwmReading {
    pub controller_name: String,
    pub pwm_path: PathBuf,
    pub enable_path: PathBuf,
    pub label: Option<String>,
    /// PWM value (0-255)
    pub pwm_value: Option<u8>,
    /// PWM enable mode (0=disabled, 1=manual, 2=auto)
    pub enable_mode: Option<u8>,
    /// Calculated percentage (0-100)
    pub percent: Option<f32>,
}

// ============================================================================
// GPU Types
// ============================================================================
