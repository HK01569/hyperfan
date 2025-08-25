/*
 * This file is part of Hyperfan.
 *
 * Copyright (C) 2025 Hyperfan contributors
 *
 * Hyperfan is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * Hyperfan is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with Hyperfan. If not, see <https://www.gnu.org/licenses/>.
 */

use std::fs;
use std::process::Command;

pub fn read_cpu_name() -> String {
    // Try /proc/cpuinfo first
    if let Ok(s) = fs::read_to_string("/proc/cpuinfo") {
        let mut model_name: Option<String> = None;
        let mut hardware: Option<String> = None;
        let mut processor_name: Option<String> = None;

        for line in s.lines() {
            if let Some((k, v)) = line.split_once(':') {
                let key = k.trim().to_ascii_lowercase();
                let val = v.trim();
                if val.is_empty() {
                    continue;
                }
                match key.as_str() {
                    "model name" => {
                        if model_name.is_none() {
                            model_name = Some(val.to_string());
                        }
                    }
                    "hardware" => {
                        if hardware.is_none() {
                            hardware = Some(val.to_string());
                        }
                    }
                    "processor" => {
                        // Avoid picking core index like "0"; keep only non-numeric descriptors
                        if processor_name.is_none() && !val.chars().all(|c| c.is_ascii_digit()) {
                            processor_name = Some(val.to_string());
                        }
                    }
                    _ => {}
                }
            }
        }

        if let Some(m) = model_name {
            return m;
        }
        if let Some(h) = hardware {
            return h;
        }
        if let Some(p) = processor_name {
            return p;
        }
    }
    // Fallback: device-tree model
    if let Ok(mut s) = fs::read_to_string("/proc/device-tree/model") {
        s.retain(|c| c != '\u{0}');
        return s.trim().to_string();
    }
    String::new()
}

pub fn read_mb_name() -> String {
    let read_trim = |p: &str| -> Option<String> {
        fs::read_to_string(p)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let vendor = read_trim("/sys/devices/virtual/dmi/id/board_vendor");
    let name = read_trim("/sys/devices/virtual/dmi/id/board_name");
    match (vendor, name) {
        (Some(v), Some(n)) => format!("{} {}", v, n),
        (Some(v), None) => v,
        (None, Some(n)) => n,
        (None, None) => read_trim("/sys/devices/virtual/dmi/id/product_name").unwrap_or_default(),
    }
}

pub fn load_sensor_modules() {
    // Common sensor kernel modules for various chipsets
    let modules = vec![
        // Super I/O chips
        "it87", "nct6775", "nct6683", "nct6687", "f71882fg", "f71808e", "w83627ehf", "w83627hf",
        "w83781d", "w83791d", "w83792d", "w83793", "w83795", "w83l785ts", "w83l786ng",
        // Intel
        "coretemp",
        // AMD
        "k10temp", "k8temp", "fam15h_power",
        // ACPI
        "acpi_power_meter", "asus_atk0110", "asus_wmi", "asus_ec_sensors",
        // Dell
        "dell_smm_hwmon", "i8k",
        // ThinkPad
        "thinkpad_acpi",
        // Generic
        "hwmon_vid", "lm75", "lm78", "lm80", "lm83", "lm85", "lm87", "lm90", "lm92", "lm93",
        "lm95241", "lm95245",
        // SMBus/I2C (needed by many sensors)
        "i2c_dev", "i2c_piix4", "i2c_i801",
    ];

    for module in modules {
        let _ = Command::new("modprobe").arg("-q").arg(module).output();
    }

    detect_and_load_chipset_modules();
}

fn detect_and_load_chipset_modules() {
    // Check DMI for specific motherboard vendors
    if let Ok(vendor) = fs::read_to_string("/sys/devices/virtual/dmi/id/board_vendor") {
        let vendor = vendor.trim().to_lowercase();

        match vendor.as_str() {
            v if v.contains("asus") => {
                let _ = Command::new("modprobe").args(["-q", "asus_wmi"]).output();
                let _ = Command::new("modprobe").args(["-q", "asus_ec_sensors"]).output();
                let _ = Command::new("modprobe").args(["-q", "asus_atk0110"]).output();
            }
            v if v.contains("gigabyte") => {
                let _ = Command::new("modprobe").args(["-q", "it87"]).output();
            }
            v if v.contains("msi") => {
                let _ = Command::new("modprobe").args(["-q", "nct6775"]).output();
            }
            v if v.contains("asrock") => {
                let _ = Command::new("modprobe").args(["-q", "nct6775"]).output();
                let _ = Command::new("modprobe").args(["-q", "w83627ehf"]).output();
            }
            v if v.contains("dell") => {
                let _ = Command::new("modprobe").args(["-q", "dell_smm_hwmon"]).output();
            }
            v if v.contains("lenovo") || v.contains("ibm") => {
                let _ = Command::new("modprobe").args(["-q", "thinkpad_acpi"]).output();
            }
            _ => {}
        }
    }

    // Check CPU vendor for CPU temp modules
    if let Ok(cpuinfo) = fs::read_to_string("/proc/cpuinfo") {
        if cpuinfo.contains("GenuineIntel") {
            let _ = Command::new("modprobe").args(["-q", "coretemp"]).output();
        } else if cpuinfo.contains("AuthenticAMD") {
            let _ = Command::new("modprobe").args(["-q", "k10temp"]).output();
            let _ = Command::new("modprobe").args(["-q", "fam15h_power"]).output();
        }
    }

    // Try to load I2C/SMBus modules based on PCI devices
    if let Ok(output) = Command::new("lspci").arg("-n").output() {
        let pci_data = String::from_utf8_lossy(&output.stdout);

        if pci_data.contains("8086:") {
            let _ = Command::new("modprobe").args(["-q", "i2c_i801"]).output();
        }
        if pci_data.contains("1022:") || pci_data.contains("1002:") {
            let _ = Command::new("modprobe").args(["-q", "i2c_piix4"]).output();
        }
    }
}
