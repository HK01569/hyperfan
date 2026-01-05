//! Binding Validation Engine
//!
//! This module validates stored bindings against the current system state.
//! It ensures that PWM-fan pairings remain correct and detects any drift
//! or hardware changes that could cause mispairing.

use std::path::PathBuf;
use tracing::{debug, info, warn};

use super::matcher::{find_chip_by_fingerprint, find_channel_by_fingerprint, MatchError};
use super::store::{FingerprintStore, StoredBinding, ValidationState};

// ============================================================================
// Validation Results
// ============================================================================

/// Result of validating a single binding
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// PWM channel ID
    pub pwm_id: String,
    
    /// Previous validation state
    pub previous_state: ValidationState,
    
    /// New validation state
    pub new_state: ValidationState,
    
    /// Overall confidence score (0.0 - 1.0)
    pub confidence: f32,
    
    /// Detailed validation reasons
    pub reasons: Vec<String>,
    
    /// Resolved PWM path in current system
    pub resolved_pwm_path: Option<PathBuf>,
    
    /// Resolved fan path in current system
    pub resolved_fan_path: Option<PathBuf>,
    
    /// Resolved temperature sensor path in current system
    pub resolved_temp_path: Option<PathBuf>,
    
    /// Whether this binding is safe for fan control
    pub safe_for_control: bool,
}

/// Report of validating all bindings
#[derive(Debug, Clone)]
pub struct ValidationReport {
    /// Total bindings validated
    pub total: usize,
    
    /// Bindings in OK state
    pub ok_count: usize,
    
    /// Bindings in degraded state
    pub degraded_count: usize,
    
    /// Bindings needing rebind
    pub needs_rebind_count: usize,
    
    /// Unsafe bindings
    pub unsafe_count: usize,
    
    /// Individual validation results
    pub results: Vec<ValidationResult>,
    
    /// Validation timestamp
    pub validated_at: u64,
}

impl ValidationReport {
    /// Check if any bindings need user attention
    pub fn has_problems(&self) -> bool {
        self.degraded_count > 0 || self.needs_rebind_count > 0 || self.unsafe_count > 0
    }

    /// Get all problematic results
    pub fn get_problematic_results(&self) -> Vec<&ValidationResult> {
        self.results
            .iter()
            .filter(|r| !r.safe_for_control)
            .collect()
    }
}

// ============================================================================
// Validation Functions
// ============================================================================

/// Validate a single binding against current system state
pub fn validate_binding(
    binding: &StoredBinding,
    store: &FingerprintStore,
) -> ValidationResult {
    let mut reasons = Vec::new();
    let mut resolved_pwm_path = None;
    let mut resolved_fan_path = None;
    let mut resolved_temp_path = None;
    let mut overall_confidence = 0.0;

    // Step 1: Get PWM fingerprint
    let pwm_fp = match store.pwm_channels.get(&binding.pwm_channel_id) {
        Some(fp) => fp,
        None => {
            reasons.push(format!("PWM fingerprint {} not found in store", binding.pwm_channel_id));
            return ValidationResult {
                pwm_id: binding.pwm_channel_id.clone(),
                previous_state: binding.last_validation_state,
                new_state: ValidationState::Unsafe,
                confidence: 0.0,
                reasons,
                resolved_pwm_path: None,
                resolved_fan_path: None,
                resolved_temp_path: None,
                safe_for_control: false,
            };
        }
    };

    // Step 2: Get chip fingerprint
    let chip_fp = match store.chips.get(&pwm_fp.channel.chip_id) {
        Some(fp) => fp,
        None => {
            reasons.push(format!("Chip fingerprint {} not found in store", pwm_fp.channel.chip_id));
            return ValidationResult {
                pwm_id: binding.pwm_channel_id.clone(),
                previous_state: binding.last_validation_state,
                new_state: ValidationState::Unsafe,
                confidence: 0.0,
                reasons,
                resolved_pwm_path: None,
                resolved_fan_path: None,
                resolved_temp_path: None,
                safe_for_control: false,
            };
        }
    };

    // Step 3: Find chip in current system
    let chip_match = match find_chip_by_fingerprint(chip_fp) {
        Ok(m) => {
            reasons.push(format!(
                "Chip matched at {:?} (confidence: {:.0}%)",
                m.hwmon_path,
                m.confidence.overall * 100.0
            ));
            reasons.extend(m.reasons.iter().map(|r| r.message.clone()));
            overall_confidence = m.confidence.overall;
            m
        }
        Err(MatchError::NoMatch) => {
            reasons.push("Chip not found in current system".to_string());
            return ValidationResult {
                pwm_id: binding.pwm_channel_id.clone(),
                previous_state: binding.last_validation_state,
                new_state: ValidationState::Unsafe,
                confidence: 0.0,
                reasons,
                resolved_pwm_path: None,
                resolved_fan_path: None,
                resolved_temp_path: None,
                safe_for_control: false,
            };
        }
        Err(e) => {
            reasons.push(format!("Error finding chip: {}", e));
            return ValidationResult {
                pwm_id: binding.pwm_channel_id.clone(),
                previous_state: binding.last_validation_state,
                new_state: ValidationState::Unsafe,
                confidence: 0.0,
                reasons,
                resolved_pwm_path: None,
                resolved_fan_path: None,
                resolved_temp_path: None,
                safe_for_control: false,
            };
        }
    };

    let chip_path = &chip_match.hwmon_path;

    // Step 4: Find PWM channel in chip
    let pwm_match = match find_channel_by_fingerprint(&pwm_fp.channel, chip_path) {
        Ok(m) => {
            reasons.push(format!(
                "PWM channel matched (confidence: {:.0}%)",
                m.confidence.overall * 100.0
            ));
            reasons.extend(m.reasons.iter().map(|r| r.message.clone()));
            
            // Combine confidences (weighted average)
            overall_confidence = (overall_confidence * 0.5) + (m.confidence.overall * 0.5);
            
            resolved_pwm_path = m.sensor_path.clone();
            m
        }
        Err(MatchError::NoMatch) => {
            reasons.push("PWM channel not found in chip".to_string());
            overall_confidence *= 0.5; // Reduce confidence
            
            return ValidationResult {
                pwm_id: binding.pwm_channel_id.clone(),
                previous_state: binding.last_validation_state,
                new_state: ValidationState::NeedsRebind,
                confidence: overall_confidence,
                reasons,
                resolved_pwm_path: None,
                resolved_fan_path: None,
                resolved_temp_path: None,
                safe_for_control: false,
            };
        }
        Err(e) => {
            reasons.push(format!("Error finding PWM channel: {}", e));
            return ValidationResult {
                pwm_id: binding.pwm_channel_id.clone(),
                previous_state: binding.last_validation_state,
                new_state: ValidationState::Unsafe,
                confidence: 0.0,
                reasons,
                resolved_pwm_path: None,
                resolved_fan_path: None,
                resolved_temp_path: None,
                safe_for_control: false,
            };
        }
    };

    // Step 5: Find fan channel if paired
    if let Some(fan_id) = &binding.fan_channel_id {
        if let Some(fan_fp) = store.channels.get(fan_id) {
            match find_channel_by_fingerprint(fan_fp, chip_path) {
                Ok(m) => {
                    reasons.push(format!(
                        "Fan channel matched (confidence: {:.0}%)",
                        m.confidence.overall * 100.0
                    ));
                    
                    // Combine confidences
                    overall_confidence = (overall_confidence * 0.7) + (m.confidence.overall * 0.3);
                    
                    resolved_fan_path = m.sensor_path.clone();
                }
                Err(MatchError::NoMatch) => {
                    reasons.push("Fan channel not found - pairing may be broken".to_string());
                    overall_confidence *= 0.8; // Reduce confidence but not critical
                }
                Err(e) => {
                    reasons.push(format!("Error finding fan channel: {}", e));
                    overall_confidence *= 0.8;
                }
            }
        }
    }

    // Step 6: Find temperature sensor if configured
    if let Some(temp_id) = &binding.temp_channel_id {
        if let Some(temp_fp) = store.channels.get(temp_id) {
            // Temperature sensor might be on a different chip
            let temp_chip_fp = store.chips.get(&temp_fp.chip_id);
            
            if let Some(temp_chip) = temp_chip_fp {
                if let Ok(temp_chip_match) = find_chip_by_fingerprint(temp_chip) {
                    match find_channel_by_fingerprint(temp_fp, &temp_chip_match.hwmon_path) {
                        Ok(m) => {
                            reasons.push(format!(
                                "Temperature sensor matched (confidence: {:.0}%)",
                                m.confidence.overall * 100.0
                            ));
                            resolved_temp_path = m.sensor_path.clone();
                        }
                        Err(_) => {
                            reasons.push("Temperature sensor not found".to_string());
                        }
                    }
                }
            }
        }
    }

    // Step 7: Validate PWM capabilities
    if let Some(pwm_path) = &resolved_pwm_path {
        if !pwm_fp.pwm_capabilities.is_writable {
            reasons.push("Warning: PWM may not be writable".to_string());
            overall_confidence *= 0.9;
        }

        // Check if PWM enable file exists and is in manual mode
        if pwm_fp.pwm_capabilities.has_enable {
            let Some(file_name) = pwm_path.file_name() else {
                reasons.push("Invalid PWM path: no filename".to_string());
                overall_confidence *= 0.5;
                return ValidationResult {
                    pwm_id: binding.pwm_channel_id.clone(),
                    previous_state: binding.last_validation_state,
                    new_state: ValidationState::Unsafe,
                    confidence: overall_confidence,
                    reasons,
                    resolved_pwm_path: None,
                    resolved_fan_path: None,
                    resolved_temp_path: None,
                    safe_for_control: false,
                };
            };
            let enable_path = pwm_path.with_file_name(format!("{}_enable", 
                file_name.to_string_lossy()));
            
            if let Ok(enable_content) = std::fs::read_to_string(&enable_path) {
                match enable_content.trim() {
                    "1" => {
                        reasons.push("PWM in manual control mode".to_string());
                    }
                    "0" => {
                        reasons.push("Warning: PWM is disabled".to_string());
                        overall_confidence *= 0.8;
                    }
                    _ => {
                        reasons.push("Warning: PWM in automatic mode".to_string());
                        overall_confidence *= 0.9;
                    }
                }
            }
        }
    }

    // Step 8: Determine validation state
    let new_state = if overall_confidence >= 0.95 {
        ValidationState::Ok
    } else if overall_confidence >= 0.85 {
        ValidationState::Degraded
    } else if overall_confidence >= 0.50 {
        ValidationState::NeedsRebind
    } else {
        ValidationState::Unsafe
    };

    let safe_for_control = new_state == ValidationState::Ok 
        && overall_confidence >= super::MIN_CONFIDENCE_FOR_CONTROL;

    debug!(
        pwm_id = %binding.pwm_channel_id,
        state = ?new_state,
        confidence = format!("{:.2}", overall_confidence),
        safe = safe_for_control,
        "Binding validation complete"
    );

    ValidationResult {
        pwm_id: binding.pwm_channel_id.clone(),
        previous_state: binding.last_validation_state,
        new_state,
        confidence: overall_confidence,
        reasons,
        resolved_pwm_path,
        resolved_fan_path,
        resolved_temp_path,
        safe_for_control,
    }
}

/// Validate all bindings in the store
pub fn validate_all_bindings(store: &mut FingerprintStore) -> ValidationReport {
    let now = current_timestamp_ms();
    let mut results = Vec::new();
    let mut ok_count = 0;
    let mut degraded_count = 0;
    let mut needs_rebind_count = 0;
    let mut unsafe_count = 0;

    info!(
        bindings = store.bindings.len(),
        "Starting full binding validation"
    );

    let binding_ids: Vec<String> = store.bindings.keys().cloned().collect();

    for pwm_id in binding_ids {
        if let Some(binding) = store.bindings.get(&pwm_id) {
            let result = validate_binding(binding, store);

            // Update store with validation results
            if let Err(e) = store.update_binding_validation(
                &pwm_id,
                result.new_state,
                result.confidence,
            ) {
                warn!(pwm_id = %pwm_id, error = %e, "Failed to update binding validation");
            }

            // Count by state
            match result.new_state {
                ValidationState::Ok => ok_count += 1,
                ValidationState::Degraded => degraded_count += 1,
                ValidationState::NeedsRebind => needs_rebind_count += 1,
                ValidationState::Unsafe => unsafe_count += 1,
                ValidationState::Unvalidated => {}
            }

            // Log state changes
            if result.previous_state != result.new_state {
                warn!(
                    pwm_id = %pwm_id,
                    previous = ?result.previous_state,
                    new = ?result.new_state,
                    confidence = format!("{:.0}%", result.confidence * 100.0),
                    "Binding state changed"
                );
            }

            results.push(result);
        }
    }

    store.last_validated_at = Some(now);

    info!(
        ok = ok_count,
        degraded = degraded_count,
        needs_rebind = needs_rebind_count,
        unsafe_bindings = unsafe_count,
        "Binding validation complete"
    );

    ValidationReport {
        total: results.len(),
        ok_count,
        degraded_count,
        needs_rebind_count,
        unsafe_count,
        results,
        validated_at: now,
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

fn current_timestamp_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
