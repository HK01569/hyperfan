//! Automatic Drift Detection and Correction
//!
//! This module detects when hwmon indices have changed and automatically corrects
//! bindings to point to the correct sensors. This ensures Hyperfan NEVER breaks
//! due to hwmon reindexing.

use std::path::{Path, PathBuf};
use tracing::{debug, info, error};

use super::anchors::*;
use super::matcher::*;
use super::store::*;

// ============================================================================
// Drift Detection
// ============================================================================

/// Result of drift detection
#[derive(Debug, Clone)]
pub struct DriftDetectionResult {
    /// Total bindings checked
    pub total_bindings: usize,
    
    /// Bindings with no drift detected
    pub no_drift_count: usize,
    
    /// Bindings with correctable drift
    pub correctable_drift_count: usize,
    
    /// Bindings with uncorrectable drift (hardware removed)
    pub uncorrectable_drift_count: usize,
    
    /// Detailed drift information per binding
    pub drift_details: Vec<BindingDriftInfo>,
    
    /// Whether any corrections were applied
    pub corrections_applied: bool,
}

/// Drift information for a single binding
#[derive(Debug, Clone)]
pub struct BindingDriftInfo {
    /// PWM channel ID
    pub pwm_id: String,
    
    /// Drift status
    pub status: DriftStatus,
    
    /// Old hwmon path (if changed)
    pub old_hwmon_path: Option<PathBuf>,
    
    /// New hwmon path (if found)
    pub new_hwmon_path: Option<PathBuf>,
    
    /// Old sensor paths
    pub old_pwm_path: Option<PathBuf>,
    pub old_fan_path: Option<PathBuf>,
    
    /// New sensor paths (after correction)
    pub new_pwm_path: Option<PathBuf>,
    pub new_fan_path: Option<PathBuf>,
    
    /// Confidence in the correction
    pub correction_confidence: f32,
    
    /// Reasons for drift detection
    pub reasons: Vec<String>,
}

/// Drift status for a binding
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriftStatus {
    /// No drift detected - sensors at expected locations
    NoDrift,
    
    /// Drift detected and automatically corrected
    CorrectedDrift,
    
    /// Drift detected but correction has low confidence
    LowConfidenceCorrection,
    
    /// Hardware not found - may have been removed
    HardwareNotFound,
    
    /// Multiple candidates found - ambiguous
    AmbiguousMatch,
}

// ============================================================================
// Drift Detection and Correction
// ============================================================================

/// Detect and correct drift in all bindings
///
/// This is the main entry point called on application startup.
/// It checks all stored bindings and automatically corrects any drift.
pub fn detect_and_correct_drift(store: &mut FingerprintStore) -> DriftDetectionResult {
    info!("Starting drift detection and correction");
    
    let mut result = DriftDetectionResult {
        total_bindings: store.bindings.len(),
        no_drift_count: 0,
        correctable_drift_count: 0,
        uncorrectable_drift_count: 0,
        drift_details: Vec::new(),
        corrections_applied: false,
    };
    
    // Check each binding
    let binding_ids: Vec<String> = store.bindings.keys().cloned().collect();
    
    for pwm_id in binding_ids {
        let drift_info = detect_and_correct_binding_drift(&pwm_id, store);
        
        match drift_info.status {
            DriftStatus::NoDrift => result.no_drift_count += 1,
            DriftStatus::CorrectedDrift | DriftStatus::LowConfidenceCorrection => {
                result.correctable_drift_count += 1;
                result.corrections_applied = true;
            }
            DriftStatus::HardwareNotFound | DriftStatus::AmbiguousMatch => {
                result.uncorrectable_drift_count += 1;
            }
        }
        
        result.drift_details.push(drift_info);
    }
    
    // Save store if corrections were applied
    if result.corrections_applied {
        if let Err(e) = store.save() {
            error!("Failed to save store after drift correction: {}", e);
        } else {
            info!("Saved corrected bindings to disk");
        }
    }
    
    info!(
        no_drift = result.no_drift_count,
        corrected = result.correctable_drift_count,
        uncorrectable = result.uncorrectable_drift_count,
        "Drift detection complete"
    );
    
    result
}

/// Detect and correct drift for a single binding
fn detect_and_correct_binding_drift(
    pwm_id: &str,
    store: &mut FingerprintStore,
) -> BindingDriftInfo {
    let mut reasons = Vec::new();
    
    // Get binding and fingerprints
    let binding = match store.bindings.get(pwm_id) {
        Some(b) => b,
        None => {
            return BindingDriftInfo {
                pwm_id: pwm_id.to_string(),
                status: DriftStatus::HardwareNotFound,
                old_hwmon_path: None,
                new_hwmon_path: None,
                old_pwm_path: None,
                old_fan_path: None,
                new_pwm_path: None,
                new_fan_path: None,
                correction_confidence: 0.0,
                reasons: vec!["Binding not found in store".to_string()],
            };
        }
    };
    
    let pwm_fp = match store.pwm_channels.get(&binding.pwm_channel_id) {
        Some(fp) => fp,
        None => {
            return BindingDriftInfo {
                pwm_id: pwm_id.to_string(),
                status: DriftStatus::HardwareNotFound,
                old_hwmon_path: None,
                new_hwmon_path: None,
                old_pwm_path: None,
                old_fan_path: None,
                new_pwm_path: None,
                new_fan_path: None,
                correction_confidence: 0.0,
                reasons: vec!["PWM fingerprint not found".to_string()],
            };
        }
    };
    
    let chip_fp = match store.chips.get(&pwm_fp.channel.chip_id) {
        Some(fp) => fp,
        None => {
            return BindingDriftInfo {
                pwm_id: pwm_id.to_string(),
                status: DriftStatus::HardwareNotFound,
                old_hwmon_path: None,
                new_hwmon_path: None,
                old_pwm_path: None,
                old_fan_path: None,
                new_pwm_path: None,
                new_fan_path: None,
                correction_confidence: 0.0,
                reasons: vec!["Chip fingerprint not found".to_string()],
            };
        }
    };
    
    let old_hwmon_path = chip_fp.original_hwmon_path.clone();
    let old_pwm_path = pwm_fp.channel.original_path.clone();
    
    // Step 1: Check if chip is still at the same location
    if old_hwmon_path.exists() {
        // Verify it's still the same chip
        if verify_chip_at_path(chip_fp, &old_hwmon_path) {
            // Check if PWM is still at same location
            if old_pwm_path.exists() {
                reasons.push("No drift detected - all sensors at expected locations".to_string());
                
                return BindingDriftInfo {
                    pwm_id: pwm_id.to_string(),
                    status: DriftStatus::NoDrift,
                    old_hwmon_path: Some(old_hwmon_path.clone()),
                    new_hwmon_path: Some(old_hwmon_path),
                    old_pwm_path: Some(old_pwm_path.clone()),
                    old_fan_path: None,
                    new_pwm_path: Some(old_pwm_path),
                    new_fan_path: None,
                    correction_confidence: 1.0,
                    reasons,
                };
            }
        }
    }
    
    reasons.push("Drift detected - searching for hardware".to_string());
    
    // Step 2: Find chip using comprehensive fingerprint matching
    let chip_match = match find_chip_by_fingerprint(chip_fp) {
        Ok(m) => {
            reasons.push(format!(
                "Found chip at {:?} (confidence: {:.0}%)",
                m.hwmon_path,
                m.confidence.overall * 100.0
            ));
            m
        }
        Err(MatchError::NoMatch) => {
            reasons.push("Chip not found in current system".to_string());
            return BindingDriftInfo {
                pwm_id: pwm_id.to_string(),
                status: DriftStatus::HardwareNotFound,
                old_hwmon_path: Some(old_hwmon_path),
                new_hwmon_path: None,
                old_pwm_path: Some(old_pwm_path),
                old_fan_path: None,
                new_pwm_path: None,
                new_fan_path: None,
                correction_confidence: 0.0,
                reasons,
            };
        }
        Err(e) => {
            reasons.push(format!("Error finding chip: {}", e));
            return BindingDriftInfo {
                pwm_id: pwm_id.to_string(),
                status: DriftStatus::HardwareNotFound,
                old_hwmon_path: Some(old_hwmon_path),
                new_hwmon_path: None,
                old_pwm_path: Some(old_pwm_path),
                old_fan_path: None,
                new_pwm_path: None,
                new_fan_path: None,
                correction_confidence: 0.0,
                reasons,
            };
        }
    };
    
    let new_hwmon_path = chip_match.hwmon_path.clone();
    
    // Step 3: Find PWM channel in the chip
    let pwm_match = match find_channel_by_fingerprint(&pwm_fp.channel, &new_hwmon_path) {
        Ok(m) => {
            reasons.push(format!(
                "Found PWM channel (confidence: {:.0}%)",
                m.confidence.overall * 100.0
            ));
            m
        }
        Err(_) => {
            reasons.push("PWM channel not found in chip".to_string());
            return BindingDriftInfo {
                pwm_id: pwm_id.to_string(),
                status: DriftStatus::HardwareNotFound,
                old_hwmon_path: Some(old_hwmon_path),
                new_hwmon_path: Some(new_hwmon_path),
                old_pwm_path: Some(old_pwm_path),
                old_fan_path: None,
                new_pwm_path: None,
                new_fan_path: None,
                correction_confidence: chip_match.confidence.overall,
                reasons,
            };
        }
    };
    
    let new_pwm_path = pwm_match.sensor_path.clone();
    
    // Step 4: Find fan channel if paired
    let (old_fan_path, new_fan_path) = if let Some(ref fan_id) = binding.fan_channel_id {
        if let Some(fan_fp) = store.pwm_channels.get(fan_id) {
            let old_fan = fan_fp.channel.original_path.clone();
            
            let new_fan = match find_channel_by_fingerprint(&fan_fp.channel, &new_hwmon_path) {
                Ok(m) => {
                    reasons.push(format!(
                        "Found fan channel (confidence: {:.0}%)",
                        m.confidence.overall * 100.0
                    ));
                    m.sensor_path
                }
                Err(_) => {
                    reasons.push("Fan channel not found".to_string());
                    None
                }
            };
            
            (Some(old_fan), new_fan)
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };
    
    // Step 5: Calculate overall correction confidence
    let correction_confidence = (chip_match.confidence.overall + pwm_match.confidence.overall) / 2.0;
    
    // Step 6: Clone data needed for correction before mutable borrow
    let chip_id = chip_fp.id.clone();
    let pwm_channel_id = pwm_fp.channel.id.clone();
    let new_hwmon_path_buf = new_hwmon_path.clone();
    
    // Step 7: Apply correction to store
    let status = if correction_confidence >= 0.90 {
        // High confidence - apply correction
        apply_drift_correction_by_id(store, &chip_id, &pwm_channel_id, &new_hwmon_path_buf, new_pwm_path.as_ref());
        reasons.push("Applied drift correction with high confidence".to_string());
        DriftStatus::CorrectedDrift
    } else if correction_confidence >= 0.70 {
        // Medium confidence - apply but warn
        apply_drift_correction_by_id(store, &chip_id, &pwm_channel_id, &new_hwmon_path_buf, new_pwm_path.as_ref());
        reasons.push("Applied drift correction with medium confidence - manual verification recommended".to_string());
        DriftStatus::LowConfidenceCorrection
    } else {
        // Low confidence - don't apply
        reasons.push("Correction confidence too low - manual intervention required".to_string());
        DriftStatus::AmbiguousMatch
    };
    
    BindingDriftInfo {
        pwm_id: pwm_id.to_string(),
        status,
        old_hwmon_path: Some(old_hwmon_path),
        new_hwmon_path: Some(new_hwmon_path),
        old_pwm_path: Some(old_pwm_path),
        old_fan_path,
        new_pwm_path,
        new_fan_path,
        correction_confidence,
        reasons,
    }
}

/// Verify a chip is still at the expected path
fn verify_chip_at_path(chip_fp: &ChipFingerprint, hwmon_path: &Path) -> bool {
    // Quick verification - check driver name
    let name_path = hwmon_path.join("name");
    if let Ok(name) = std::fs::read_to_string(&name_path) {
        if name.trim() == chip_fp.driver.driver_name {
            return true;
        }
    }
    false
}

/// Apply drift correction to the store using IDs
fn apply_drift_correction_by_id(
    store: &mut FingerprintStore,
    chip_id: &str,
    pwm_channel_id: &str,
    new_hwmon_path: &Path,
    new_pwm_path: Option<&PathBuf>,
) {
    // Update chip fingerprint
    if let Some(chip) = store.chips.get_mut(chip_id) {
        let old_path = chip.original_hwmon_path.clone();
        chip.original_hwmon_path = new_hwmon_path.to_path_buf();
        debug!(
            chip_id = %chip_id,
            old_path = ?old_path,
            new_path = ?new_hwmon_path,
            "Updated chip hwmon path"
        );
    }
    
    // Update PWM channel fingerprint
    if let Some(pwm) = store.pwm_channels.get_mut(pwm_channel_id) {
        if let Some(path) = new_pwm_path {
            let old_path = pwm.channel.original_path.clone();
            pwm.channel.original_path = path.clone();
            debug!(
                pwm_id = %pwm_channel_id,
                old_path = ?old_path,
                new_path = ?path,
                "Updated PWM channel path"
            );
        }
    }
}

// ============================================================================
// Drift Correction Report
// ============================================================================

/// Generate human-readable drift correction report
pub fn generate_drift_report(result: &DriftDetectionResult) -> String {
    let mut report = String::new();
    
    report.push_str("=== Drift Detection and Correction Report ===\n\n");
    
    report.push_str(&format!("Total bindings checked: {}\n", result.total_bindings));
    report.push_str(&format!("  ✓ No drift: {}\n", result.no_drift_count));
    report.push_str(&format!("  ✓ Corrected: {}\n", result.correctable_drift_count));
    report.push_str(&format!("  ✗ Uncorrectable: {}\n", result.uncorrectable_drift_count));
    report.push_str("\n");
    
    if result.corrections_applied {
        report.push_str("⚠️  Corrections were applied and saved to disk.\n\n");
    }
    
    // Detail each binding with drift
    for drift_info in &result.drift_details {
        if drift_info.status != DriftStatus::NoDrift {
            report.push_str(&format!("Binding: {}\n", drift_info.pwm_id));
            report.push_str(&format!("  Status: {:?}\n", drift_info.status));
            
            if let Some(ref old_path) = drift_info.old_hwmon_path {
                report.push_str(&format!("  Old hwmon: {:?}\n", old_path));
            }
            if let Some(ref new_path) = drift_info.new_hwmon_path {
                report.push_str(&format!("  New hwmon: {:?}\n", new_path));
            }
            
            if drift_info.correction_confidence > 0.0 {
                report.push_str(&format!("  Confidence: {:.0}%\n", drift_info.correction_confidence * 100.0));
            }
            
            for reason in &drift_info.reasons {
                report.push_str(&format!("  - {}\n", reason));
            }
            
            report.push_str("\n");
        }
    }
    
    report
}

// ============================================================================
// Helper Functions
// ============================================================================

#[allow(dead_code)]
fn current_timestamp_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drift_status() {
        assert_eq!(DriftStatus::NoDrift, DriftStatus::NoDrift);
        assert_ne!(DriftStatus::NoDrift, DriftStatus::CorrectedDrift);
    }

    #[test]
    fn test_drift_report_generation() {
        let result = DriftDetectionResult {
            total_bindings: 3,
            no_drift_count: 2,
            correctable_drift_count: 1,
            uncorrectable_drift_count: 0,
            drift_details: vec![],
            corrections_applied: true,
        };
        
        let report = generate_drift_report(&result);
        assert!(report.contains("Total bindings checked: 3"));
        assert!(report.contains("No drift: 2"));
        assert!(report.contains("Corrected: 1"));
    }
}
