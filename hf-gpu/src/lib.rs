//! GPU detection and control for Hyperfan
//!
//! Provides vendor-specific GPU detection and fan control for:
//! - NVIDIA GPUs (via nvidia-smi and nvidia-settings)
//! - AMD GPUs (via amdgpu driver and sysfs)
//! - Intel Arc GPUs (via i915 driver and sysfs)

pub mod nvidia;
pub mod amd;
pub mod intel;

mod types;
pub mod constants;

pub use types::*;
pub use constants as gpu_const;

use hf_error::HyperfanError;
use tracing::{debug, info};

pub type Result<T> = std::result::Result<T, HyperfanError>;

/// Enumerate all detected GPUs (NVIDIA, AMD, and Intel)
pub fn enumerate_gpus() -> Result<Vec<GpuDevice>> {
    let mut gpus = Vec::new();

    // Detect NVIDIA GPUs
    match nvidia::enumerate_gpus() {
        Ok(nvidia_gpus) => {
            info!("Found {} NVIDIA GPU(s)", nvidia_gpus.len());
            gpus.extend(nvidia_gpus);
        }
        Err(e) => {
            debug!("No NVIDIA GPUs detected: {}", e);
        }
    }

    // Detect AMD GPUs
    match amd::enumerate_gpus() {
        Ok(amd_gpus) => {
            info!("Found {} AMD GPU(s)", amd_gpus.len());
            gpus.extend(amd_gpus);
        }
        Err(e) => {
            debug!("No AMD GPUs detected: {}", e);
        }
    }

    // Detect Intel GPUs
    match intel::enumerate_gpus() {
        Ok(intel_gpus) => {
            info!("Found {} Intel GPU(s)", intel_gpus.len());
            gpus.extend(intel_gpus);
        }
        Err(e) => {
            debug!("No Intel GPUs detected: {}", e);
        }
    }

    Ok(gpus)
}

/// Enumerate all GPU PWM controllers for fan pairing
pub fn enumerate_gpu_pwm_controllers() -> Vec<GpuPwmController> {
    let mut controllers = Vec::new();
    
    // Enumerate NVIDIA GPU fans
    match nvidia::enumerate_pwm_controllers() {
        Ok(nvidia_controllers) => {
            info!("Found {} NVIDIA GPU fan controller(s)", nvidia_controllers.len());
            controllers.extend(nvidia_controllers);
        }
        Err(e) => {
            debug!("No NVIDIA GPU fan controllers: {}", e);
        }
    }
    
    // Enumerate AMD GPU fans
    match amd::enumerate_pwm_controllers() {
        Ok(amd_controllers) => {
            info!("Found {} AMD GPU fan controller(s)", amd_controllers.len());
            controllers.extend(amd_controllers);
        }
        Err(e) => {
            debug!("No AMD GPU fan controllers: {}", e);
        }
    }
    
    // Enumerate Intel Arc GPU fans
    match intel::enumerate_pwm_controllers() {
        Ok(intel_controllers) => {
            if !intel_controllers.is_empty() {
                info!("Found {} Intel GPU fan controller(s)", intel_controllers.len());
                controllers.extend(intel_controllers);
            }
        }
        Err(e) => {
            debug!("No Intel GPU fan controllers: {}", e);
        }
    }
    
    controllers
}

/// Capture a snapshot of all GPU data
pub fn capture_gpu_snapshot() -> Result<GpuSnapshot> {
    let gpus = enumerate_gpus()?;
    let timestamp_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_else(|e| {
            tracing::warn!("System time before Unix epoch: {}, using 0", e);
            0
        });

    Ok(GpuSnapshot { timestamp_ms, gpus })
}

/// Set GPU fan speed by controller ID
pub fn set_gpu_fan_speed_by_id(controller_id: &str, percent: u32) -> Result<()> {
    if percent > 100 {
        return Err(HyperfanError::InvalidConfig {
            field: "percent".to_string(),
            reason: format!("Fan speed must be 0-100, got {}", percent),
        });
    }
    
    let parts: Vec<&str> = controller_id.split(':').collect();
    if parts.len() < 3 {
        return Err(HyperfanError::InvalidConfig { 
            field: "controller_id".to_string(), 
            reason: format!("Invalid format: {}", controller_id) 
        });
    }
    
    let vendor = parts[0];
    let gpu_index: u32 = parts[1].parse()
        .map_err(|e| HyperfanError::InvalidConfig { 
            field: "gpu_index".to_string(), 
            reason: format!("Invalid GPU index: {}", e) 
        })?;
    let fan_index: u32 = parts[2].parse()
        .map_err(|e| HyperfanError::InvalidConfig { 
            field: "fan_index".to_string(), 
            reason: format!("Invalid fan index: {}", e) 
        })?;
    
    match vendor {
        "nvidia" => nvidia::set_fan_speed(gpu_index, fan_index, percent),
        "amd" => {
            let controllers = amd::enumerate_pwm_controllers()?;
            let controller = controllers.iter()
                .find(|c| c.gpu_index == gpu_index && c.fan_index == fan_index)
                .ok_or_else(|| HyperfanError::HardwareNotFound(format!("AMD GPU {}:{} not found", gpu_index, fan_index)))?;
            amd::set_fan_speed(&controller.pwm_path, percent)
        }
        "intel" => {
            let controllers = intel::enumerate_pwm_controllers()?;
            let controller = controllers.iter()
                .find(|c| c.gpu_index == gpu_index && c.fan_index == fan_index)
                .ok_or_else(|| HyperfanError::HardwareNotFound(format!("Intel GPU {}:{} not found", gpu_index, fan_index)))?;
            
            let pwm_path = std::path::Path::new(&controller.pwm_path);
            let enable_path = pwm_path.parent()
                .ok_or_else(|| HyperfanError::GpuError("Invalid PWM path".to_string()))?
                .join("pwm1_enable");
                
            if enable_path.exists() {
                std::fs::write(&enable_path, "1")
                    .map_err(|e| HyperfanError::GpuError(format!("Failed to enable manual control: {}", e)))?;
            }
            
            let pwm_value = gpu_const::pwm::from_percent(percent as f32);
            std::fs::write(pwm_path, pwm_value.to_string())
                .map_err(|e| HyperfanError::GpuError(format!("Failed to set PWM: {}", e)))?;
            
            Ok(())
        }
        _ => Err(HyperfanError::NotSupported(format!("Unknown vendor: {}", vendor))),
    }
}
