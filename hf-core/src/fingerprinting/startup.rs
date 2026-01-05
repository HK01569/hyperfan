//! Startup Routine for Fool-Proof Fingerprinting
//!
//! This module provides the main startup routine that:
//! 1. Loads stored fingerprints
//! 2. Detects and corrects any hwmon drift
//! 3. Validates all bindings
//! 4. Ensures Hyperfan NEVER breaks due to hwmon changes

use tracing::{info, warn};

use super::drift_correction::*;
use super::store::*;
use super::validator::*;
use super::hardware_change_detection::*;

// ============================================================================
// Startup Routine
// ============================================================================

/// Complete startup result
#[derive(Debug)]
pub struct StartupResult {
    /// Loaded fingerprint store
    pub store: FingerprintStore,
    
    /// Drift detection result
    pub drift_result: DriftDetectionResult,
    
    /// Validation report
    pub validation_report: ValidationReport,
    
    /// Hardware change detection report (v3 anti-drift)
    pub hardware_change_report: HardwareChangeReport,
    
    /// IDs of bindings that are safe for control
    pub safe_binding_ids: Vec<String>,
    
    /// Whether system is ready for fan control
    pub ready_for_control: bool,
    
    /// Status message for user
    pub status_message: String,
    
    /// Bindings that need attention
    pub problematic_binding_ids: Vec<String>,
}

/// Initialize fingerprinting system on startup
///
/// This is the MAIN ENTRY POINT for the fool-proof fingerprinting system.
/// Call this on application startup to:
/// - Load stored fingerprints
/// - Detect and correct hwmon drift
/// - Validate all bindings
/// - Get list of safe bindings for fan control
pub fn initialize_fingerprinting_system() -> Result<StartupResult, String> {
    info!("Initializing fool-proof fingerprinting system");
    
    // Step 1: Load stored fingerprints
    let mut store = FingerprintStore::load()
        .map_err(|e| format!("Failed to load fingerprint store: {}", e))?;
    
    info!(
        chips = store.chips.len(),
        channels = store.channels.len(),
        pwm_channels = store.pwm_channels.len(),
        bindings = store.bindings.len(),
        "Loaded fingerprint store"
    );
    
    // Step 2: Detect and correct drift
    let drift_result = detect_and_correct_drift(&mut store);
    
    if drift_result.corrections_applied {
        info!("Drift corrections were applied");
        
        // Print drift report
        let drift_report = generate_drift_report(&drift_result);
        info!("\n{}", drift_report);
    }
    
    // Step 3: Check for hardware changes (v3 anti-drift)
    let hardware_change_report = store.check_hardware_changes();
    
    if !hardware_change_report.allow_control {
        warn!("Hardware changes detected - fan control disabled");
        warn!("{}", hardware_change_report.user_message);
    }
    
    // Step 4: Validate all bindings
    let validation_report = validate_all_bindings(&mut store);
    
    // Step 5: Determine which bindings are safe
    // If hardware changes detected, override with hardware change report
    let (safe_binding_ids, problematic_binding_ids) = if hardware_change_report.allow_control {
        let safe: Vec<String> = store.bindings
            .iter()
            .filter(|(_, b)| b.is_safe_for_control())
            .map(|(id, _)| id.clone())
            .collect();
        
        let problematic: Vec<String> = store.bindings
            .iter()
            .filter(|(_, b)| b.needs_attention())
            .map(|(id, _)| id.clone())
            .collect();
        
        (safe, problematic)
    } else {
        // Hardware changes detected - no bindings are safe
        (hardware_change_report.safe_bindings.clone(), 
         hardware_change_report.invalid_bindings.clone())
    };
    
    // Step 6: Determine overall readiness
    let ready_for_control = !safe_binding_ids.is_empty() && hardware_change_report.allow_control;
    
    // Step 6: Generate status message
    let status_message = generate_status_message(
        &drift_result,
        &validation_report,
        safe_binding_ids.len(),
        problematic_binding_ids.len(),
    );
    
    info!("{}", status_message);
    
    // Step 7: Save store if any changes were made
    if drift_result.corrections_applied {
        store.save()
            .map_err(|e| format!("Failed to save store: {}", e))?;
    }
    
    Ok(StartupResult {
        store,
        drift_result,
        validation_report,
        hardware_change_report,
        ready_for_control,
        safe_binding_ids,
        status_message,
        problematic_binding_ids: Vec::new(),
    })
}

/// Generate human-readable status message
fn generate_status_message(
    drift_result: &DriftDetectionResult,
    validation_report: &ValidationReport,
    safe_count: usize,
    problematic_count: usize,
) -> String {
    let mut msg = String::new();
    
    msg.push_str("=== Fingerprinting System Status ===\n\n");
    
    // Drift status
    if drift_result.total_bindings > 0 {
        msg.push_str(&format!("Drift Detection:\n"));
        msg.push_str(&format!("  ✓ No drift: {}\n", drift_result.no_drift_count));
        
        if drift_result.correctable_drift_count > 0 {
            msg.push_str(&format!("  ✓ Auto-corrected: {}\n", drift_result.correctable_drift_count));
        }
        
        if drift_result.uncorrectable_drift_count > 0 {
            msg.push_str(&format!("  ✗ Uncorrectable: {}\n", drift_result.uncorrectable_drift_count));
        }
        msg.push_str("\n");
    }
    
    // Validation status
    msg.push_str(&format!("Binding Validation:\n"));
    msg.push_str(&format!("  ✓ OK: {}\n", validation_report.ok_count));
    
    if validation_report.degraded_count > 0 {
        msg.push_str(&format!("  ⚠ Degraded: {}\n", validation_report.degraded_count));
    }
    
    if validation_report.needs_rebind_count > 0 {
        msg.push_str(&format!("  ⚠ Needs rebind: {}\n", validation_report.needs_rebind_count));
    }
    
    if validation_report.unsafe_count > 0 {
        msg.push_str(&format!("  ✗ Unsafe: {}\n", validation_report.unsafe_count));
    }
    msg.push_str("\n");
    
    // Overall status
    if safe_count > 0 {
        msg.push_str(&format!("✓ System ready: {} bindings available for fan control\n", safe_count));
    } else {
        msg.push_str("✗ System not ready: No safe bindings available\n");
    }
    
    if problematic_count > 0 {
        msg.push_str(&format!("⚠ {} bindings need attention\n", problematic_count));
    }
    
    msg
}

/// Quick check if fingerprinting system is initialized
pub fn is_fingerprinting_initialized() -> bool {
    FingerprintStore::get_store_path()
        .map(|p| p.exists())
        .unwrap_or(false)
}

/// Get list of safe bindings for fan control
///
/// This is a convenience function that can be called after initialization
/// to get the list of bindings that are safe to use for fan control.
pub fn get_safe_bindings(store: &FingerprintStore) -> Vec<String> {
    store.bindings
        .iter()
        .filter(|(_, b)| b.is_safe_for_control())
        .map(|(id, _)| id.clone())
        .collect()
}

/// Get detailed information about a specific binding
pub fn get_binding_info(store: &FingerprintStore, pwm_id: &str) -> Option<BindingInfo> {
    let binding = store.bindings.get(pwm_id)?;
    let pwm_fp = store.pwm_channels.get(&binding.pwm_channel_id)?;
    let chip_fp = store.chips.get(&pwm_fp.channel.chip_id)?;
    
    let fan_info = binding.fan_channel_id.as_ref()
        .and_then(|fan_id| store.channels.get(fan_id))
        .map(|fan_fp| FanInfo {
            name: fan_fp.original_name.clone(),
            path: fan_fp.original_path.clone(),
            label: fan_fp.firmware.sensor_label.as_ref().map(|l| l.raw_label.clone()),
        });
    
    Some(BindingInfo {
        pwm_id: pwm_id.to_string(),
        pwm_name: pwm_fp.channel.original_name.clone(),
        pwm_path: pwm_fp.channel.original_path.clone(),
        chip_name: chip_fp.driver.driver_name.clone(),
        chip_path: chip_fp.original_hwmon_path.clone(),
        fan_info,
        validation_state: binding.last_validation_state,
        confidence: binding.last_confidence,
        safe_for_control: binding.is_safe_for_control(),
        user_label: binding.user_label.clone(),
    })
}

/// Detailed binding information
#[derive(Debug, Clone)]
pub struct BindingInfo {
    pub pwm_id: String,
    pub pwm_name: String,
    pub pwm_path: std::path::PathBuf,
    pub chip_name: String,
    pub chip_path: std::path::PathBuf,
    pub fan_info: Option<FanInfo>,
    pub validation_state: ValidationState,
    pub confidence: f32,
    pub safe_for_control: bool,
    pub user_label: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FanInfo {
    pub name: String,
    pub path: std::path::PathBuf,
    pub label: Option<String>,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_message_generation() {
        let drift_result = DriftDetectionResult {
            total_bindings: 3,
            no_drift_count: 2,
            correctable_drift_count: 1,
            uncorrectable_drift_count: 0,
            drift_details: vec![],
            corrections_applied: true,
        };
        
        let validation_report = ValidationReport {
            total: 3,
            ok_count: 3,
            degraded_count: 0,
            needs_rebind_count: 0,
            unsafe_count: 0,
            results: vec![],
            validated_at: 0,
        };
        
        let msg = generate_status_message(&drift_result, &validation_report, 3, 0);
        
        assert!(msg.contains("No drift: 2"));
        assert!(msg.contains("Auto-corrected: 1"));
        assert!(msg.contains("OK: 3"));
        assert!(msg.contains("System ready"));
    }
}
