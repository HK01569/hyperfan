//! Fan-to-PWM mapping detection
//!
//! Provides both heuristic and active probing methods to accurately
//! determine which PWM controller controls which fan.
//!
//! # Detection Methods
//!
//! 1. **Active Probing** (primary): Sets each PWM to 0%, measures which
//!    fans slow down, calculates confidence based on RPM drop percentage.
//!
//! 2. **Heuristic Matching** (fallback): Matches fans to PWMs by index
//!    (fan1 -> pwm1) or label similarity when probing isn't possible.
//!
//! # Fingerprinting
//!
//! Detection now also creates comprehensive fingerprints for all discovered
//! sensors to prevent mispairing across reboots.

use crate::error::Result;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use tracing::{debug, info, trace, warn};

use crate::constants::{
    detection::{confidence_scores, heuristic, rpm_drop_thresholds},
    limits,
    timing,
};
use crate::data::{FanMapping, HwmonChip, PwmController, TempSource};
use crate::hw::binding::BindingStore;
use crate::hw::fingerprint::{
    extract_chip_fingerprint, extract_channel_fingerprint, extract_pwm_fingerprint,
    ChannelType, PwmProbeData,
};
use crate::hw::hardware::{check_pwm_permissions, enumerate_hwmon_chips};

/// Ultra-advanced auto-detection with active probing for accurate PWM/FAN pairing.
pub fn autodetect_fan_pwm_mappings() -> Result<Vec<FanMapping>> {
    autodetect_fan_pwm_mappings_advanced()
}

/// Legacy heuristic-based detection (fallback if probing fails)
///
/// Matches fans to PWMs using two strategies:
/// 1. Index matching: fan1 -> pwm1, fan2 -> pwm2, etc.
/// 2. Position matching: first fan -> first PWM (when indices unavailable)
pub fn autodetect_fan_pwm_mappings_heuristic() -> Result<Vec<FanMapping>> {
    let chips = enumerate_hwmon_chips()?;
    let mut mappings: Vec<FanMapping> = Vec::new();

    for chip in chips {
        let mut chip_mappings: Vec<FanMapping> = Vec::new();
        // Build lookup tables: numeric index -> sensor
        let mut fans_by_index: HashMap<u32, _> = HashMap::new();
        let mut pwms_by_index: HashMap<u32, _> = HashMap::new();

        for fan in &chip.fans {
            if let Some(index) = extract_sensor_index(&fan.name) {
                fans_by_index.insert(index, fan);
            }
        }
        for pwm in &chip.pwms {
            if let Some(index) = extract_sensor_index(&pwm.name) {
                pwms_by_index.insert(index, pwm);
            }
        }

        // Strategy 1: Match by numeric index (fan1 -> pwm1)
        for (index, fan) in fans_by_index.iter() {
            if let Some(pwm) = pwms_by_index.get(index) {
                let base_confidence = heuristic::INDEX_MATCH_BASE;
                let label_bonus = calculate_label_similarity_bonus(
                    fan.label.as_deref(),
                    pwm.label.as_deref(),
                );
                let final_confidence = (base_confidence + label_bonus).min(heuristic::INDEX_MATCH_CAP);

                chip_mappings.push(FanMapping {
                    fan_name: format!("{}/{}", chip.name, fan.name),
                    pwm_name: format!("{}/{}", chip.name, pwm.name),
                    confidence: final_confidence,
                    temp_sources: Vec::new(),
                    response_time_ms: None,
                    min_pwm: None,
                    max_rpm: None,
                });
            }
        }

        // Strategy 2: Position-based matching (fallback when no index matches found)
        if chip_mappings.is_empty() {
            let fan_count = chip.fans.len();
            let pwm_count = chip.pwms.len();
            let pairs_to_create = fan_count.min(pwm_count);

            for position in 0..pairs_to_create {
                let fan = &chip.fans[position];
                let pwm = &chip.pwms[position];

                let base_confidence = heuristic::POSITION_MATCH_BASE;
                let label_bonus = calculate_label_similarity_bonus(
                    fan.label.as_deref(),
                    pwm.label.as_deref(),
                );
                let final_confidence = (base_confidence + label_bonus).min(heuristic::POSITION_MATCH_CAP);

                chip_mappings.push(FanMapping {
                    fan_name: format!("{}/{}", chip.name, fan.name),
                    pwm_name: format!("{}/{}", chip.name, pwm.name),
                    confidence: final_confidence,
                    temp_sources: Vec::new(),
                    response_time_ms: None,
                    min_pwm: None,
                    max_rpm: None,
                });
            }
        }

        mappings.extend(chip_mappings);
    }

    Ok(mappings)
}

/// Extract the numeric index from a sensor name (e.g., "fan1" -> 1, "pwm2" -> 2)
fn extract_sensor_index(sensor_name: &str) -> Option<u32> {
    // Read digits from the end of the name (handles "fan1", "pwm12", etc.)
    let trailing_digits: String = sensor_name
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect();

    if trailing_digits.is_empty() {
        return None;
    }

    // Reverse back to correct order and parse
    let digits_correct_order: String = trailing_digits.chars().rev().collect();
    digits_correct_order.parse::<u32>().ok()
}

/// Calculate a confidence bonus based on label similarity between fan and PWM
///
/// Labels like "CPU Fan" and "CPU PWM" would get a bonus for the common "CPU" prefix.
fn calculate_label_similarity_bonus(fan_label: Option<&str>, pwm_label: Option<&str>) -> f32 {
    match (fan_label, pwm_label) {
        (Some(fan_lbl), Some(pwm_lbl)) => {
            let fan_normalized = fan_lbl.trim().to_ascii_lowercase();
            let pwm_normalized = pwm_lbl.trim().to_ascii_lowercase();

            if fan_normalized.is_empty() || pwm_normalized.is_empty() {
                return 0.0;
            }

            let common_prefix_length = count_common_prefix_chars(&fan_normalized, &pwm_normalized);

            if common_prefix_length >= limits::LABEL_PREFIX_STRONG {
                heuristic::LABEL_MATCH_STRONG
            } else if common_prefix_length >= limits::LABEL_PREFIX_WEAK {
                heuristic::LABEL_MATCH_WEAK
            } else {
                0.0
            }
        }
        _ => 0.0, // No bonus if either label is missing
    }
}

/// Count how many characters match at the start of two strings
fn count_common_prefix_chars(string_a: &str, string_b: &str) -> usize {
    string_a
        .chars()
        .zip(string_b.chars())
        .take_while(|(char_a, char_b)| char_a == char_b)
        .count()
}

/// Advanced probing-based auto-detection with systematic PWM testing
pub fn autodetect_fan_pwm_mappings_advanced() -> Result<Vec<FanMapping>> {
    let chips = enumerate_hwmon_chips()?;

    let total_pwm_count: usize = chips.iter().map(|c| c.pwms.len()).sum();
    let total_fan_count: usize = chips.iter().map(|c| c.fans.len()).sum();

    if total_pwm_count == 0 {
        let mut error_msg = String::from("No PWM controllers found on this system.\n\n");

        if total_fan_count > 0 {
            error_msg.push_str(&format!(
                "Found {} fan sensor(s) but no PWM control interfaces:\n",
                total_fan_count
            ));
            for chip in &chips {
                for fan in &chip.fans {
                    error_msg.push_str(&format!(
                        "  - {}/{} (current: {:?} RPM)\n",
                        chip.name, fan.name, fan.current_rpm
                    ));
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

        return Err(crate::error::HyperfanError::HardwareNotFound(error_msg));
    }

    info!(
        pwm_count = total_pwm_count,
        chip_count = chips.len(),
        "Starting PWM-fan detection"
    );

    for chip in &chips {
        debug!(
            chip = %chip.name,
            pwms = chip.pwms.len(),
            fans = chip.fans.len(),
            "Chip details"
        );
    }

    if !check_pwm_permissions(&chips) {
        return Err(crate::error::HyperfanError::PermissionDenied(
            "Insufficient permissions to control PWM. Please run as root or with appropriate permissions.".to_string()
        ));
    }

    let mut original_states: Vec<(PathBuf, u8)> = Vec::new();
    let mut all_pwms: Vec<&PwmController> = Vec::new();
    let mut all_fans: Vec<(PathBuf, String)> = Vec::new();

    for chip in &chips {
        for pwm in &chip.pwms {
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

    debug!(fan_count = all_fans.len(), "Fans to monitor");

    info!("Step 1: Ramping all fans to 100%");
    for pwm in &all_pwms {
        let pwm_name = path_to_pwm_name(&pwm.pwm_path, &chips);
        trace!(pwm = %pwm_name, "Setting up PWM controller");

        if pwm.enable_path.exists() {
            if let Err(e) = fs::write(&pwm.enable_path, "1") {
                warn!(pwm = %pwm_name, error = %e, "Failed to enable manual PWM control");
            } else {
                trace!(pwm = %pwm_name, "Manual control enabled");
            }
        }

        if let Err(e) = fs::write(&pwm.pwm_path, "255") {
            return Err(crate::error::HyperfanError::PwmWrite {
                path: pwm.pwm_path.clone(),
                reason: format!("Failed to set PWM to 100%: {}. Check permissions.", e)
            });
        }
    }

    debug!("Waiting for fans to reach full speed");
    thread::sleep(timing::FAN_SPINUP);

    let mut mappings = Vec::new();

    for (pwm_index, pwm) in all_pwms.iter().enumerate() {
        let pwm_name = path_to_pwm_name(&pwm.pwm_path, &chips);
        info!(step = pwm_index + 1, pwm = %pwm_name, "Testing PWM controller");

        let mut baseline_rpms: HashMap<PathBuf, u32> = HashMap::new();
        for (fan_path, _) in &all_fans {
            if let Ok(content) = fs::read_to_string(fan_path) {
                if let Ok(rpm) = content.trim().parse::<u32>() {
                    baseline_rpms.insert(fan_path.clone(), rpm);
                }
            }
        }

        if let Err(e) = fs::write(&pwm.pwm_path, "0") {
            warn!(pwm = %pwm_name, error = %e, "Failed to set PWM to 0");
            continue;
        }

        trace!("Waiting for fan response");
        thread::sleep(timing::FAN_STABILIZATION);

        let mut test_rpms: HashMap<PathBuf, u32> = HashMap::new();
        for (fan_path, _) in &all_fans {
            if let Ok(content) = fs::read_to_string(fan_path) {
                if let Ok(rpm) = content.trim().parse::<u32>() {
                    test_rpms.insert(fan_path.clone(), rpm);
                }
            }
        }

        let mut best_match: Option<(PathBuf, i32, f32)> = None;
        let mut any_fans_detected = false;

        for (fan_path, fan_name) in &all_fans {
            if let (Some(baseline), Some(test)) =
                (baseline_rpms.get(fan_path), test_rpms.get(fan_path))
            {
                any_fans_detected = true;
                let rpm_drop = (*baseline as i32) - (*test as i32);

                // Calculate confidence based on how much the fan slowed down
                let confidence = if *baseline > 0 {
                    let percent_drop = (rpm_drop as f32) / (*baseline as f32) * 100.0;

                    // Higher percentage drops indicate stronger PWM-fan correlation
                    if percent_drop > rpm_drop_thresholds::VERY_HIGH {
                        confidence_scores::VERY_HIGH_DROP
                    } else if percent_drop > rpm_drop_thresholds::HIGH {
                        confidence_scores::HIGH_DROP
                    } else if percent_drop > rpm_drop_thresholds::MEDIUM {
                        confidence_scores::MEDIUM_DROP
                    } else if percent_drop > rpm_drop_thresholds::LOW {
                        confidence_scores::LOW_DROP
                    } else if percent_drop > rpm_drop_thresholds::MINIMAL {
                        confidence_scores::MINIMAL_DROP
                    } else if rpm_drop > crate::constants::detection::MIN_RPM_DROP {
                        // Small percentage but absolute drop exceeds threshold
                        confidence_scores::ABSOLUTE_DROP
                    } else {
                        0.0 // No significant response detected
                    }
                } else {
                    0.0 // Can't calculate confidence without baseline RPM
                };

                trace!(
                    fan = %fan_name,
                    baseline_rpm = baseline,
                    test_rpm = test,
                    rpm_drop = rpm_drop,
                    confidence = format!("{:.2}", confidence),
                    "Fan response measured"
                );

                if rpm_drop > crate::constants::detection::MIN_RPM_DROP && confidence > crate::constants::detection::MIN_CONFIDENCE {
                    if let Some((_, current_drop, current_conf)) = &best_match {
                        if rpm_drop > *current_drop
                            || (rpm_drop == *current_drop && confidence > *current_conf)
                        {
                            best_match = Some((fan_path.clone(), rpm_drop, confidence));
                        }
                    } else {
                        best_match = Some((fan_path.clone(), rpm_drop, confidence));
                    }
                }
            } else {
                trace!(fan = %fan_name, "Could not read RPM values");
            }
        }

        if !any_fans_detected {
            warn!(pwm = %pwm_name, "No fan RPM readings available");
        }

        if let Some((fan_path, rpm_drop, confidence)) = best_match {
            let temp_sources = collect_temp_sources(&chips);
            let fan_name = path_to_fan_name(&fan_path, &chips);

            info!(
                pwm = %pwm_name,
                fan = %fan_name,
                rpm_drop = rpm_drop,
                confidence = format!("{:.2}", confidence),
                "Matched PWM to fan"
            );

            mappings.push(FanMapping {
                fan_name: fan_name.clone(),
                pwm_name: path_to_pwm_name(&pwm.pwm_path, &chips),
                confidence,
                temp_sources,
                response_time_ms: Some(timing::FAN_STABILIZATION_MS),
                min_pwm: None,
                max_rpm: baseline_rpms.get(&fan_path).copied(),
            });
        } else {
            debug!(pwm = %pwm_name, "No clear fan match found");
        }

        let _ = fs::write(&pwm.pwm_path, "255");
        thread::sleep(timing::DETECTION_DELAY);
    }

    info!("Restoring original PWM states");
    for (path, value) in &original_states {
        let _ = fs::write(path, value.to_string());
    }

    info!(mappings_found = mappings.len(), "Detection complete");

    if mappings.is_empty() {
        debug!("No mappings found via probing, falling back to heuristic method");
        return autodetect_fan_pwm_mappings_heuristic();
    }

    Ok(mappings)
}

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

// ============================================================================
// Fingerprinted Detection
// ============================================================================

/// Result of fingerprinted detection including both mappings and binding store
#[derive(Debug)]
pub struct FingerprintedDetectionResult {
    /// Traditional fan mappings (for backwards compatibility)
    pub mappings: Vec<FanMapping>,
    /// Comprehensive binding store with all fingerprints
    pub binding_store: BindingStore,
    /// Number of PWM channels detected
    pub pwm_count: usize,
    /// Number of fan sensors detected
    pub fan_count: usize,
    /// Number of temperature sensors detected
    pub temp_count: usize,
}

/// Perform PWM-fan detection with comprehensive fingerprinting
///
/// This is the recommended detection method that creates validated bindings
/// with full fingerprints to prevent sensor mispairing across reboots.
pub fn autodetect_with_fingerprints() -> Result<FingerprintedDetectionResult> {
    let chips = enumerate_hwmon_chips()?;
    
    let total_pwm_count: usize = chips.iter().map(|c| c.pwms.len()).sum();
    let total_fan_count: usize = chips.iter().map(|c| c.fans.len()).sum();
    let total_temp_count: usize = chips.iter().map(|c| c.temperatures.len()).sum();
    
    if total_pwm_count == 0 {
        return Err(crate::error::HyperfanError::HardwareNotFound(
            "No PWM controllers found on this system. Fan control not available.".to_string()
        ));
    }
    
    info!(
        pwm_count = total_pwm_count,
        fan_count = total_fan_count,
        temp_count = total_temp_count,
        chip_count = chips.len(),
        "Starting fingerprinted PWM-fan detection"
    );
    
    // Create binding store and fingerprint all sensors
    let mut store = BindingStore::new();
    let mut chip_fingerprints = HashMap::new();
    
    // Phase 1: Fingerprint all chips and sensors
    for chip in &chips {
        if let Some(chip_fp) = extract_chip_fingerprint(&chip.path) {
            let chip_id = store.register_chip(chip_fp.clone());
            chip_fingerprints.insert(chip.path.clone(), (chip_id.clone(), chip_fp.clone()));
            
            debug!(
                chip = %chip.name,
                driver = %chip_fp.driver_name,
                chip_class = ?chip_fp.chip_class,
                "Fingerprinted chip"
            );
            
            // Fingerprint temperature sensors
            for temp in &chip.temperatures {
                let temp_fp = extract_channel_fingerprint(
                    &chip_fp,
                    ChannelType::Temperature,
                    &temp.name,
                    &temp.input_path,
                );
                store.register_temp_channel(temp_fp);
            }
            
            // Fingerprint fan sensors
            for fan in &chip.fans {
                let fan_fp = extract_channel_fingerprint(
                    &chip_fp,
                    ChannelType::Fan,
                    &fan.name,
                    &fan.input_path,
                );
                store.register_fan_channel(fan_fp);
            }
            
            // Fingerprint PWM controllers
            for pwm in &chip.pwms {
                let pwm_fp = extract_pwm_fingerprint(
                    &chip_fp,
                    &pwm.name,
                    &pwm.pwm_path,
                    &pwm.enable_path,
                );
                store.register_pwm_channel(pwm_fp);
            }
        }
    }
    
    info!(
        chips = store.chips.len(),
        pwm_channels = store.pwm_channels.len(),
        fan_channels = store.fan_channels.len(),
        temp_channels = store.temp_channels.len(),
        "Phase 1 complete: All sensors fingerprinted"
    );
    
    // Phase 2: Check permissions
    if !check_pwm_permissions(&chips) {
        return Err(crate::error::HyperfanError::PermissionDenied(
            "Insufficient permissions to control PWM. Please run as root or with appropriate permissions.".to_string()
        ));
    }
    
    // Phase 3: Active probing with fingerprint correlation
    let mut original_states: Vec<(PathBuf, u8)> = Vec::new();
    let mut all_pwms: Vec<(&PwmController, PathBuf)> = Vec::new();
    let mut all_fans: Vec<(PathBuf, String, PathBuf)> = Vec::new(); // (input_path, name, chip_path)
    
    for chip in &chips {
        for pwm in &chip.pwms {
            if let Ok(content) = fs::read_to_string(&pwm.pwm_path) {
                if let Ok(value) = content.trim().parse::<u8>() {
                    original_states.push((pwm.pwm_path.clone(), value));
                }
            }
            all_pwms.push((pwm, chip.path.clone()));
        }
        
        for fan in &chip.fans {
            all_fans.push((fan.input_path.clone(), fan.name.clone(), chip.path.clone()));
        }
    }
    
    // Ramp all fans to 100%
    info!("Phase 3: Ramping all fans to 100% for baseline measurement");
    for (pwm, _) in &all_pwms {
        if pwm.enable_path.exists() {
            let _ = fs::write(&pwm.enable_path, "1");
        }
        if let Err(e) = fs::write(&pwm.pwm_path, "255") {
            warn!(path = ?pwm.pwm_path, error = %e, "Failed to set PWM to 100%");
        }
    }
    
    thread::sleep(timing::FAN_SPINUP);
    
    // Phase 4: Test each PWM and correlate with fans
    let mut mappings = Vec::new();
    
    for (pwm_index, (pwm, chip_path)) in all_pwms.iter().enumerate() {
        let pwm_name = path_to_pwm_name(&pwm.pwm_path, &chips);
        info!(step = pwm_index + 1, pwm = %pwm_name, "Testing PWM controller");
        
        // Read baseline RPMs
        let mut baseline_rpms: HashMap<PathBuf, u32> = HashMap::new();
        for (fan_path, _, _) in &all_fans {
            if let Ok(content) = fs::read_to_string(fan_path) {
                if let Ok(rpm) = content.trim().parse::<u32>() {
                    baseline_rpms.insert(fan_path.clone(), rpm);
                }
            }
        }
        
        // Set PWM to 0
        if let Err(e) = fs::write(&pwm.pwm_path, "0") {
            warn!(pwm = %pwm_name, error = %e, "Failed to set PWM to 0");
            continue;
        }
        
        thread::sleep(timing::FAN_STABILIZATION);
        
        // Read test RPMs
        let mut test_rpms: HashMap<PathBuf, u32> = HashMap::new();
        for (fan_path, _, _) in &all_fans {
            if let Ok(content) = fs::read_to_string(fan_path) {
                if let Ok(rpm) = content.trim().parse::<u32>() {
                    test_rpms.insert(fan_path.clone(), rpm);
                }
            }
        }
        
        // Find best matching fan
        let mut best_match: Option<(PathBuf, String, PathBuf, i32, f32)> = None;
        
        for (fan_path, fan_name, fan_chip_path) in &all_fans {
            if let (Some(baseline), Some(test)) = (baseline_rpms.get(fan_path), test_rpms.get(fan_path)) {
                let rpm_drop = (*baseline as i32) - (*test as i32);
                
                let confidence = if *baseline > 0 {
                    let percent_drop = (rpm_drop as f32) / (*baseline as f32) * 100.0;
                    
                    if percent_drop > rpm_drop_thresholds::VERY_HIGH {
                        confidence_scores::VERY_HIGH_DROP
                    } else if percent_drop > rpm_drop_thresholds::HIGH {
                        confidence_scores::HIGH_DROP
                    } else if percent_drop > rpm_drop_thresholds::MEDIUM {
                        confidence_scores::MEDIUM_DROP
                    } else if percent_drop > rpm_drop_thresholds::LOW {
                        confidence_scores::LOW_DROP
                    } else if percent_drop > rpm_drop_thresholds::MINIMAL {
                        confidence_scores::MINIMAL_DROP
                    } else if rpm_drop > crate::constants::detection::MIN_RPM_DROP {
                        confidence_scores::ABSOLUTE_DROP
                    } else {
                        0.0
                    }
                } else {
                    0.0
                };
                
                if rpm_drop > crate::constants::detection::MIN_RPM_DROP && confidence > crate::constants::detection::MIN_CONFIDENCE {
                    if let Some((_, _, _, current_drop, current_conf)) = &best_match {
                        if rpm_drop > *current_drop || (rpm_drop == *current_drop && confidence > *current_conf) {
                            best_match = Some((fan_path.clone(), fan_name.clone(), fan_chip_path.clone(), rpm_drop, confidence));
                        }
                    } else {
                        best_match = Some((fan_path.clone(), fan_name.clone(), fan_chip_path.clone(), rpm_drop, confidence));
                    }
                }
            }
        }
        
        // Create binding if match found
        if let Some((fan_path, fan_name, fan_chip_path, rpm_drop, confidence)) = best_match {
            let temp_sources = collect_temp_sources(&chips);
            let full_fan_name = path_to_fan_name(&fan_path, &chips);
            
            info!(
                pwm = %pwm_name,
                fan = %full_fan_name,
                rpm_drop = rpm_drop,
                confidence = format!("{:.2}", confidence),
                "Matched PWM to fan with fingerprinting"
            );
            
            // Create probe data for the binding
            let probe_data = PwmProbeData {
                response_map: vec![(0, *test_rpms.get(&fan_path).unwrap_or(&0)), (255, *baseline_rpms.get(&fan_path).unwrap_or(&0))],
                rpm_delta_on_step: Some(rpm_drop),
                write_capability: true,
                control_authority_override: false,
                response_time_ms: Some(timing::FAN_STABILIZATION.as_millis() as u32),
            };
            
            // Find PWM and fan fingerprint IDs
            let pwm_fp_id = find_pwm_fingerprint_id(&store, chip_path, &pwm.name);
            let fan_fp_id = find_fan_fingerprint_id(&store, &fan_chip_path, &fan_name);
            
            // Create validated binding
            if let Some(pwm_id) = pwm_fp_id {
                if let Err(e) = store.create_binding(
                    &pwm_id,
                    fan_fp_id.as_deref(),
                    None, // temp source selected later by user
                    Some(probe_data),
                ) {
                    warn!(error = %e, "Failed to create binding");
                }
            }
            
            // Also create traditional mapping for backwards compatibility
            mappings.push(FanMapping {
                fan_name: full_fan_name,
                pwm_name: pwm_name.clone(),
                confidence,
                temp_sources,
                response_time_ms: Some(timing::FAN_STABILIZATION.as_millis() as u32),
                min_pwm: None,
                max_rpm: baseline_rpms.get(&fan_path).copied(),
            });
        }
        
        // Restore PWM to 100%
        let _ = fs::write(&pwm.pwm_path, "255");
        thread::sleep(timing::DETECTION_DELAY);
    }
    
    // Phase 5: Restore original states
    info!("Phase 5: Restoring original PWM states");
    for (path, value) in &original_states {
        let _ = fs::write(path, value.to_string());
    }
    
    info!(
        mappings_found = mappings.len(),
        bindings = store.bindings.len(),
        "Fingerprinted detection complete"
    );
    
    // If no mappings found via probing, fall back to heuristic
    if mappings.is_empty() {
        debug!("No mappings found via probing, falling back to heuristic method");
        let heuristic_mappings = autodetect_fan_pwm_mappings_heuristic()?;
        return Ok(FingerprintedDetectionResult {
            mappings: heuristic_mappings,
            binding_store: store,
            pwm_count: total_pwm_count,
            fan_count: total_fan_count,
            temp_count: total_temp_count,
        });
    }
    
    Ok(FingerprintedDetectionResult {
        mappings,
        binding_store: store,
        pwm_count: total_pwm_count,
        fan_count: total_fan_count,
        temp_count: total_temp_count,
    })
}

/// Find PWM fingerprint ID by chip path and PWM name
fn find_pwm_fingerprint_id(store: &BindingStore, chip_path: &Path, pwm_name: &str) -> Option<String> {
    for (id, pwm_fp) in &store.pwm_channels {
        if pwm_fp.channel.original_name == pwm_name {
            // Check if chip matches
            if let Some(chip) = store.chips.get(&pwm_fp.channel.chip_fingerprint_id) {
                if chip.original_hwmon_path == chip_path {
                    return Some(id.clone());
                }
            }
        }
    }
    None
}

/// Find fan fingerprint ID by chip path and fan name
fn find_fan_fingerprint_id(store: &BindingStore, chip_path: &Path, fan_name: &str) -> Option<String> {
    for (id, fan_fp) in &store.fan_channels {
        if fan_fp.original_name == fan_name {
            if let Some(chip) = store.chips.get(&fan_fp.chip_fingerprint_id) {
                if chip.original_hwmon_path == chip_path {
                    return Some(id.clone());
                }
            }
        }
    }
    None
}
