//! Intel GPU detection and control
//!
//! Basic temperature monitoring for iGPUs, fan control for Arc discrete GPUs
//! Detection via i915 driver and sysfs

use crate::{gpu_const, GpuDevice, GpuPwmController, GpuTemperature, GpuVendor, Result};
use hf_error::HyperfanError;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::debug;

pub fn enumerate_gpus() -> Result<Vec<GpuDevice>> {
    let mut gpus = Vec::new();
    let drm_path = Path::new(gpu_const::DRM_PATH);

    if !drm_path.exists() {
        return Err(HyperfanError::HardwareNotFound(format!("DRM path {} not found", gpu_const::DRM_PATH)));
    }

    let mut gpu_index = 0u32;

    for entry in fs::read_dir(drm_path)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if !name_str.starts_with("card") || name_str.contains('-') {
            continue;
        }

        let card_path = entry.path();
        let device_path = card_path.join("device");

        // Check if this is an Intel GPU
        if !is_intel_gpu(&device_path) {
            continue;
        }

        // Intel GPUs typically don't have controllable fans
        // but we can still report temperatures via hwmon

        let hwmon_path = find_hwmon(&device_path);
        let temperatures = match hwmon_path.as_ref().map(|p| read_temperatures(p)) {
            Some(temps) => temps,
            None => {
                tracing::debug!("No hwmon path for Intel GPU, temperature data unavailable");
                Vec::new()
            }
        };

        let name = read_gpu_name(&device_path);

        gpus.push(GpuDevice {
            index: gpu_index,
            name,
            vendor: GpuVendor::Intel,
            pci_bus_id: device_path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string()),
            vram_total_mb: None,
            vram_used_mb: None,
            temperatures,
            fans: Vec::new(), // Intel iGPUs typically don't have dedicated fans
            power_watts: None,
            power_limit_watts: None,
            utilization_percent: None,
        });

        gpu_index += 1;
    }

    Ok(gpus)
}

pub fn enumerate_pwm_controllers() -> Result<Vec<GpuPwmController>> {
    let mut controllers = Vec::new();
    let drm_path = Path::new(gpu_const::DRM_PATH);
    
    if !drm_path.exists() {
        return Err(HyperfanError::HardwareNotFound("DRM path not found".to_string()));
    }
    
    for entry in fs::read_dir(drm_path)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        
        if !name_str.starts_with("card") || name_str.contains('-') {
            continue;
        }
        
        let card_path = entry.path();
        let device_path = card_path.join("device");
        
        // Check if this is an Intel GPU
        let vendor_path = device_path.join("vendor");
        if let Ok(vendor_id) = fs::read_to_string(&vendor_path) {
            if vendor_id.trim() != gpu_const::INTEL_VENDOR_ID {
                continue;
            }
        } else {
            continue;
        }
        
        // Intel Arc discrete GPUs may have fan control via hwmon
        let hwmon_dir = device_path.join("hwmon");
        if !hwmon_dir.exists() {
            continue;
        }
        
        let hwmon_path = fs::read_dir(&hwmon_dir)?
            .filter_map(|e| e.ok())
            .next()
            .map(|e| e.path());
        
        let Some(hwmon_path) = hwmon_path else {
            continue;
        };
        
        // Check for PWM control (Intel Arc GPUs)
        let pwm_path = hwmon_path.join("pwm1");
        if !pwm_path.exists() {
            // Intel iGPUs don't have fan control - skip silently
            continue;
        }
        
        let card_num = match name_str.replace("card", "").parse::<u32>() {
            Ok(num) => num,
            Err(e) => {
                tracing::warn!("Failed to parse Intel card number from '{}': {}", name_str, e);
                continue;
            }
        };
        
        // Read current values
        let current_pwm = fs::read_to_string(&pwm_path)
            .ok()
            .and_then(|s| s.trim().parse::<u8>().ok());
        let current_percent = current_pwm.map(|v| gpu_const::pwm::to_percent(v).round() as u32);
        
        let fan_input_path = hwmon_path.join("fan1_input");
        let current_rpm = if fan_input_path.exists() {
            fs::read_to_string(&fan_input_path)
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok())
        } else {
            None
        };
        
        let pci_bus_id = device_path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string());
        
        controllers.push(GpuPwmController {
            id: format!("intel:{}:0", card_num),
            name: "Intel Arc GPU Fan".to_string(),
            vendor: GpuVendor::Intel,
            gpu_index: card_num,
            fan_index: 0,
            pwm_path: pwm_path.to_string_lossy().to_string(),
            fan_input_path: if fan_input_path.exists() {
                Some(fan_input_path.to_string_lossy().to_string())
            } else {
                None
            },
            current_percent,
            current_rpm,
            manual_control: false,
            pci_bus_id,
        });
    }
    
    Ok(controllers)
}

pub fn set_fan_speed_by_index(gpu_index: u32, fan_index: u32, percent: u32) -> Result<()> {
    let percent = percent.min(100);
    
    // Find the controller
    let controllers = enumerate_pwm_controllers()?;
    let controller = controllers.iter()
        .find(|c| c.gpu_index == gpu_index && c.fan_index == fan_index)
        .ok_or_else(|| HyperfanError::HardwareNotFound(format!("Intel GPU {}:{} not found", gpu_index, fan_index)))?;
    
    // Enable manual control
    let pwm_path = Path::new(&controller.pwm_path);
    let enable_path = pwm_path.parent()
        .ok_or_else(|| HyperfanError::GpuError("Invalid PWM path".to_string()))?
        .join("pwm1_enable");
        
    if enable_path.exists() {
        fs::write(&enable_path, "1")
            .map_err(|e| HyperfanError::GpuError(format!("Failed to enable manual control: {}", e)))?;
    }
    
    // Set PWM value
    let pwm_value = gpu_const::pwm::from_percent(percent as f32);
    fs::write(pwm_path, pwm_value.to_string())
        .map_err(|e| HyperfanError::GpuError(format!("Failed to set PWM: {}", e)))?;
    
    debug!("Set Intel GPU fan to {}% (PWM: {})", percent, pwm_value);
    Ok(())
}

// ============================================================================
// Helper Functions
// ============================================================================

fn is_intel_gpu(device_path: &Path) -> bool {
    let vendor_path = device_path.join("vendor");
    if let Ok(vendor_id) = fs::read_to_string(&vendor_path) {
        return vendor_id.trim() == gpu_const::INTEL_VENDOR_ID;
    }
    false
}

fn find_hwmon(device_path: &Path) -> Option<PathBuf> {
    let hwmon_dir = device_path.join("hwmon");
    if hwmon_dir.exists() {
        if let Ok(entries) = fs::read_dir(&hwmon_dir) {
            for entry in entries.flatten() {
                return Some(entry.path());
            }
        }
    }
    None
}

fn read_gpu_name(device_path: &Path) -> String {
    // Try i915 driver info
    let uevent_path = device_path.join("uevent");
    if let Ok(uevent) = fs::read_to_string(&uevent_path) {
        for line in uevent.lines() {
            if line.starts_with("DRIVER=") {
                let driver = line.replace("DRIVER=", "");
                return format!("Intel {} Graphics", driver.to_uppercase());
            }
        }
    }
    "Intel Graphics".to_string()
}

fn read_temperatures(hwmon_path: &Path) -> Vec<GpuTemperature> {
    let mut temps = Vec::new();

    for i in 1..=gpu_const::INTEL_TEMP_SENSOR_COUNT {
        let input_path = hwmon_path.join(format!("temp{}_input", i));
        if !input_path.exists() {
            continue;
        }

        // Temperature is in millidegrees Celsius
        let current_temp = fs::read_to_string(&input_path)
            .ok()
            .and_then(|s| s.trim().parse::<i32>().ok())
            .map(|millidegrees| millidegrees as f32 / gpu_const::MILLIDEGREE_DIVISOR);

        let label_path = hwmon_path.join(format!("temp{}_label", i));
        let name = fs::read_to_string(&label_path)
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| format!("Temp {}", i));

        temps.push(GpuTemperature {
            name,
            current_temp,
            max_temp: None,
            critical_temp: None,
            slowdown_temp: None,
        });
    }

    temps
}
