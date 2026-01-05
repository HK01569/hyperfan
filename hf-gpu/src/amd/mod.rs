//! AMD GPU detection and control
//!
//! Detection via sysfs (amdgpu driver), fan control via PWM files
//! Requires write access to `/sys/class/drm/card*/device/hwmon/*/pwm1`

use crate::{gpu_const, GpuDevice, GpuFan, GpuPwmController, GpuTemperature, GpuVendor, Result};
use hf_error::HyperfanError;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

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

        if !is_amd_gpu(&device_path) {
            continue;
        }

        let hwmon_path = find_hwmon(&device_path);
        let gpu = read_gpu(gpu_index, &device_path, hwmon_path.as_deref())?;
        if let Some(gpu) = gpu {
            gpus.push(gpu);
            gpu_index += 1;
        }
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
        
        let vendor_path = device_path.join("vendor");
        if let Ok(vendor_id) = fs::read_to_string(&vendor_path) {
            if vendor_id.trim() != gpu_const::AMD_VENDOR_ID {
                continue;
            }
        } else {
            continue;
        }
        
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
        
        let pwm_path = hwmon_path.join("pwm1");
        if !pwm_path.exists() {
            debug!("AMD GPU at {:?} has no PWM control", card_path);
            continue;
        }
        
        let gpu_name = read_gpu_name(&device_path);
        let pci_bus_id = device_path.file_name().and_then(|n| n.to_str()).map(|s| s.to_string());
        let card_num = match name_str.replace("card", "").parse::<u32>() {
            Ok(num) => num,
            Err(e) => {
                tracing::warn!("Failed to parse AMD card number from '{}': {}", name_str, e);
                continue;
            }
        };
        
        let current_pwm = fs::read_to_string(&pwm_path).ok().and_then(|s| s.trim().parse::<u8>().ok());
        let current_percent = current_pwm.map(|v| gpu_const::pwm::to_percent(v).round() as u32);
        
        let fan_input_path = hwmon_path.join("fan1_input");
        let current_rpm = if fan_input_path.exists() {
            fs::read_to_string(&fan_input_path).ok().and_then(|s| s.trim().parse::<u32>().ok())
        } else {
            None
        };
        
        let pwm_enable = fs::read_to_string(hwmon_path.join("pwm1_enable")).ok().and_then(|s| s.trim().parse::<u8>().ok());
        let manual_control = pwm_enable == Some(1);
        
        controllers.push(GpuPwmController {
            id: format!("amd:{}:0", card_num),
            name: format!("{} Fan", gpu_name),
            vendor: GpuVendor::Amd,
            gpu_index: card_num,
            fan_index: 0,
            pwm_path: pwm_path.to_string_lossy().to_string(),
            fan_input_path: if fan_input_path.exists() { Some(fan_input_path.to_string_lossy().to_string()) } else { None },
            current_percent,
            current_rpm,
            manual_control,
            pci_bus_id: pci_bus_id.clone(),
        });
    }
    
    Ok(controllers)
}

pub fn set_fan_speed(hwmon_pwm_path: &str, percent: u32) -> Result<()> {
    let percent = percent.min(100);
    let pwm_path = Path::new(hwmon_pwm_path);

    let pwm_enable_path = pwm_path.parent()
        .ok_or_else(|| HyperfanError::GpuError("Invalid PWM path".to_string()))?
        .join("pwm1_enable");
        
    if pwm_enable_path.exists() {
        fs::write(&pwm_enable_path, "1")
            .map_err(|e| HyperfanError::GpuError(format!("Failed to enable manual fan control: {}", e)))?;
    }

    let pwm_value = gpu_const::pwm::from_percent(percent as f32);
    fs::write(pwm_path, pwm_value.to_string())
        .map_err(|e| HyperfanError::GpuError(format!("Failed to set PWM value: {}", e)))?;

    info!("Set AMD GPU fan to {}% (PWM: {})", percent, pwm_value);
    Ok(())
}

pub fn reset_fan_auto(hwmon_pwm_path: &str) -> Result<()> {
    let pwm_path = Path::new(hwmon_pwm_path);
    let pwm_enable_path = pwm_path.parent()
        .ok_or_else(|| HyperfanError::GpuError("Invalid PWM path".to_string()))?
        .join("pwm1_enable");
        
    if pwm_enable_path.exists() {
        fs::write(&pwm_enable_path, "2")
            .map_err(|e| HyperfanError::GpuError(format!("Failed to reset fan to auto: {}", e)))?;
        info!("Reset AMD GPU fan to automatic control");
    }
    Ok(())
}

fn is_amd_gpu(device_path: &Path) -> bool {
    let vendor_path = device_path.join("vendor");
    if let Ok(vendor_id) = fs::read_to_string(&vendor_path) {
        return vendor_id.trim() == gpu_const::AMD_VENDOR_ID;
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

fn read_gpu(index: u32, device_path: &Path, hwmon_path: Option<&Path>) -> Result<Option<GpuDevice>> {
    let name = read_gpu_name(device_path);
    let pci_bus_id = device_path.file_name().and_then(|n| n.to_str()).map(|s| s.to_string());
    let (vram_total_mb, vram_used_mb) = read_vram(device_path);
    
    let temperatures = match hwmon_path.as_ref().map(|p| read_temperatures(p)) {
        Some(temps) => temps,
        None => {
            tracing::debug!("No hwmon path for AMD GPU {}, temperature data unavailable", index);
            Vec::new()
        }
    };
    
    let fans = match hwmon_path.as_ref().map(|p| read_fans(p, index)) {
        Some(f) => f,
        None => {
            tracing::debug!("No hwmon path for AMD GPU {}, fan data unavailable", index);
            Vec::new()
        }
    };
    
    let (power_watts, power_limit_watts) = match hwmon_path.as_ref() {
        Some(path) => read_power(path),
        None => {
            tracing::debug!("No hwmon path for AMD GPU {}, power data unavailable", index);
            (None, None)
        }
    };
    let utilization_percent = read_utilization(device_path);

    Ok(Some(GpuDevice {
        index, name, vendor: GpuVendor::Amd, pci_bus_id, vram_total_mb, vram_used_mb,
        temperatures, fans, power_watts, power_limit_watts, utilization_percent,
    }))
}

fn read_gpu_name(device_path: &Path) -> String {
    let product_path = device_path.join("product_name");
    if let Ok(name) = fs::read_to_string(&product_path) {
        let name = name.trim();
        if !name.is_empty() {
            return name.to_string();
        }
    }
    "AMD GPU".to_string()
}

fn read_vram(device_path: &Path) -> (Option<u32>, Option<u32>) {
    let total_mb = fs::read_to_string(device_path.join("mem_info_vram_total")).ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|bytes| (bytes / gpu_const::BYTES_PER_MB) as u32);
    let used_mb = fs::read_to_string(device_path.join("mem_info_vram_used")).ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|bytes| (bytes / gpu_const::BYTES_PER_MB) as u32);
    (total_mb, used_mb)
}

fn read_temperatures(hwmon_path: &Path) -> Vec<GpuTemperature> {
    let mut temps = Vec::new();
    let temp_files = [("temp1_input", "GPU Edge"), ("temp2_input", "GPU Junction"), ("temp3_input", "GPU Memory")];
    for (file, label) in &temp_files {
        let input_path = hwmon_path.join(file);
        if !input_path.exists() { continue; }
        let current_temp = fs::read_to_string(&input_path).ok()
            .and_then(|s| s.trim().parse::<i32>().ok())
            .map(|millidegrees| millidegrees as f32 / gpu_const::MILLIDEGREE_DIVISOR);
        let name = label.to_string();
        temps.push(GpuTemperature { name, current_temp, max_temp: None, critical_temp: None, slowdown_temp: None });
    }
    temps
}

fn read_fans(hwmon_path: &Path, _gpu_index: u32) -> Vec<GpuFan> {
    let mut fans = Vec::new();
    let pwm_path = hwmon_path.join("pwm1");
    if !pwm_path.exists() { return fans; }
    let pwm_value = fs::read_to_string(&pwm_path).ok().and_then(|s| s.trim().parse::<u8>().ok());
    let speed_percent = pwm_value.map(|v| gpu_const::pwm::to_percent(v).round() as u32);
    let rpm = fs::read_to_string(hwmon_path.join("fan1_input")).ok().and_then(|s| s.trim().parse::<u32>().ok());
    let pwm_enable = fs::read_to_string(hwmon_path.join("pwm1_enable")).ok().and_then(|s| s.trim().parse::<u8>().ok());
    let manual_control = pwm_enable == Some(1);
    fans.push(GpuFan { index: 0, name: "GPU Fan".to_string(), speed_percent, rpm, target_percent: None, manual_control, min_percent: None, max_percent: None });
    fans
}

fn read_power(hwmon_path: &Path) -> (Option<f32>, Option<f32>) {
    let current_power_watts = fs::read_to_string(hwmon_path.join("power1_average")).ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|microwatts| microwatts as f32 / gpu_const::MICROWATTS_PER_WATT);
    let power_limit_watts = fs::read_to_string(hwmon_path.join("power1_cap")).ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|microwatts| microwatts as f32 / gpu_const::MICROWATTS_PER_WATT);
    (current_power_watts, power_limit_watts)
}

fn read_utilization(device_path: &Path) -> Option<u32> {
    fs::read_to_string(device_path.join("gpu_busy_percent")).ok().and_then(|s| s.trim().parse::<u32>().ok())
}
