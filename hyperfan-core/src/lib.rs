use anyhow::{Result, Context};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Serialize)]
pub struct SystemSummary {
    pub hostname: String,
    pub kernel_version: String,
    pub cpu_model: String,
    pub cpu_cores: u32,
    pub memory_total_mb: u32,
    pub memory_available_mb: u32,
    pub motherboard_name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HwmonChip {
    pub name: String,
    pub path: PathBuf,
    pub temperatures: Vec<TemperatureSensor>,
    pub fans: Vec<FanSensor>,
    pub pwms: Vec<PwmController>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TemperatureSensor {
    pub name: String,
    pub input_path: PathBuf,
    pub label: Option<String>,
    pub current_temp: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FanSensor {
    pub name: String,
    pub input_path: PathBuf,
    pub label: Option<String>,
    pub current_rpm: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PwmController {
    pub name: String,
    pub pwm_path: PathBuf,
    pub enable_path: PathBuf,
    pub label: Option<String>,
    pub current_value: Option<u8>,
    pub current_percent: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FanMapping {
    pub fan_name: String,
    pub pwm_name: String,
    pub confidence: f32,
    pub temp_sources: Vec<TempSource>,  // Available temperature sources for this pairing
    pub response_time_ms: Option<u32>,   // How quickly the fan responds to PWM changes
    pub min_pwm: Option<u8>,             // Minimum PWM value where fan starts
    pub max_rpm: Option<u32>,            // Maximum RPM observed
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TempSource {
    pub sensor_path: PathBuf,
    pub sensor_name: String,
    pub sensor_label: Option<String>,
    pub current_temp: Option<f32>,
    pub chip_name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProbeResult {
    pub pwm_path: PathBuf,
    pub fan_path: PathBuf,
    pub baseline_rpm: u32,
    pub test_rpm: u32,
    pub rpm_delta: i32,
    pub response_time_ms: u32,
    pub confidence: f32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProfileConfig {
    pub mappings: Vec<FanMapping>,
    pub curves: HashMap<String, Vec<CurvePoint>>,
    #[serde(default)]
    pub sensor_names: HashMap<String, String>, // key: temperature input_path, value: user-friendly name
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CurvePoint {
    pub temperature: f32,
    pub fan_percent: f32,
}

pub fn get_system_summary() -> Result<SystemSummary> {
    let hostname = read_hostname();
    let kernel_version = read_kernel_version();
    let cpu_model = read_cpu_name();
    let cpu_cores = read_cpu_cores();
    let (memory_total_mb, memory_available_mb) = read_meminfo_mb();
    let motherboard_name = read_mb_name();
    Ok(SystemSummary {
        hostname,
        kernel_version,
        cpu_model,
        cpu_cores,
        memory_total_mb,
        memory_available_mb,
        motherboard_name,
    })
}

fn read_cpu_name() -> String {
    // Try /proc/cpuinfo model name
    if let Ok(s) = fs::read_to_string("/proc/cpuinfo") {
        for line in s.lines() {
            if let Some((_, name)) = line.split_once(':') {
                if line.to_ascii_lowercase().starts_with("model name") {
                    return name.trim().to_string();
                }
            }
        }
    }
    // Fallback: lscpu like files (rare)
    "Unknown CPU".to_string()
}

fn read_mb_name() -> String {
    // Use DMI info if available
    let vendor = fs::read_to_string("/sys/devices/virtual/dmi/id/board_vendor").unwrap_or_default();
    let name = fs::read_to_string("/sys/devices/virtual/dmi/id/board_name").unwrap_or_default();
    let product = fs::read_to_string("/sys/devices/virtual/dmi/id/product_name").unwrap_or_default();

    let vendor = vendor.trim();
    let name = name.trim();
    let product = product.trim();

    let combined = if !vendor.is_empty() || !name.is_empty() {
        format!("{} {}", vendor, name).trim().to_string()
    } else if !product.is_empty() {
        product.to_string()
    } else {
        String::new()
    };

    if combined.is_empty() { "Unknown Motherboard".to_string() } else { combined }
}

fn read_hostname() -> String {
  if let Ok(s) = fs::read_to_string("/proc/sys/kernel/hostname") {
    let v = s.trim();
    if !v.is_empty() { return v.to_string(); }
  }
  if let Ok(s) = fs::read_to_string("/etc/hostname") {
    let v = s.trim();
    if !v.is_empty() { return v.to_string(); }
  }
  String::from("unknown-host")
}

fn read_kernel_version() -> String {
  if let Ok(s) = fs::read_to_string("/proc/sys/kernel/osrelease") {
    let v = s.trim();
    if !v.is_empty() { return v.to_string(); }
  }
  if let Ok(s) = fs::read_to_string("/proc/version") {
    return s.trim().to_string();
  }
  String::from("unknown-kernel")
}

fn read_cpu_cores() -> u32 {
  if let Ok(s) = fs::read_to_string("/proc/cpuinfo") {
    let count = s
      .lines()
      .filter(|l| l.trim_start().starts_with("processor"))
      .count();
    if count > 0 { return count as u32; }
  }
  1
}

fn read_meminfo_mb() -> (u32, u32) {
  let mut total_kb: u64 = 0;
  let mut avail_kb: u64 = 0;
  if let Ok(s) = fs::read_to_string("/proc/meminfo") {
    for line in s.lines() {
      if line.starts_with("MemTotal:") {
        total_kb = line
          .split_whitespace()
          .nth(1)
          .and_then(|v| v.parse::<u64>().ok())
          .unwrap_or(0);
      } else if line.starts_with("MemAvailable:") {
        avail_kb = line
          .split_whitespace()
          .nth(1)
          .and_then(|v| v.parse::<u64>().ok())
          .unwrap_or(0);
      }
    }
  }
  let total_mb = (total_kb / 1024) as u32;
  let avail_mb = (avail_kb / 1024) as u32;
  (total_mb, avail_mb)
}

pub fn enumerate_hwmon_chips() -> Result<Vec<HwmonChip>> {
    let hwmon_path = Path::new("/sys/class/hwmon");
    let mut chips = Vec::new();

    if !hwmon_path.exists() {
        println!("Warning: /sys/class/hwmon does not exist - no hardware monitoring available");
        return Ok(chips);
    }

    println!("Scanning hwmon chips in {:?}...", hwmon_path);
    
    for entry in fs::read_dir(hwmon_path)? {
        let entry = entry?;
        let path = entry.path();
        println!("  Checking hwmon device: {:?}", path);
        
        if let Some(chip) = read_hwmon_chip(&path)? {
            println!("    ✓ Found chip '{}' with {} temps, {} fans, {} PWMs", 
                     chip.name, chip.temperatures.len(), chip.fans.len(), chip.pwms.len());
            chips.push(chip);
        } else {
            println!("    ✗ Skipped (no useful sensors)");
        }
    }

    println!("Total hwmon chips found: {}", chips.len());
    Ok(chips)
}

fn read_hwmon_chip(chip_path: &Path) -> Result<Option<HwmonChip>> {
    let name_path = chip_path.join("name");
    let name = if name_path.exists() {
        fs::read_to_string(&name_path)?.trim().to_string()
    } else {
        chip_path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string()
    };

    println!("    Reading chip '{}' from {:?}", name, chip_path);

    let mut temperatures = Vec::new();
    let mut fans = Vec::new();
    let mut pwms = Vec::new();

    // Read directory entries
    let entries = fs::read_dir(chip_path)?;
    let mut all_files = Vec::new();
    
    for entry in entries {
        let entry = entry?;
        let file_name = entry.file_name();
        let file_name_str = file_name.to_string_lossy();
        all_files.push(file_name_str.to_string());
    }
    
    println!("      Files found: {:?}", all_files);

    for file_name_str in &all_files {
        if file_name_str.starts_with("temp") && file_name_str.ends_with("_input") {
            println!("        Processing temperature sensor: {}", file_name_str);
            if let Some(temp) = read_temperature_sensor(chip_path, file_name_str)? {
                temperatures.push(temp);
            }
        } else if file_name_str.starts_with("fan") && file_name_str.ends_with("_input") {
            println!("        Processing fan sensor: {}", file_name_str);
            if let Some(fan) = read_fan_sensor(chip_path, file_name_str)? {
                fans.push(fan);
            }
        } else if file_name_str.starts_with("pwm") && !file_name_str.contains("_") {
            println!("        Processing PWM controller: {}", file_name_str);
            if let Some(pwm) = read_pwm_controller(chip_path, file_name_str)? {
                pwms.push(pwm);
            }
        }
    }

    println!("      Final counts - Temps: {}, Fans: {}, PWMs: {}", 
             temperatures.len(), fans.len(), pwms.len());

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

fn read_temperature_sensor(chip_path: &Path, input_file: &str) -> Result<Option<TemperatureSensor>> {
    let input_path = chip_path.join(input_file);
    let base_name = input_file.replace("_input", "");
    let label_path = chip_path.join(format!("{}_label", base_name));
    
    let label = if label_path.exists() {
        Some(fs::read_to_string(&label_path)?.trim().to_string())
    } else {
        None
    };

    let current_temp = if input_path.exists() {
        fs::read_to_string(&input_path)?
            .trim()
            .parse::<i32>()
            .map(|v| v as f32 / 1000.0)
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
    
    println!("          Fan path: {:?} (exists: {})", input_path, input_path.exists());
    
    let label = if label_path.exists() {
        match fs::read_to_string(&label_path) {
            Ok(content) => Some(content.trim().to_string()),
            Err(e) => {
                println!("          Warning: Could not read fan label: {}", e);
                None
            }
        }
    } else {
        None
    };

    let current_rpm = if input_path.exists() {
        match fs::read_to_string(&input_path) {
            Ok(content) => {
                let trimmed = content.trim();
                println!("          Fan RPM read: '{}'", trimmed);
                trimmed.parse::<u32>().ok()
            }
            Err(e) => {
                println!("          Warning: Could not read fan RPM: {}", e);
                None
            }
        }
    } else {
        None
    };

    println!("          ✓ Created fan sensor '{}' (RPM: {:?})", base_name, current_rpm);

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
    
    println!("          PWM path: {:?} (exists: {})", pwm_path, pwm_path.exists());
    
    if !pwm_path.exists() {
        return Ok(None);
    }

    let label = if label_path.exists() {
        match fs::read_to_string(&label_path) {
            Ok(content) => Some(content.trim().to_string()),
            Err(e) => {
                println!("          Warning: Could not read PWM label: {}", e);
                None
            }
        }
    } else {
        None
    };

    let current_value = match fs::read_to_string(&pwm_path) {
        Ok(content) => {
            let trimmed = content.trim();
            println!("          PWM value read: '{}'", trimmed);
            trimmed.parse::<u8>().ok()
        }
        Err(e) => {
            println!("          Warning: Could not read PWM value: {}", e);
            None
        }
    };

    let current_percent = current_value.map(|v| (v as f32 / 255.0) * 100.0);

    println!("          ✓ Created PWM controller '{}' (value: {:?}, percent: {:?})", 
             pwm_file, current_value, current_percent);

    Ok(Some(PwmController {
        name: pwm_file.to_string(),
        pwm_path,
        enable_path,
        label,
        current_value,
        current_percent,
    }))
}

pub fn set_pwm_value(pwm_path: &Path, value: u8) -> Result<()> {
    fs::write(pwm_path, value.to_string())
        .with_context(|| format!("Failed to write PWM value to {:?}", pwm_path))
}

pub fn set_pwm_percent(pwm_path: &Path, percent: f32) -> Result<()> {
    let value = ((percent / 100.0) * 255.0).round() as u8;
    set_pwm_value(pwm_path, value)
}

pub fn load_profile_config() -> Result<Option<ProfileConfig>> {
    let profile_path = Path::new("/etc/hyperfan/profile.json");
    if !profile_path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(profile_path)?;
    let config: ProfileConfig = serde_json::from_str(&content)?;
    Ok(Some(config))
}

pub fn save_profile_config(config: &ProfileConfig) -> Result<()> {
    let profile_dir = Path::new("/etc/hyperfan");
    if !profile_dir.exists() {
        fs::create_dir_all(profile_dir)?;
    }

    let profile_path = profile_dir.join("profile.json");
    let content = serde_json::to_string_pretty(config)?;
    fs::write(profile_path, content)?;
    Ok(())
}

pub fn validate_profile_exists() -> bool {
    Path::new("/etc/hyperfan/profile.json").exists()
}

/// Ultra-advanced auto-detection with active probing for accurate PWM/FAN pairing.
/// This function performs intelligent testing by:
/// 1. Saving current PWM states
/// 2. Testing each PWM controller by changing values
/// 3. Monitoring all fans for RPM changes
/// 4. Calculating confidence based on response correlation
/// 5. Detecting temperature sources available for each pairing
/// 6. Restoring original PWM states
pub fn autodetect_fan_pwm_mappings() -> Result<Vec<FanMapping>> {
    autodetect_fan_pwm_mappings_advanced()
}

/// Legacy heuristic-based detection (fallback if probing fails)
pub fn autodetect_fan_pwm_mappings_heuristic() -> Result<Vec<FanMapping>> {
    let chips = enumerate_hwmon_chips()?;
    let mut mappings: Vec<FanMapping> = Vec::new();

    for chip in chips {
        // Build maps by numeric suffix
        let mut fans_by_idx: HashMap<u32, &FanSensor> = HashMap::new();
        let mut pwms_by_idx: HashMap<u32, &PwmController> = HashMap::new();

        for f in &chip.fans {
            if let Some(idx) = parse_numeric_suffix(&f.name) {
                fans_by_idx.insert(idx, f);
            }
        }
        for p in &chip.pwms {
            if let Some(idx) = parse_numeric_suffix(&p.name) {
                pwms_by_idx.insert(idx, p);
            }
        }

        // First pass: same index
        for (idx, fan) in fans_by_idx.iter() {
            if let Some(pwm) = pwms_by_idx.get(idx) {
                let mut confidence = 0.5; // same index within chip
                confidence += label_similarity_boost(fan.label.as_deref(), pwm.label.as_deref());
                mappings.push(FanMapping {
                    fan_name: format!("{}/{}", chip.name, fan.name),
                    pwm_name: format!("{}/{}", chip.name, pwm.name),
                    confidence: confidence.min(0.9),
                    temp_sources: Vec::new(),
                    response_time_ms: None,
                    min_pwm: None,
                    max_rpm: None,
                });
            }
        }

        // Second pass: fallback pair by order if counts differ
        if mappings.is_empty() {
            let mut i = 0usize;
            while i < chip.fans.len() && i < chip.pwms.len() {
                let fan = &chip.fans[i];
                let pwm = &chip.pwms[i];
                let mut confidence = 0.3; // weak heuristic
                confidence += label_similarity_boost(fan.label.as_deref(), pwm.label.as_deref());
                mappings.push(FanMapping {
                    fan_name: format!("{}/{}", chip.name, fan.name),
                    pwm_name: format!("{}/{}", chip.name, pwm.name),
                    confidence: confidence.min(0.7),
                    temp_sources: Vec::new(),
                    response_time_ms: None,
                    min_pwm: None,
                    max_rpm: None,
                });
                i += 1;
            }
        }
    }

    // Convert to new format with empty temp sources for legacy method
    let mappings_with_temps: Vec<FanMapping> = mappings.into_iter().map(|m| {
        FanMapping {
            fan_name: m.fan_name,
            pwm_name: m.pwm_name,
            confidence: m.confidence,
            temp_sources: Vec::new(),
            response_time_ms: None,
            min_pwm: None,
            max_rpm: None,
        }
    }).collect();
    
    Ok(mappings_with_temps)
}

fn parse_numeric_suffix(name: &str) -> Option<u32> {
    // Extract trailing digits, e.g., fan1 -> 1, pwm2 -> 2
    let digits: String = name.chars().rev().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() { return None; }
    let digits: String = digits.chars().rev().collect();
    digits.parse::<u32>().ok()
}

fn label_similarity_boost(a: Option<&str>, b: Option<&str>) -> f32 {
    match (a, b) {
        (Some(la), Some(lb)) => {
            let la = la.trim().to_ascii_lowercase();
            let lb = lb.trim().to_ascii_lowercase();
            if la.is_empty() || lb.is_empty() { return 0.0; }
            let common = common_prefix_len(&la, &lb);
            if common >= 3 { 0.15 } else if common >= 2 { 0.08 } else { 0.0 }
        }
        _ => 0.0,
    }
}

fn common_prefix_len(a: &str, b: &str) -> usize {
    a.chars().zip(b.chars()).take_while(|(ca, cb)| ca == cb).count()
}

/// Check if we have write permissions to PWM controls (non-destructive)
fn check_pwm_permissions(chips: &[HwmonChip]) -> bool {
    for chip in chips {
        for pwm in &chip.pwms {
            // Check if PWM files are writable without actually writing
            use std::fs::OpenOptions;
            
            // Test PWM enable path
            if pwm.enable_path.exists() {
                if OpenOptions::new().write(true).open(&pwm.enable_path).is_err() {
                    return false;
                }
            }
            
            // Test PWM value path
            if OpenOptions::new().write(true).open(&pwm.pwm_path).is_err() {
                return false;
            }
        }
    }
    true
}

/// Advanced probing-based auto-detection with systematic PWM testing
pub fn autodetect_fan_pwm_mappings_advanced() -> Result<Vec<FanMapping>> {
    let chips = enumerate_hwmon_chips()?;
    
    // Check if we have any PWM controllers at all
    let total_pwm_count: usize = chips.iter().map(|c| c.pwms.len()).sum();
    let total_fan_count: usize = chips.iter().map(|c| c.fans.len()).sum();
    
    if total_pwm_count == 0 {
        let mut error_msg = String::from("No PWM controllers found on this system.\n\n");
        
        if total_fan_count > 0 {
            error_msg.push_str(&format!("Found {} fan sensor(s) but no PWM control interfaces:\n", total_fan_count));
            for chip in &chips {
                for fan in &chip.fans {
                    error_msg.push_str(&format!("  - {}/{} (current: {:?} RPM)\n", 
                                               chip.name, fan.name, fan.current_rpm));
                }
            }
            error_msg.push_str("\nThis typically means:\n");
            error_msg.push_str("• Fan control is handled by BIOS/firmware only\n");
            error_msg.push_str("• PWM control is disabled in BIOS settings\n");
            error_msg.push_str("• Your motherboard doesn't expose PWM control to the OS\n\n");
            error_msg.push_str("Try:\n");
            error_msg.push_str("1. Check BIOS settings for 'Fan Control' or 'PWM Control'\n");
            error_msg.push_str("2. Enable 'Manual Fan Control' or 'Advanced Fan Control'\n");
            error_msg.push_str("3. Look for 'Smart Fan' or 'Q-Fan' settings\n");
        } else {
            error_msg.push_str("No fan sensors or PWM controllers detected.\n");
            error_msg.push_str("Your system may not support hardware fan monitoring/control.");
        }
        
        return Err(anyhow::anyhow!(error_msg));
    }
    
    println!("Found {} PWM controllers across {} chips", total_pwm_count, chips.len());
    
    // Debug: Show what we found
    for chip in &chips {
        println!("Chip '{}' at {:?}:", chip.name, chip.path);
        println!("  PWM controllers: {}", chip.pwms.len());
        for pwm in &chip.pwms {
            println!("    - {} at {:?}", pwm.name, pwm.pwm_path);
        }
        println!("  Fan sensors: {}", chip.fans.len());
        for fan in &chip.fans {
            println!("    - {} at {:?} (current: {:?} RPM)", fan.name, fan.input_path, fan.current_rpm);
        }
    }
    
    // Check permissions first
    if !check_pwm_permissions(&chips) {
        return Err(anyhow::anyhow!(
            "Insufficient permissions to control PWM. Please run as root or with appropriate permissions."
        ));
    }
    
    // Save original PWM states to restore later
    let mut original_states: Vec<(PathBuf, u8)> = Vec::new();
    
    // Collect all PWM controllers and fans
    let mut all_pwms: Vec<&PwmController> = Vec::new();
    let mut all_fans: Vec<(PathBuf, String)> = Vec::new();
    
    for chip in &chips {
        for pwm in &chip.pwms {
            // Save original state
            if let Ok(content) = fs::read_to_string(&pwm.pwm_path) {
                if let Ok(value) = content.trim().parse::<u8>() {
                    original_states.push((pwm.pwm_path.clone(), value));
                }
            }
            all_pwms.push(pwm);
        }
        
        for fan in &chip.fans {
            all_fans.push((fan.input_path.clone(), format!("{}/{}", chip.name, fan.name)));
        }
    }
    
    println!("Found {} fan sensors to monitor", all_fans.len());
    
    // Step 1: Enable manual control and ramp ALL fans to 100%
    println!("Step 1: Ramping all fans to 100%...");
    for pwm in &all_pwms {
        println!("  Setting up PWM controller: {}", path_to_pwm_name(&pwm.pwm_path, &chips));
        
        // Enable manual PWM control first
        if pwm.enable_path.exists() {
            println!("    Enabling manual control at {:?}", pwm.enable_path);
            if let Err(e) = fs::write(&pwm.enable_path, "1") {
                println!("    Warning: Failed to enable manual PWM control: {}", e);
            } else {
                println!("    ✓ Manual control enabled");
            }
        } else {
            println!("    No enable path found - PWM may already be in manual mode");
        }
        
        // Set to 100%
        println!("    Setting PWM to 100% (255) at {:?}", pwm.pwm_path);
        if let Err(e) = fs::write(&pwm.pwm_path, "255") {
            return Err(anyhow::anyhow!(
                "Failed to set PWM to 100% for {}: {}. Check permissions.", 
                pwm.pwm_path.display(), e
            ));
        } else {
            println!("    ✓ PWM set to 100%");
        }
    }
    
    // Wait for all fans to spin up
    println!("Waiting for fans to reach full speed...");
    thread::sleep(Duration::from_millis(4000));
    
    // Step 2: Test each PWM controller one by one
    let mut mappings = Vec::new();
    
    for (pwm_index, pwm) in all_pwms.iter().enumerate() {
        println!("Step 2.{}: Testing PWM controller {}", pwm_index + 1, path_to_pwm_name(&pwm.pwm_path, &chips));
        
        // Read baseline RPMs (all fans at 100%)
        let mut baseline_rpms: HashMap<PathBuf, u32> = HashMap::new();
        for (fan_path, _) in &all_fans {
            if let Ok(content) = fs::read_to_string(fan_path) {
                if let Ok(rpm) = content.trim().parse::<u32>() {
                    baseline_rpms.insert(fan_path.clone(), rpm);
                }
            }
        }
        
        // Set THIS PWM controller to 0 while keeping others at 100%
        if let Err(e) = fs::write(&pwm.pwm_path, "0") {
            println!("Warning: Failed to set PWM to 0 for {}: {}", pwm.pwm_path.display(), e);
            continue;
        }
        
        // Wait 3 seconds for fan to slow down
        println!("  Waiting 3 seconds for fan response...");
        thread::sleep(Duration::from_millis(3000));
        
        // Read RPMs after PWM change
        let mut test_rpms: HashMap<PathBuf, u32> = HashMap::new();
        for (fan_path, _) in &all_fans {
            if let Ok(content) = fs::read_to_string(fan_path) {
                if let Ok(rpm) = content.trim().parse::<u32>() {
                    test_rpms.insert(fan_path.clone(), rpm);
                }
            }
        }
        
        // Find which fan dropped RPM the most
        let mut best_match: Option<(PathBuf, i32, f32)> = None; // (fan_path, rpm_drop, confidence)
        let mut any_fans_detected = false;
        
        for (fan_path, fan_name) in &all_fans {
            if let (Some(baseline), Some(test)) = (baseline_rpms.get(fan_path), test_rpms.get(fan_path)) {
                any_fans_detected = true;
                let rpm_drop = (*baseline as i32) - (*test as i32);
                
                // Calculate confidence based on RPM drop
                let confidence = if *baseline > 0 {
                    let percent_drop = (rpm_drop as f32) / (*baseline as f32) * 100.0;
                    if percent_drop > 80.0 {
                        0.95
                    } else if percent_drop > 60.0 {
                        0.85
                    } else if percent_drop > 40.0 {
                        0.75
                    } else if percent_drop > 20.0 {
                        0.65
                    } else if percent_drop > 10.0 {
                        0.45
                    } else if rpm_drop > 50 {  // Even small drops might be significant
                        0.25
                    } else {
                        0.0
                    }
                } else {
                    0.0
                };
                
                println!("    Fan {}: {} RPM -> {} RPM (drop: {}, {:.1}% drop, confidence: {:.2})", 
                         fan_name, baseline, test, rpm_drop, 
                         if *baseline > 0 { (rpm_drop as f32) / (*baseline as f32) * 100.0 } else { 0.0 },
                         confidence);
                
                // Lower threshold to catch more potential matches
                if rpm_drop > 50 && confidence > 0.2 {
                    if let Some((_, current_drop, current_conf)) = &best_match {
                        if rpm_drop > *current_drop || (rpm_drop == *current_drop && confidence > *current_conf) {
                            best_match = Some((fan_path.clone(), rpm_drop, confidence));
                        }
                    } else {
                        best_match = Some((fan_path.clone(), rpm_drop, confidence));
                    }
                }
            } else {
                println!("    Fan {}: Could not read RPM values (baseline: {:?}, test: {:?})", 
                         fan_name, baseline_rpms.get(fan_path), test_rpms.get(fan_path));
            }
        }
        
        if !any_fans_detected {
            println!("  ⚠️  No fan RPM readings available - fans may not be spinning or sensors not working");
        }
        
        // If we found a good match, create the mapping
        if let Some((fan_path, rpm_drop, confidence)) = best_match {
            let temp_sources = collect_temp_sources(&chips);
            
            println!("  ✓ Matched PWM {} with fan {} (drop: {} RPM, confidence: {:.2})", 
                     path_to_pwm_name(&pwm.pwm_path, &chips),
                     path_to_fan_name(&fan_path, &chips),
                     rpm_drop, confidence);
            
            mappings.push(FanMapping {
                fan_name: path_to_fan_name(&fan_path, &chips),
                pwm_name: path_to_pwm_name(&pwm.pwm_path, &chips),
                confidence,
                temp_sources,
                response_time_ms: Some(3000), // We waited 3 seconds
                min_pwm: None, // We'll determine this later if needed
                max_rpm: baseline_rpms.get(&fan_path).copied(),
            });
        } else {
            println!("  ✗ No clear fan match found for PWM {}", path_to_pwm_name(&pwm.pwm_path, &chips));
        }
        
        // Restore this PWM to 100% before testing the next one
        let _ = fs::write(&pwm.pwm_path, "255");
        thread::sleep(Duration::from_millis(1000)); // Brief wait before next test
    }
    
    // Always restore original PWM states
    println!("Restoring original PWM states...");
    for (path, value) in &original_states {
        let _ = fs::write(path, value.to_string());
    }
    
    println!("Systematic probing found {} mappings", mappings.len());
    
    // If no mappings found, fall back to heuristic method
    if mappings.is_empty() {
        println!("No mappings found via systematic probing, falling back to heuristic method");
        return autodetect_fan_pwm_mappings_heuristic();
    }
    
    Ok(mappings)
}

/// Probe a single PWM controller to find which fans it controls
fn probe_pwm_controller(pwm: &PwmController, all_fans: &[(PathBuf, String)]) -> Result<Vec<ProbeResult>> {
    let mut results = Vec::new();
    
    // Save original PWM value
    let _original_pwm = fs::read_to_string(&pwm.pwm_path)
        .unwrap_or_else(|_| "128".to_string());
    
    // Start from 100% (should already be there from initial setup)
    let _ = fs::write(&pwm.pwm_path, "255");  
    // Wait for stabilization
    for _ in 0..10 {
        thread::sleep(Duration::from_millis(100));
    }
    
    // Read baseline RPMs at 100% state
    let mut baseline_rpms: HashMap<PathBuf, u32> = HashMap::new();
    for (fan_path, _) in all_fans {
        if let Ok(content) = fs::read_to_string(fan_path) {
            if let Ok(rpm) = content.trim().parse::<u32>() {
                baseline_rpms.insert(fan_path.clone(), rpm);
            }
        }
    }
    
    // Set PWM to very low value (10%) for dramatic change
    let _ = fs::write(&pwm.pwm_path, "25");  // 10% of 255
    // Wait for fans to slow down noticeably
    for _ in 0..20 {
        thread::sleep(Duration::from_millis(100));
    }
    
    // Read low RPMs (take average of 3 readings for stability)
    let mut low_rpms: HashMap<PathBuf, u32> = HashMap::new();
    for _ in 0..3 {
        thread::sleep(Duration::from_millis(200));
        for (fan_path, _) in all_fans {
            if let Ok(content) = fs::read_to_string(fan_path) {
                if let Ok(rpm) = content.trim().parse::<u32>() {
                    *low_rpms.entry(fan_path.clone()).or_insert(0) += rpm;
                }
            }
        }
    }
    // Average the readings
    for rpm in low_rpms.values_mut() {
        *rpm /= 3;
    }
    
    // Set PWM back to high value (100%) for dramatic ramp up
    let start_time = Instant::now();
    let _ = fs::write(&pwm.pwm_path, "255");  // 100% of 255
    
    // Monitor RPM changes over time with more iterations
    let mut response_times: HashMap<PathBuf, u32> = HashMap::new();
    let mut max_iterations = 20;  // Check for up to 2 seconds
    
    while max_iterations > 0 {
        thread::sleep(Duration::from_millis(100));
        
        for (fan_path, _) in all_fans {
            if response_times.contains_key(fan_path) {
                continue;  // Already detected response
            }
            
            if let Ok(content) = fs::read_to_string(fan_path) {
                if let Ok(rpm) = content.trim().parse::<u32>() {
                    if let Some(low_rpm) = low_rpms.get(fan_path) {
                        let delta = (rpm as i32) - (*low_rpm as i32);
                        // Lower threshold for detection to catch slower responding fans
                        if delta.abs() > 50 {  // Reduced threshold
                            let response_time = start_time.elapsed().as_millis() as u32;
                            response_times.insert(fan_path.clone(), response_time);
                        }
                    }
                }
            }
        }
        
        max_iterations -= 1;
    }
    
    // Final RPM reading - wait longer and take average
    // Break up the sleep to avoid blocking
    for _ in 0..20 {
        thread::sleep(Duration::from_millis(100));
    }
    
    // Take average of multiple readings for accuracy
    let mut high_rpms: HashMap<PathBuf, u32> = HashMap::new();
    for _ in 0..3 {
        thread::sleep(Duration::from_millis(200));
        for (fan_path, _) in all_fans {
            if let Ok(content) = fs::read_to_string(fan_path) {
                if let Ok(rpm) = content.trim().parse::<u32>() {
                    *high_rpms.entry(fan_path.clone()).or_insert(0) += rpm;
                }
            }
        }
    }
    // Average the readings
    for rpm in high_rpms.values_mut() {
        *rpm /= 3;
    }
    
    // Now perform a second test cycle from high to low for confirmation
    let _ = fs::write(&pwm.pwm_path, "25");  // Back to 10%
    // Wait for fans to slow down again
    for _ in 0..15 {
        thread::sleep(Duration::from_millis(100));
    }
    
    let mut confirm_low_rpms: HashMap<PathBuf, u32> = HashMap::new();
    for _ in 0..3 {
        thread::sleep(Duration::from_millis(200));
        for (fan_path, _) in all_fans {
            if let Ok(content) = fs::read_to_string(fan_path) {
                if let Ok(rpm) = content.trim().parse::<u32>() {
                    *confirm_low_rpms.entry(fan_path.clone()).or_insert(0) += rpm;
                }
            }
        }
    }
    for rpm in confirm_low_rpms.values_mut() {
        *rpm /= 3;
    }
    
    for (fan_path, _fan_name) in all_fans {
        if let (Some(high), Some(low), Some(confirm_low)) = (high_rpms.get(fan_path), low_rpms.get(fan_path), confirm_low_rpms.get(fan_path)) {
            let rpm_delta = (*high as i32) - (*low as i32);
            let confirm_delta = (*high as i32) - (*confirm_low as i32);
            let response_time = response_times.get(fan_path).copied().unwrap_or(9999);
            
            // Calculate confidence based on:
            // 1. RPM delta magnitude
            // 2. Response time
            // 3. Consistency between test cycles
            let mut confidence: f32 = 0.0;
            
            // Use percentage change for better accuracy
            let percent_change = if *low > 0 {
                ((rpm_delta.abs() as f32) / (*low as f32)) * 100.0
            } else if *high > 0 {
                100.0  // Fan was off at low PWM
            } else {
                0.0
            };
            
            if percent_change > 50.0 {
                confidence = 0.95;  // Very strong correlation
            } else if percent_change > 30.0 {
                confidence = 0.85;  // Strong correlation
            } else if percent_change > 20.0 {
                confidence = 0.75;  // Good correlation
            } else if percent_change > 10.0 {
                confidence = 0.65;  // Moderate correlation
            } else if percent_change > 5.0 {
                confidence = 0.45;  // Weak correlation
            }
                    
            // Boost confidence for fast response
            if response_time < 300 {
                confidence += 0.1;
            } else if response_time < 600 {
                confidence += 0.05;
            }
            
            // Check consistency between the two test cycles
            let consistency_ratio = if rpm_delta != 0 {
                (confirm_delta.abs() as f32) / (rpm_delta.abs() as f32)
            } else {
                0.0
            };
            
            // Boost confidence if both cycles showed similar changes
            if consistency_ratio > 0.8 && consistency_ratio < 1.2 {
                confidence += 0.1;  // Very consistent
            } else if consistency_ratio > 0.6 && consistency_ratio < 1.4 {
                confidence += 0.05;  // Reasonably consistent
            } else if consistency_ratio < 0.4 || consistency_ratio > 2.0 {
                confidence *= 0.7;  // Inconsistent results
            }
            
            confidence = confidence.min(1.0);
            
            if confidence > 0.2 {  // Lower threshold to include more potential matches
                results.push(ProbeResult {
                    pwm_path: pwm.pwm_path.clone(),
                    fan_path: fan_path.clone(),
                    baseline_rpm: baseline_rpms.get(fan_path).copied().unwrap_or(0),
                    test_rpm: *high,
                    rpm_delta,
                    response_time_ms: response_time,
                    confidence,
                });
            }
        }
    }
    
    Ok(results)
}

/// Find minimum PWM value where fan starts and maximum RPM
fn find_fan_characteristics(pwm: &PwmController, fan_path: &Path) -> Result<(u8, u32)> {
    let mut min_pwm = 0u8;
    let mut max_rpm = 0u32;
    
    // Test different PWM values to find min start point
    for test_pwm in [0, 25, 50, 75, 100, 125].iter() {
        let _ = fs::write(&pwm.pwm_path, test_pwm.to_string());
        thread::sleep(Duration::from_millis(1000));
        
        if let Ok(content) = fs::read_to_string(fan_path) {
            if let Ok(rpm) = content.trim().parse::<u32>() {
                if rpm > 0 && min_pwm == 0 {
                    min_pwm = *test_pwm;
                }
            }
        }
    }
    
    // Find max RPM at 100%
    let _ = fs::write(&pwm.pwm_path, "255");
    thread::sleep(Duration::from_millis(2000));
    
    if let Ok(content) = fs::read_to_string(fan_path) {
        if let Ok(rpm) = content.trim().parse::<u32>() {
            max_rpm = rpm;
        }
    }
    
    Ok((min_pwm, max_rpm))
}

/// Collect all available temperature sources from all chips
fn collect_temp_sources(chips: &[HwmonChip]) -> Vec<TempSource> {
    let mut sources = Vec::new();
    
    for chip in chips {
        for temp in &chip.temperatures {
            sources.push(TempSource {
                sensor_path: temp.input_path.clone(),
                sensor_name: temp.name.clone(),
                sensor_label: temp.label.clone(),
                current_temp: temp.current_temp,
                chip_name: chip.name.clone(),
            });
        }
    }
    
    sources
}

/// Convert a fan path to a readable name
fn path_to_fan_name(fan_path: &Path, chips: &[HwmonChip]) -> String {
    for chip in chips {
        for fan in &chip.fans {
            if fan.input_path == fan_path {
                return format!("{}/{}", chip.name, fan.name);
            }
        }
    }
    fan_path.to_string_lossy().to_string()
}

/// Convert a PWM path to a readable name
fn path_to_pwm_name(pwm_path: &Path, chips: &[HwmonChip]) -> String {
    for chip in chips {
        for pwm in &chip.pwms {
            if pwm.pwm_path == pwm_path {
                return format!("{}/{}", chip.name, pwm.name);
            }
        }
    }
    pwm_path.to_string_lossy().to_string()
}

/// Load custom sensor names map from the profile (or empty if none exists)
pub fn get_sensor_names() -> Result<HashMap<String, String>> {
    match load_profile_config()? {
        Some(cfg) => Ok(cfg.sensor_names),
        None => Ok(HashMap::new()),
    }
}

/// Set or update a custom sensor name by its input_path key.
/// If no profile exists, one will be created with empty mappings/curves.
pub fn set_sensor_name(key_input_path: String, name: String) -> Result<()> {
    let mut cfg = load_profile_config()?.unwrap_or(ProfileConfig {
        mappings: Vec::new(),
        curves: HashMap::new(),
        sensor_names: HashMap::new(),
    });
    cfg.sensor_names.insert(key_input_path, name);
    save_profile_config(&cfg)
}
