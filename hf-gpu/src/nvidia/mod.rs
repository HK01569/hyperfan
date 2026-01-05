//! NVIDIA GPU detection and control
//!
//! Detection via `nvidia-smi`, fan control via `nvidia-settings`
//! Requires X11 and `nvidia-settings` with Coolbits enabled for fan control

use crate::{GpuDevice, GpuFan, GpuPwmController, GpuTemperature, GpuVendor, Result};
use hf_error::HyperfanError;
use std::process::Command;
use tracing::{debug, info, trace, warn};

pub fn enumerate_gpus() -> Result<Vec<GpuDevice>> {
    // Check if nvidia-smi is available
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=index,name,pci.bus_id,memory.total,memory.used,temperature.gpu,power.draw,power.limit,utilization.gpu,fan.speed",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .map_err(|e| HyperfanError::GpuError(format!("nvidia-smi not found: {}", e)))?;

    if !output.status.success() {
        return Err(HyperfanError::GpuError("nvidia-smi failed".to_string()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut gpus = Vec::new();

    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
        if parts.len() < 10 {
            trace!("Skipping malformed nvidia-smi line: {}", line);
            continue;
        }

        let index = match parts[0].parse::<u32>() {
            Ok(idx) => idx,
            Err(e) => {
                tracing::warn!("Failed to parse GPU index '{}': {}", parts[0], e);
                continue;
            }
        };
        let name = parts[1].to_string();
        let pci_bus_id = if parts[2].is_empty() || parts[2] == "N/A" || parts[2] == "[N/A]" {
            None
        } else {
            Some(parts[2].to_string())
        };

        let vram_total_mb = parse_nvidia_value(parts[3]);
        let vram_used_mb = parse_nvidia_value(parts[4]);
        let temp = parse_nvidia_value_f32(parts[5]);
        let power_watts = parse_nvidia_value_f32(parts[6]);
        let power_limit_watts = parse_nvidia_value_f32(parts[7]);
        let utilization_percent = parse_nvidia_value(parts[8]);
        let fan_speed = parse_nvidia_value(parts[9]);

        // Build temperature sensor
        let mut temperatures = vec![GpuTemperature {
            name: "GPU Core".to_string(),
            current_temp: temp,
            max_temp: None,
            critical_temp: None,
            slowdown_temp: None,
        }];

        // Try to get additional temperature info
        if let Ok(extra_temps) = get_extra_temps(index) {
            temperatures.extend(extra_temps);
        }

        // Build fan info - query actual fan count for multi-fan GPUs
        let fan_count = get_fan_count(index).unwrap_or(1);
        let mut fans = Vec::new();
        
        // Create fan entries for all detected fans
        for fan_idx in 0..fan_count {
            fans.push(GpuFan {
                index: fan_idx,
                name: if fan_count > 1 {
                    format!("Fan {}", fan_idx)
                } else {
                    "GPU Fan".to_string()
                },
                speed_percent: if fan_idx == 0 { fan_speed } else { None }, // nvidia-smi only reports first fan
                rpm: None,
                target_percent: None,
                manual_control: false,
                min_percent: Some(0),
                max_percent: Some(100),
            });
        }

        gpus.push(GpuDevice {
            index,
            name,
            vendor: GpuVendor::Nvidia,
            pci_bus_id,
            vram_total_mb,
            vram_used_mb,
            temperatures,
            fans,
            power_watts,
            power_limit_watts,
            utilization_percent,
        });
    }

    Ok(gpus)
}

pub fn enumerate_pwm_controllers() -> Result<Vec<GpuPwmController>> {
    let mut controllers = Vec::new();
    
    // Query NVIDIA GPUs with fan info
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=index,name,pci.bus_id,fan.speed",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .map_err(|e| HyperfanError::GpuError(format!("nvidia-smi not found: {}", e)))?;
    
    if !output.status.success() {
        return Err(HyperfanError::GpuError("nvidia-smi failed".to_string()));
    }
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    
    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }
        
        let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
        if parts.len() < 4 {
            continue;
        }
        
        let gpu_index = match parts[0].parse::<u32>() {
            Ok(idx) => idx,
            Err(e) => {
                tracing::warn!("Failed to parse GPU index '{}': {}", parts[0], e);
                continue;
            }
        };
        let gpu_name = parts[1].to_string();
        let pci_bus_id = if parts[2].is_empty() || parts[2] == "N/A" || parts[2] == "[N/A]" {
            None
        } else {
            Some(parts[2].to_string())
        };
        let fan_speed = if parts[3].is_empty() || parts[3] == "N/A" || parts[3] == "[N/A]" || parts[3] == "[Not Supported]" {
            None
        } else {
            parts[3].parse::<u32>().ok()
        };
        
        // Skip GPUs without fan support
        if fan_speed.is_none() && parts[3] == "[Not Supported]" {
            debug!("GPU {} has no fan support, skipping", gpu_name);
            continue;
        }
        
        // NVIDIA GPUs can have multiple fans - query fan count
        let fan_count = get_fan_count(gpu_index).unwrap_or(1);
        info!("GPU {} ({}) has {} fan(s)", gpu_index, gpu_name, fan_count);
        
        for fan_idx in 0..fan_count {
            let controller_id = format!("nvidia:{}:{}", gpu_index, fan_idx);
            info!("Creating GPU fan controller: {}", controller_id);
            controllers.push(GpuPwmController {
                id: format!("nvidia:{}:{}", gpu_index, fan_idx),
                name: if fan_count > 1 {
                    format!("{} Fan {}", gpu_name, fan_idx)
                } else {
                    format!("{} Fan", gpu_name)
                },
                vendor: GpuVendor::Nvidia,
                gpu_index,
                fan_index: fan_idx,
                pwm_path: format!("nvidia:{}:{}", gpu_index, fan_idx), // Virtual path
                fan_input_path: None, // NVIDIA doesn't expose RPM via sysfs
                current_percent: fan_speed,
                current_rpm: None,
                manual_control: false,
                pci_bus_id: pci_bus_id.clone(),
            });
        }
    }
    
    info!("NVIDIA enumeration complete: {} controllers created", controllers.len());
    for controller in &controllers {
        info!("  - {}: {}", controller.id, controller.name);
    }
    
    Ok(controllers)
}

pub fn set_fan_speed(gpu_index: u32, fan_index: u32, percent: u32) -> Result<()> {
    let percent = percent.min(100);

    // First enable manual fan control
    let enable_result = Command::new("nvidia-settings")
        .args([
            "-a",
            &format!("[gpu:{}]/GPUFanControlState=1", gpu_index),
        ])
        .output();

    if let Err(e) = enable_result {
        warn!("Failed to enable NVIDIA fan control: {}", e);
    }

    // Set fan speed
    let output = Command::new("nvidia-settings")
        .args([
            "-a",
            &format!(
                "[fan:{}]/GPUTargetFanSpeed={}",
                fan_index, percent
            ),
        ])
        .output()
        .map_err(|e| HyperfanError::GpuError(format!("Failed to run nvidia-settings: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(HyperfanError::GpuError(format!("nvidia-settings failed: {}", stderr)));
    }

    info!(
        "Set NVIDIA GPU {} fan {} to {}%",
        gpu_index, fan_index, percent
    );
    Ok(())
}

pub fn reset_fan_auto(gpu_index: u32) -> Result<()> {
    let output = Command::new("nvidia-settings")
        .args([
            "-a",
            &format!("[gpu:{}]/GPUFanControlState=0", gpu_index),
        ])
        .output()
        .map_err(|e| HyperfanError::GpuError(format!("Failed to run nvidia-settings: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(HyperfanError::GpuError(format!("nvidia-settings failed: {}", stderr)));
    }

    info!("Reset NVIDIA GPU {} fan to automatic control", gpu_index);
    Ok(())
}

// ============================================================================
// Helper Functions
// ============================================================================

fn get_extra_temps(gpu_index: u32) -> Result<Vec<GpuTemperature>> {
    let mut temps = Vec::new();

    // Try to get memory junction temperature
    let output = Command::new("nvidia-smi")
        .args([
            "-i",
            &gpu_index.to_string(),
            "--query-gpu=temperature.memory",
            "--format=csv,noheader,nounits",
        ])
        .output();

    if let Ok(output) = output {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(temp) = parse_nvidia_value_f32(stdout.trim()) {
                temps.push(GpuTemperature {
                    name: "Memory".to_string(),
                    current_temp: Some(temp),
                    max_temp: None,
                    critical_temp: None,
                    slowdown_temp: None,
                });
            }
        }
    }

    Ok(temps)
}

fn get_fan_count(gpu_index: u32) -> Result<u32> {
    // Ensure DISPLAY is set for nvidia-settings
    let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string());
    
    // Query fans directly - no need to check GPUFanControlState first
    let fan_output = Command::new("nvidia-settings")
        .env("DISPLAY", &display)
        .args(["-q", "fans"])
        .output();
    
    if let Ok(fan_output) = fan_output {
        if !fan_output.status.success() {
            warn!("nvidia-settings fans query failed with status: {:?}", fan_output.status);
            warn!("stderr: {}", String::from_utf8_lossy(&fan_output.stderr));
            return Ok(1);
        }
        
        let stdout = String::from_utf8_lossy(&fan_output.stdout);
        info!("nvidia-settings fans output for GPU {}:", gpu_index);
        for line in stdout.lines().take(10) {
            info!("  {}", line);
        }
        
        // Count lines that mention fans
        // Look for lines like "[0] hostname:0[fan:0]" or "Attribute 'GPUTargetFanSpeed' (hostname:0[fan:0])"
        // The format is "hostname:display[fan:N]" where display matches the GPU index
        let display_marker = format!(":{}[fan:", gpu_index);
        info!("Looking for pattern: '{}'", display_marker);
        
        let matching_lines: Vec<&str> = stdout.lines()
            .filter(|l| l.contains(&display_marker))
            .collect();
        let count = matching_lines.len();
        
        info!("Found {} matching lines:", count);
        for line in &matching_lines {
            info!("  {}", line);
        }
        
        if count > 0 {
            info!("Returning {} fans for GPU {}", count, gpu_index);
            return Ok(count.min(crate::constants::MAX_FANS_PER_GPU as usize) as u32);
        }
        
        // Fallback: just count all [fan:N] entries if we can't match by display
        let fan_count = stdout.lines()
            .filter(|l| l.contains("[fan:"))
            .count();
        if fan_count > 0 {
            info!("Fallback: Found {} total fans via nvidia-settings", fan_count);
            return Ok(fan_count.min(crate::constants::MAX_FANS_PER_GPU as usize) as u32);
        }
    } else {
        warn!("Failed to execute nvidia-settings");
    }
    
    warn!("Defaulting to 1 fan for GPU {}", gpu_index);
    Ok(1) // Default to 1 fan
}

fn parse_nvidia_value(s: &str) -> Option<u32> {
    if s.is_empty() || s == "N/A" || s == "[N/A]" || s == "[Not Supported]" {
        None
    } else {
        s.parse().ok()
    }
}

fn parse_nvidia_value_f32(s: &str) -> Option<f32> {
    if s.is_empty() || s == "N/A" || s == "[N/A]" || s == "[Not Supported]" {
        None
    } else {
        s.parse().ok()
    }
}
