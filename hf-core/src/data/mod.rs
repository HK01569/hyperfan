//! Data types, configuration, and validation modules
//!
//! Contains all core data structures and configuration management.

mod config;
mod persistence;
mod types;
mod validation;

pub use config::{
    create_default_curve,
};
pub use types::{
    CurvePoint, FanMapping, FanSensor, HwmonChip, ProbeResult, PwmController,
    RawChipData, RawControllerSnapshot, RawFanReading, RawPwmReading, RawTempReading,
    SystemSummary, TempSource, TemperatureSensor,
};

// Re-export GPU types from hf-gpu crate
pub use hf_gpu::{GpuDevice, GpuFan, GpuSnapshot, GpuTemperature, GpuVendor};
pub use persistence::{
    delete_curve, get_curves_path, load_curves, save_curve, save_curves,
    update_curve_points, CurveStore, PersistedCurve,
};
pub use validation::{
    validate_curve_points, validate_fan_path, validate_file_size, validate_percentage,
    validate_pwm_path, validate_pwm_value, validate_sensor_name, validate_temp_path,
};
