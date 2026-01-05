//! Hardware enumeration for hwmon devices
//!
//! Cross-platform support for hardware monitoring:
//! - **Linux**: Uses `/sys/class/hwmon` (hwmon subsystem)
//! - **FreeBSD**: Uses `sysctl dev.cpu` and `dev.acpi_thermal`
//! - **OpenBSD/NetBSD**: Uses `sysctl hw.sensors`
//!
//! # Sensor Types
//!
//! - **Temperature**: `tempN_input` files (millidegrees Celsius)
//! - **Fan**: `fanN_input` files (RPM)
//! - **PWM**: `pwmN` files (0-255 duty cycle)

use crate::error::Result;
use std::fs;
use std::path::Path;
use tracing::{debug, info, trace, warn};

use crate::constants::{paths, temperature};
use crate::data::{FanSensor, HwmonChip, PwmController, TemperatureSensor};

/// Enumerate all hwmon chips and their sensors
/// Works on Linux (hwmon), FreeBSD (sysctl), and OpenBSD/NetBSD (hw.sensors)
pub fn enumerate_hwmon_chips() -> Result<Vec<HwmonChip>> {
    // Try Linux hwmon first
    let hwmon_path = Path::new(paths::HWMON_BASE);
    
    if hwmon_path.exists() && hwmon_path.is_dir() {
        return enumerate_linux_hwmon(hwmon_path);
    }
    
    // Try BSD sysctl-based detection
    #[cfg(any(target_os = "freebsd", target_os = "dragonfly"))]
    {
        return enumerate_freebsd_sensors();
    }
    
    #[cfg(any(target_os = "openbsd", target_os = "netbsd"))]
    {
        return enumerate_openbsd_sensors();
    }
    
    // No sensors found
    warn!("No hardware monitoring interface found on this platform");
    Ok(Vec::new())
}

/// Linux hwmon enumeration
fn enumerate_linux_hwmon(hwmon_path: &Path) -> Result<Vec<HwmonChip>> {
    let mut chips = Vec::new();
    
    debug!("Scanning Linux hwmon chips in {:?}", hwmon_path);

    for entry in fs::read_dir(hwmon_path)? {
        let entry = entry?;
        let path = entry.path();
        trace!("Checking hwmon device: {:?}", path);

        if let Some(chip) = read_hwmon_chip(&path)? {
            info!(
                chip = %chip.name,
                temps = chip.temperatures.len(),
                fans = chip.fans.len(),
                pwms = chip.pwms.len(),
                "Found hwmon chip"
            );
            chips.push(chip);
        } else {
            trace!("Skipped {:?} (no useful sensors)", path);
        }
    }

    info!("Total hwmon chips found: {}", chips.len());
    Ok(chips)
}

/// FreeBSD sensor enumeration using sysctl
#[cfg(any(target_os = "freebsd", target_os = "dragonfly"))]
fn enumerate_freebsd_sensors() -> Result<Vec<HwmonChip>> {
    let mut chips = Vec::new();
    
    debug!("Scanning FreeBSD sensors via sysctl");
    
    // Get CPU temperatures (dev.cpu.N.temperature)
    if let Ok(output) = Command::new("sysctl").args(["-a"]).output() {
        if output.status.success() {
            let sysctl_output = String::from_utf8_lossy(&output.stdout);
            
            let mut cpu_temps = Vec::new();
            let mut acpi_temps = Vec::new();
            
            for line in sysctl_output.lines() {
                // CPU temperature: dev.cpu.0.temperature: 45.0C
                if line.starts_with("dev.cpu.") && line.contains(".temperature:") {
                    if let Some(temp) = parse_bsd_temperature(line) {
                        let parts: Vec<&str> = line.split('.').collect();
                        if parts.len() >= 3 {
                            let cpu_num = parts[2];
                            cpu_temps.push(TemperatureSensor {
                                name: format!("temp{}", cpu_num),
                                input_path: PathBuf::from(format!("sysctl:dev.cpu.{}.temperature", cpu_num)),
                                label: Some(format!("CPU {}", cpu_num)),
                                current_temp: Some(temp),
                            });
                        }
                    }
                }
                
                // ACPI thermal zones: hw.acpi.thermal.tz0.temperature: 50.0C
                if line.contains("acpi.thermal") && line.contains(".temperature:") {
                    if let Some(temp) = parse_bsd_temperature(line) {
                        let name = line.split(':').next().unwrap_or("unknown");
                        acpi_temps.push(TemperatureSensor {
                            name: name.replace("hw.acpi.thermal.", "").replace(".temperature", ""),
                            input_path: PathBuf::from(format!("sysctl:{}", name)),
                            label: Some("ACPI Thermal Zone".to_string()),
                            current_temp: Some(temp),
                        });
                    }
                }
            }
            
            if !cpu_temps.is_empty() {
                chips.push(HwmonChip {
                    name: "cpu".to_string(),
                    path: PathBuf::from("sysctl:dev.cpu"),
                    temperatures: cpu_temps,
                    fans: Vec::new(),
                    pwms: Vec::new(),
                });
            }
            
            if !acpi_temps.is_empty() {
                chips.push(HwmonChip {
                    name: "acpi_thermal".to_string(),
                    path: PathBuf::from("sysctl:hw.acpi.thermal"),
                    temperatures: acpi_temps,
                    fans: Vec::new(),
                    pwms: Vec::new(),
                });
            }
        }
    }
    
    info!("FreeBSD sensors found: {} chips", chips.len());
    Ok(chips)
}

/// OpenBSD/NetBSD sensor enumeration using hw.sensors
#[cfg(any(target_os = "openbsd", target_os = "netbsd"))]
fn enumerate_openbsd_sensors() -> Result<Vec<HwmonChip>> {
    let mut chips = Vec::new();
    
    debug!("Scanning OpenBSD/NetBSD sensors via hw.sensors");
    
    if let Ok(output) = Command::new("sysctl").args(["hw.sensors"]).output() {
        if output.status.success() {
            let sysctl_output = String::from_utf8_lossy(&output.stdout);
            
            // Parse hw.sensors.chip.sensorN=value format
            let mut current_chip: Option<HwmonChip> = None;
            let mut chip_name = String::new();
            
            for line in sysctl_output.lines() {
                // hw.sensors.cpu0.temp0=45.00 degC
                if let Some(rest) = line.strip_prefix("hw.sensors.") {
                    let parts: Vec<&str> = rest.splitn(3, '.').collect();
                    if parts.len() >= 2 {
                        let new_chip_name = parts[0].to_string();
                        
                        if new_chip_name != chip_name {
                            if let Some(chip) = current_chip.take() {
                                if !chip.temperatures.is_empty() {
                                    chips.push(chip);
                                }
                            }
                            chip_name = new_chip_name.clone();
                            current_chip = Some(HwmonChip {
                                name: new_chip_name,
                                path: PathBuf::from("sysctl:hw.sensors"),
                                temperatures: Vec::new(),
                                fans: Vec::new(),
                                pwms: Vec::new(),
                            });
                        }
                        
                        if let Some(ref mut chip) = current_chip {
                            if parts[1].starts_with("temp") {
                                if let Some(temp) = parse_bsd_temperature(line) {
                                    chip.temperatures.push(TemperatureSensor {
                                        name: parts[1].to_string(),
                                        input_path: PathBuf::from(format!("sysctl:hw.sensors.{}.{}", chip_name, parts[1])),
                                        label: Some(format!("{} {}", chip_name, parts[1])),
                                        current_temp: Some(temp),
                                    });
                                }
                            }
                        }
                    }
                }
            }
            
            if let Some(chip) = current_chip {
                if !chip.temperatures.is_empty() {
                    chips.push(chip);
                }
            }
        }
    }
    
    info!("OpenBSD/NetBSD sensors found: {} chips", chips.len());
    Ok(chips)
}

/// Parse BSD temperature string (e.g., "45.0C" or "45.00 degC")
#[cfg(any(target_os = "freebsd", target_os = "openbsd", target_os = "netbsd", target_os = "dragonfly"))]
fn parse_bsd_temperature(line: &str) -> Option<f32> {
    // Extract value after colon or equals
    let value_part = line.split([':', '=']).last()?.trim();
    
    // Remove units and parse
    let numeric: String = value_part
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    
    numeric.parse::<f32>().ok()
}

fn read_hwmon_chip(chip_path: &Path) -> Result<Option<HwmonChip>> {
    let name_path = chip_path.join("name");
    let name = if name_path.exists() {
        fs::read_to_string(&name_path)?.trim().to_string()
    } else {
        chip_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string()
    };

    trace!(chip = %name, path = ?chip_path, "Reading hwmon chip");

    let mut temperatures = Vec::new();
    let mut fans = Vec::new();
    let mut pwms = Vec::new();

    let entries = fs::read_dir(chip_path)?;
    let mut all_files = Vec::new();

    for entry in entries {
        let entry = entry?;
        let file_name = entry.file_name();
        let file_name_str = file_name.to_string_lossy();
        all_files.push(file_name_str.to_string());
    }

    for file_name_str in &all_files {
        if file_name_str.starts_with("temp") && file_name_str.ends_with("_input") {
            if let Some(temp) = read_temperature_sensor(chip_path, file_name_str)? {
                trace!(sensor = %file_name_str, "Found temperature sensor");
                temperatures.push(temp);
            }
        } else if file_name_str.starts_with("fan") && file_name_str.ends_with("_input") {
            if let Some(fan) = read_fan_sensor(chip_path, file_name_str)? {
                trace!(sensor = %file_name_str, "Found fan sensor");
                fans.push(fan);
            }
        } else if file_name_str.starts_with("pwm") && !file_name_str.contains('_') {
            if let Some(pwm) = read_pwm_controller(chip_path, file_name_str)? {
                trace!(controller = %file_name_str, "Found PWM controller");
                pwms.push(pwm);
            }
        }
    }

    debug!(
        chip = %name,
        temps = temperatures.len(),
        fans = fans.len(),
        pwms = pwms.len(),
        "Chip sensor counts"
    );

    if temperatures.is_empty() && fans.is_empty() && pwms.is_empty() {
        return Ok(None);
    }

    Ok(Some(HwmonChip {
        name,
        path: chip_path.to_path_buf(),
        temperatures,
        fans,
        pwms,
    }))
}

fn read_temperature_sensor(
    chip_path: &Path,
    input_file: &str,
) -> Result<Option<TemperatureSensor>> {
    let input_path = chip_path.join(input_file);
    let base_name = input_file.replace("_input", "");
    let label_path = chip_path.join(format!("{}_label", base_name));

    let label = if label_path.exists() {
        Some(fs::read_to_string(&label_path)?.trim().to_string())
    } else {
        None
    };

    // Temperature is reported in millidegrees Celsius (e.g., 45000 = 45.0°C)
    let current_temp = if input_path.exists() {
        fs::read_to_string(&input_path)?
            .trim()
            .parse::<i32>()
            .map(|millidegrees| millidegrees as f32 / temperature::MILLIDEGREE_DIVISOR)
            .ok()
    } else {
        None
    };

    Ok(Some(TemperatureSensor {
        name: base_name,
        input_path,
        label,
        current_temp,
    }))
}

fn read_fan_sensor(chip_path: &Path, input_file: &str) -> Result<Option<FanSensor>> {
    let input_path = chip_path.join(input_file);
    let base_name = input_file.replace("_input", "");
    let label_path = chip_path.join(format!("{}_label", base_name));

    let label = if label_path.exists() {
        match fs::read_to_string(&label_path) {
            Ok(content) => Some(content.trim().to_string()),
            Err(e) => {
                trace!(path = ?label_path, error = %e, "Could not read fan label");
                None
            }
        }
    } else {
        None
    };

    let current_rpm = if input_path.exists() {
        match fs::read_to_string(&input_path) {
            Ok(content) => content.trim().parse::<u32>().ok(),
            Err(e) => {
                trace!(path = ?input_path, error = %e, "Could not read fan RPM");
                None
            }
        }
    } else {
        None
    };

    trace!(fan = %base_name, rpm = ?current_rpm, "Read fan sensor");

    Ok(Some(FanSensor {
        name: base_name,
        input_path,
        label,
        current_rpm,
    }))
}

fn read_pwm_controller(chip_path: &Path, pwm_file: &str) -> Result<Option<PwmController>> {
    let pwm_path = chip_path.join(pwm_file);
    let enable_path = chip_path.join(format!("{}_enable", pwm_file));
    let label_path = chip_path.join(format!("{}_label", pwm_file));

    if !pwm_path.exists() {
        return Ok(None);
    }

    let label = if label_path.exists() {
        match fs::read_to_string(&label_path) {
            Ok(content) => Some(content.trim().to_string()),
            Err(e) => {
                trace!(path = ?label_path, error = %e, "Could not read PWM label");
                None
            }
        }
    } else {
        None
    };

    let current_value = match fs::read_to_string(&pwm_path) {
        Ok(content) => content.trim().parse::<u8>().ok(),
        Err(e) => {
            trace!(path = ?pwm_path, error = %e, "Could not read PWM value");
            None
        }
    };

    let current_percent = current_value.map(crate::constants::pwm::to_percent);

    trace!(
        pwm = %pwm_file,
        value = ?current_value,
        percent = ?current_percent,
        "Read PWM controller"
    );

    Ok(Some(PwmController {
        name: pwm_file.to_string(),
        pwm_path,
        enable_path,
        label,
        current_value,
        current_percent,
    }))
}

/// Check if we have write permissions to PWM controls (non-destructive)
pub fn check_pwm_permissions(chips: &[HwmonChip]) -> bool {
    use std::fs::OpenOptions;

    for chip in chips {
        for pwm in &chip.pwms {
            if pwm.enable_path.exists() {
                if OpenOptions::new().write(true).open(&pwm.enable_path).is_err() {
                    return false;
                }
            }

            if OpenOptions::new().write(true).open(&pwm.pwm_path).is_err() {
                return false;
            }
        }
    }
    true
}
