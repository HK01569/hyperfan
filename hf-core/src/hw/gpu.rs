//! GPU detection and control
//!
//! This module re-exports GPU functionality from the hf-gpu crate.
//! All GPU vendor-specific code has been moved to hf-gpu for better organization.

// Re-export all GPU types and functions from hf-gpu
pub use hf_gpu::{
    capture_gpu_snapshot, enumerate_gpus, enumerate_gpu_pwm_controllers, set_gpu_fan_speed_by_id,
    GpuPwmController,
};

use crate::error::Result;
use std::path::Path;

/// Set fan speed for an NVIDIA GPU (wrapper for compatibility)
pub fn set_nvidia_fan_speed(gpu_index: u32, fan_index: u32, percent: u32) -> Result<()> {
    hf_gpu::nvidia::set_fan_speed(gpu_index, fan_index, percent)
        .map_err(|e| e.into())
}

/// Set fan speed for an AMD GPU via sysfs (wrapper for compatibility)
pub fn set_amd_fan_speed(hwmon_path: &Path, percent: u32) -> Result<()> {
    hf_gpu::amd::set_fan_speed(&hwmon_path.to_string_lossy(), percent)
        .map_err(|e| e.into())
}

/// Reset AMD GPU fan to automatic control (wrapper for compatibility)
pub fn reset_amd_fan_auto(hwmon_path: &Path) -> Result<()> {
    hf_gpu::amd::reset_fan_auto(&hwmon_path.to_string_lossy())
        .map_err(|e| e.into())
}

/// Reset NVIDIA GPU fan to automatic control (wrapper for compatibility)
pub fn reset_nvidia_fan_auto(gpu_index: u32) -> Result<()> {
    hf_gpu::nvidia::reset_fan_auto(gpu_index)
        .map_err(|e| e.into())
}
