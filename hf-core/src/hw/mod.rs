//! Hardware interaction modules
//!
//! Contains all low-level hardware access for hwmon devices and GPUs.

pub mod binding;
mod capture;
mod control;
mod detection;
pub mod fingerprint;
mod gpu;
mod hardware;

pub use capture::{
    capture_chip_data, capture_raw_snapshot, snapshot_to_json, snapshot_to_json_compact,
};
pub use control::{
    enable_manual_pwm, read_fan_rpm, read_pwm_value, read_temperature, set_pwm_percent,
    set_pwm_value,
};
pub use detection::{
    autodetect_fan_pwm_mappings, autodetect_fan_pwm_mappings_advanced,
    autodetect_fan_pwm_mappings_heuristic, autodetect_with_fingerprints,
    FingerprintedDetectionResult,
};
pub use gpu::{
    capture_gpu_snapshot, enumerate_gpus, enumerate_gpu_pwm_controllers,
    reset_amd_fan_auto, reset_nvidia_fan_auto, set_amd_fan_speed, set_nvidia_fan_speed,
    set_gpu_fan_speed_by_id, GpuPwmController,
};
pub use hardware::{check_pwm_permissions, enumerate_hwmon_chips};
