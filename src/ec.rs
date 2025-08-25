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
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde::{Serialize, Deserialize};

use crate::hwmon;
use crate::system::{read_cpu_name, read_mb_name};

#[derive(Debug, Serialize, Deserialize)]
struct EcChip {
    name: String,
    hwmon: String,
    fans: Vec<(usize, String)>,
    pwms: Vec<(usize, String)>,
    temps: Vec<(usize, String)>,
}

#[derive(Debug, Serialize, Deserialize)]
struct EcMappingProfile {
    fan: String,
    pwm: String,
    temp: String,
    confidence: f64,
}

#[derive(Debug, Serialize, Deserialize)]
struct EcProfile {
    ec_name: String,
    motherboard: String,
    cpu: String,
    chips: Vec<EcChip>,
    mappings: Vec<EcMappingProfile>,
}

fn profiles_dir() -> PathBuf {
    PathBuf::from("/etc/hyperfan/profiles")
}

fn ensure_dir(path: &Path) -> io::Result<()> {
    if !path.exists() {
        fs::create_dir_all(path)?;
        let perms = fs::Permissions::from_mode(0o755);
        let _ = fs::set_permissions(path, perms);
    }
    Ok(())
}

fn detect_ec_name() -> String {
    // Prefer a hwmon chip that looks like an EC
    let mut ec_like: Option<String> = None;
    if let Ok(chips) = hwmon::read_all() {
        for chip in chips {
            let lname = chip.name.to_ascii_lowercase();
            if lname.contains("ec") || lname.contains("embedded") {
                ec_like = Some(chip.name);
                break;
            }
        }
    }
    // Fallback to motherboard name
    ec_like.unwrap_or_else(|| {
        let mb = read_mb_name();
        if mb.is_empty() { "unknown-ec".to_string() } else { mb }
    })
}

pub fn dump_ec_profile() -> anyhow::Result<PathBuf> {
    // Load a fresh view of hwmon
    let root = Path::new("/sys/class/hwmon");
    let mut chips_out: Vec<EcChip> = Vec::new();

    if let Ok(entries) = fs::read_dir(root) {
        for ent in entries.flatten() {
            let path = ent.path();
            if !path.is_dir() { continue; }

            let dir = fs::canonicalize(&path).unwrap_or(path);
            let name = fs::read_to_string(dir.join("name")).unwrap_or_else(|_| "unknown".into()).trim().to_string();
            let hwmon_tag = dir.file_name().and_then(|s| s.to_str()).unwrap_or("hwmon?").to_string();

            let mut fans = Vec::new();
            let mut pwms = Vec::new();
            let mut temps = Vec::new();
            if let Ok(dir_iter) = fs::read_dir(&dir) {
                for f in dir_iter.flatten() {
                    let fname = f.file_name();
                    let fname = fname.to_string_lossy();
                    if fname.starts_with("fan") && fname.ends_with("_label") {
                        if let Some(idx) = super::hwmon::extract_index(&fname, "fan", "_label") {
                            let label = fs::read_to_string(f.path()).unwrap_or_default().trim().to_string();
                            fans.push((idx, if label.is_empty() { format!("fan{}", idx) } else { label }));
                        }
                    } else if fname.starts_with("pwm") && fname.ends_with("_label") {
                        if let Some(idx) = super::hwmon::extract_index(&fname, "pwm", "_label") {
                            let label = fs::read_to_string(f.path()).unwrap_or_default().trim().to_string();
                            pwms.push((idx, if label.is_empty() { format!("pwm{}", idx) } else { label }));
                        }
                    } else if fname.starts_with("temp") && fname.ends_with("_label") {
                        if let Some(idx) = super::hwmon::extract_index(&fname, "temp", "_label") {
                            let label = fs::read_to_string(f.path()).unwrap_or_default().trim().to_string();
                            temps.push((idx, if label.is_empty() { format!("temp{}", idx) } else { label }));
                        }
                    }
                }
            }

            // If *_label files are missing, fall back to inputs to at least enumerate indices
            if fans.is_empty() || pwms.is_empty() || temps.is_empty() {
                if let Ok(dir_iter) = fs::read_dir(&dir) {
                    for f in dir_iter.flatten() {
                        let fname = f.file_name();
                        let fname = fname.to_string_lossy();
                        if fans.is_empty() && fname.starts_with("fan") && fname.ends_with("_input") {
                            if let Some(idx) = super::hwmon::extract_index(&fname, "fan", "_input") {
                                fans.push((idx, format!("fan{}", idx)));
                            }
                        }
                        if fname.starts_with("pwm") && !fname.contains('_') {
                            if let Some(idx) = super::hwmon::extract_index(&fname, "pwm", "") {
                                if !pwms.iter().any(|(i, _)| *i == idx) {
                                    pwms.push((idx, format!("pwm{}", idx)));
                                }
                            }
                        }
                        if temps.is_empty() && fname.starts_with("temp") && fname.ends_with("_input") {
                            if let Some(idx) = super::hwmon::extract_index(&fname, "temp", "_input") {
                                temps.push((idx, format!("temp{}", idx)));
                            }
                        }
                    }
                }
            }

            chips_out.push(EcChip { name, hwmon: hwmon_tag, fans, pwms, temps });
        }
    }

    // Attempt to auto-detect pairings to enrich the profile
    let mut mappings: Vec<EcMappingProfile> = Vec::new();
    if let Ok(pairs) = hwmon::auto_detect_pairings() {
        for p in pairs {
            let fan = format!("{}:{}", p.fan_chip, p.fan_label);
            let pwm = format!("{}:{}", p.pwm_chip, p.pwm_label);
            // pick first temp for now; tuning later
            let temp = "temp1".to_string();
            mappings.push(EcMappingProfile { fan, pwm, temp, confidence: p.confidence });
        }
    }

    let profile = EcProfile {
        ec_name: sanitize_name(&detect_ec_name()),
        motherboard: read_mb_name(),
        cpu: read_cpu_name(),
        chips: chips_out,
        mappings,
    };

    // Write JSON
    let dir = profiles_dir();
    ensure_dir(&dir)?;
    let fname = format!("{}.json", profile.ec_name);
    let out_path = dir.join(fname);
    let json = serde_json::to_string_pretty(&profile)?;
    fs::write(&out_path, json)?;
    // set 0644
    let perms = fs::Permissions::from_mode(0o644);
    let _ = fs::set_permissions(&out_path, perms);

    println!("EC profile written to {}", out_path.display());
    Ok(out_path)
}

fn sanitize_name(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
            out.push(c);
        } else if c.is_whitespace() {
            out.push('_');
        }
    }
    if out.is_empty() { "ec".into() } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;

    fn create_test_ec_chip() -> EcChip {
        EcChip {
            name: "test_chip".to_string(),
            hwmon: "hwmon0".to_string(),
            fans: vec![(1, "fan1".to_string()), (2, "fan2".to_string())],
            pwms: vec![(1, "pwm1".to_string()), (2, "pwm2".to_string())],
            temps: vec![(1, "temp1".to_string()), (2, "temp2".to_string())],
        }
    }

    fn create_test_ec_mapping_profile() -> EcMappingProfile {
        EcMappingProfile {
            fan: "chip1:fan1".to_string(),
            pwm: "chip1:pwm1".to_string(),
            temp: "temp1".to_string(),
            confidence: 0.85,
        }
    }

    fn create_test_ec_profile() -> EcProfile {
        EcProfile {
            ec_name: "test_ec".to_string(),
            motherboard: "Test Motherboard".to_string(),
            cpu: "Test CPU".to_string(),
            chips: vec![create_test_ec_chip()],
            mappings: vec![create_test_ec_mapping_profile()],
        }
    }

    #[test]
    fn test_sanitize_name_alphanumeric() {
        assert_eq!(sanitize_name("test123"), "test123");
        assert_eq!(sanitize_name("TestChip"), "TestChip");
    }

    #[test]
    fn test_sanitize_name_allowed_chars() {
        assert_eq!(sanitize_name("test-chip"), "test-chip");
        assert_eq!(sanitize_name("test_chip"), "test_chip");
        assert_eq!(sanitize_name("test.chip"), "test.chip");
    }

    #[test]
    fn test_sanitize_name_whitespace() {
        assert_eq!(sanitize_name("test chip"), "test_chip");
        assert_eq!(sanitize_name("test\tchip"), "test_chip");
        assert_eq!(sanitize_name("test\nchip"), "test_chip");
        assert_eq!(sanitize_name("  test  chip  "), "__test__chip__");
    }

    #[test]
    fn test_sanitize_name_special_chars() {
        assert_eq!(sanitize_name("test@chip"), "testchip");
        assert_eq!(sanitize_name("test#chip"), "testchip");
        assert_eq!(sanitize_name("test$chip"), "testchip");
        assert_eq!(sanitize_name("test/chip"), "testchip");
    }

    #[test]
    fn test_sanitize_name_empty() {
        assert_eq!(sanitize_name(""), "ec");
        assert_eq!(sanitize_name("   "), "___");
        assert_eq!(sanitize_name("@#$"), "ec");
    }

    #[test]
    fn test_sanitize_name_mixed() {
        assert_eq!(sanitize_name("Test Chip-1.0@hwmon"), "Test_Chip-1.0hwmon");
        assert_eq!(sanitize_name("EC/Controller #1"), "ECController_1");
    }

    #[test]
    fn test_profiles_dir() {
        let dir = profiles_dir();
        assert_eq!(dir, PathBuf::from("/etc/hyperfan/profiles"));
    }

    #[test]
    fn test_ensure_dir_creates_directory() {
        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().join("test_subdir");
        
        assert!(!test_path.exists());
        ensure_dir(&test_path).unwrap();
        assert!(test_path.exists());
        assert!(test_path.is_dir());
    }

    #[test]
    fn test_ensure_dir_existing_directory() {
        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().join("existing_dir");
        fs::create_dir(&test_path).unwrap();
        
        assert!(test_path.exists());
        ensure_dir(&test_path).unwrap();
        assert!(test_path.exists());
        assert!(test_path.is_dir());
    }

    #[test]
    fn test_detect_ec_name_fallback() {
        // This test will likely use the motherboard name fallback since we're in a test environment
        let ec_name = detect_ec_name();
        assert!(!ec_name.is_empty());
        // Should either be an EC name or fallback to motherboard/unknown-ec
        assert!(ec_name.len() > 0);
    }

    #[test]
    fn test_ec_chip_serialization() {
        let chip = create_test_ec_chip();
        let json = serde_json::to_string(&chip).unwrap();
        
        assert!(json.contains("test_chip"));
        assert!(json.contains("hwmon0"));
        assert!(json.contains("fan1"));
        assert!(json.contains("pwm1"));
        assert!(json.contains("temp1"));
    }

    #[test]
    fn test_ec_mapping_profile_serialization() {
        let mapping = create_test_ec_mapping_profile();
        let json = serde_json::to_string(&mapping).unwrap();
        
        assert!(json.contains("chip1:fan1"));
        assert!(json.contains("chip1:pwm1"));
        assert!(json.contains("temp1"));
        assert!(json.contains("0.85"));
    }

    #[test]
    fn test_ec_profile_serialization() {
        let profile = create_test_ec_profile();
        let json = serde_json::to_string(&profile).unwrap();
        
        assert!(json.contains("test_ec"));
        assert!(json.contains("Test Motherboard"));
        assert!(json.contains("Test CPU"));
        assert!(json.contains("test_chip"));
        assert!(json.contains("chip1:fan1"));
    }

    #[test]
    fn test_ec_profile_deserialization() {
        let profile = create_test_ec_profile();
        let json = serde_json::to_string(&profile).unwrap();
        let deserialized: EcProfile = serde_json::from_str(&json).unwrap();
        
        assert_eq!(deserialized.ec_name, profile.ec_name);
        assert_eq!(deserialized.motherboard, profile.motherboard);
        assert_eq!(deserialized.cpu, profile.cpu);
        assert_eq!(deserialized.chips.len(), profile.chips.len());
        assert_eq!(deserialized.mappings.len(), profile.mappings.len());
    }

    #[test]
    fn test_ec_chip_fields() {
        let chip = create_test_ec_chip();
        
        assert_eq!(chip.name, "test_chip");
        assert_eq!(chip.hwmon, "hwmon0");
        assert_eq!(chip.fans.len(), 2);
        assert_eq!(chip.pwms.len(), 2);
        assert_eq!(chip.temps.len(), 2);
        
        assert_eq!(chip.fans[0], (1, "fan1".to_string()));
        assert_eq!(chip.pwms[0], (1, "pwm1".to_string()));
        assert_eq!(chip.temps[0], (1, "temp1".to_string()));
    }

    #[test]
    fn test_ec_mapping_profile_fields() {
        let mapping = create_test_ec_mapping_profile();
        
        assert_eq!(mapping.fan, "chip1:fan1");
        assert_eq!(mapping.pwm, "chip1:pwm1");
        assert_eq!(mapping.temp, "temp1");
        assert_eq!(mapping.confidence, 0.85);
    }

    #[test]
    fn test_ec_profile_fields() {
        let profile = create_test_ec_profile();
        
        assert_eq!(profile.ec_name, "test_ec");
        assert_eq!(profile.motherboard, "Test Motherboard");
        assert_eq!(profile.cpu, "Test CPU");
        assert_eq!(profile.chips.len(), 1);
        assert_eq!(profile.mappings.len(), 1);
    }

    // Note: dump_ec_profile() is harder to test in isolation since it reads from /sys/class/hwmon
    // and writes to /etc/hyperfan/profiles. In a real test environment, we'd need to mock
    // the filesystem or use dependency injection to make it testable.
}
