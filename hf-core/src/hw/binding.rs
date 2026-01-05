//! PWM-Fan Binding Manager
//!
//! Manages the creation, validation, and persistence of validated PWM-fan bindings.
//! Ensures sensors CANNOT be mispaired after user configuration by validating
//! fingerprints on every system boot.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};

use crate::constants::fingerprint as fp_const;
use crate::hw::fingerprint::{
    extract_chip_fingerprint, extract_channel_fingerprint, extract_pwm_fingerprint,
    find_matching_hwmon, generate_channel_id, generate_chip_id,
    validate_channel_fingerprint, ChannelFingerprint,
    ChannelType, ChipFingerprint, PwmChannelFingerprint, PwmProbeData,
    SafeFallbackPolicy, ValidatedPwmFanBinding, ValidationState,
};

// ============================================================================
// Binding Store
// ============================================================================

/// Persistent store for all fingerprints and bindings
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BindingStore {
    /// All known chip fingerprints, keyed by chip ID
    pub chips: HashMap<String, ChipFingerprint>,
    /// All known PWM channel fingerprints, keyed by channel ID
    pub pwm_channels: HashMap<String, PwmChannelFingerprint>,
    /// All known fan channel fingerprints, keyed by channel ID
    pub fan_channels: HashMap<String, ChannelFingerprint>,
    /// All known temperature channel fingerprints, keyed by channel ID
    pub temp_channels: HashMap<String, ChannelFingerprint>,
    /// Active PWM-fan bindings, keyed by PWM channel ID
    pub bindings: HashMap<String, ValidatedPwmFanBinding>,
    /// Store version for migration
    pub version: u32,
    /// Last full validation timestamp
    pub last_validated_at: Option<u64>,
}

impl BindingStore {
    pub const CURRENT_VERSION: u32 = 1;

    /// Create a new empty binding store
    pub fn new() -> Self {
        Self {
            chips: HashMap::new(),
            pwm_channels: HashMap::new(),
            fan_channels: HashMap::new(),
            temp_channels: HashMap::new(),
            bindings: HashMap::new(),
            version: Self::CURRENT_VERSION,
            last_validated_at: None,
        }
    }

    /// Get store file path
    pub fn get_store_path() -> Option<PathBuf> {
        crate::constants::paths::user_config_dir().map(|p| p.join("bindings.json"))
    }

    /// Load binding store from disk
    pub fn load() -> Result<Self, String> {
        let path = Self::get_store_path().ok_or("Could not determine config directory")?;

        if !path.exists() {
            debug!("No binding store found, creating new");
            return Ok(Self::new());
        }

        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read binding store: {}", e))?;

        let store: Self = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse binding store: {}", e))?;

        info!(
            chips = store.chips.len(),
            bindings = store.bindings.len(),
            "Loaded binding store"
        );

        Ok(store)
    }

    /// Save binding store to disk
    pub fn save(&self) -> Result<(), String> {
        let path = Self::get_store_path().ok_or("Could not determine config directory")?;

        // Ensure directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create config directory: {}", e))?;
        }

        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize binding store: {}", e))?;

        fs::write(&path, json).map_err(|e| format!("Failed to write binding store: {}", e))?;

        debug!(path = ?path, "Saved binding store");
        Ok(())
    }

    /// Register a chip fingerprint
    pub fn register_chip(&mut self, chip: ChipFingerprint) -> String {
        let id = generate_chip_id(&chip);
        self.chips.insert(id.clone(), chip);
        id
    }

    /// Register a PWM channel fingerprint
    pub fn register_pwm_channel(&mut self, pwm: PwmChannelFingerprint) -> String {
        let id = generate_channel_id(&pwm.channel);
        self.pwm_channels.insert(id.clone(), pwm);
        id
    }

    /// Register a fan channel fingerprint
    pub fn register_fan_channel(&mut self, fan: ChannelFingerprint) -> String {
        let id = generate_channel_id(&fan);
        self.fan_channels.insert(id.clone(), fan);
        id
    }

    /// Register a temperature channel fingerprint
    pub fn register_temp_channel(&mut self, temp: ChannelFingerprint) -> String {
        let id = generate_channel_id(&temp);
        self.temp_channels.insert(id.clone(), temp);
        id
    }

    /// Create a new PWM-fan binding
    pub fn create_binding(
        &mut self,
        pwm_id: &str,
        fan_id: Option<&str>,
        temp_id: Option<&str>,
        probe_data: Option<PwmProbeData>,
    ) -> Result<String, String> {
        let pwm_fp = self
            .pwm_channels
            .get(pwm_id)
            .ok_or_else(|| format!("PWM channel {} not found", pwm_id))?
            .clone();

        let fan_fp = fan_id.and_then(|id| self.fan_channels.get(id).cloned());
        let temp_fp = temp_id.and_then(|id| self.temp_channels.get(id).cloned());

        let now = current_timestamp_ms();

        // Calculate initial confidence
        let (confidence, reasons) = calculate_binding_confidence(&pwm_fp, &fan_fp, &probe_data);

        let state = confidence_to_state(confidence);

        let binding = ValidatedPwmFanBinding {
            pwm_fingerprint: PwmChannelFingerprint {
                probe_data,
                paired_fan_fingerprint_id: fan_id.map(|s| s.to_string()),
                ..pwm_fp
            },
            fan_fingerprint: fan_fp,
            temp_fingerprint: temp_fp,
            validation_state: state,
            confidence_score: confidence,
            user_override_ack: false,
            confidence_reasons: reasons,
            created_at: now,
            last_validated_at: Some(now),
            validation_count: 1,
        };

        self.bindings.insert(pwm_id.to_string(), binding);
        Ok(pwm_id.to_string())
    }

    /// Update user override acknowledgment for low-confidence binding
    pub fn acknowledge_override(&mut self, pwm_id: &str) -> Result<(), String> {
        let binding = self
            .bindings
            .get_mut(pwm_id)
            .ok_or_else(|| format!("Binding {} not found", pwm_id))?;

        binding.user_override_ack = true;
        info!(pwm_id = %pwm_id, "User acknowledged low-confidence binding");
        Ok(())
    }

    /// Get all bindings that need user attention
    pub fn get_bindings_needing_attention(&self) -> Vec<(&str, &ValidatedPwmFanBinding)> {
        self.bindings
            .iter()
            .filter(|(_, b)| {
                matches!(
                    b.validation_state,
                    ValidationState::Degraded | ValidationState::NeedsRebind
                ) && !b.user_override_ack
            })
            .map(|(k, v)| (k.as_str(), v))
            .collect()
    }

    /// Get all unsafe bindings
    pub fn get_unsafe_bindings(&self) -> Vec<(&str, &ValidatedPwmFanBinding)> {
        self.bindings
            .iter()
            .filter(|(_, b)| b.validation_state == ValidationState::Unsafe)
            .map(|(k, v)| (k.as_str(), v))
            .collect()
    }
}

// ============================================================================
// Validation Engine
// ============================================================================

/// Result of validating all bindings against current system state
#[derive(Debug, Clone)]
pub struct ValidationReport {
    /// Total bindings validated
    pub total_bindings: usize,
    /// Bindings that passed validation
    pub ok_count: usize,
    /// Bindings in degraded state
    pub degraded_count: usize,
    /// Bindings needing rebind
    pub needs_rebind_count: usize,
    /// Unsafe bindings (will not be applied)
    pub unsafe_count: usize,
    /// Individual binding results
    pub results: Vec<BindingValidationResult>,
    /// Timestamp of validation
    pub validated_at: u64,
}

/// Result of validating a single binding
#[derive(Debug, Clone)]
pub struct BindingValidationResult {
    /// PWM channel ID
    pub pwm_id: String,
    /// Previous validation state
    pub previous_state: ValidationState,
    /// New validation state
    pub new_state: ValidationState,
    /// New confidence score
    pub confidence: f32,
    /// Reasons for confidence level
    pub reasons: Vec<String>,
    /// Current resolved PWM path (if found)
    pub resolved_pwm_path: Option<PathBuf>,
    /// Current resolved fan path (if found)
    pub resolved_fan_path: Option<PathBuf>,
}

/// Validate all bindings in a store against current system state
pub fn validate_all_bindings(store: &mut BindingStore) -> ValidationReport {
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

    for (pwm_id, binding) in store.bindings.iter_mut() {
        let previous_state = binding.validation_state;
        let result = validate_single_binding(pwm_id, binding, &store.chips);

        // Update binding state
        binding.validation_state = result.new_state;
        binding.confidence_score = result.confidence;
        binding.confidence_reasons = result.reasons.clone();
        binding.last_validated_at = Some(now);
        binding.validation_count += 1;

        // Count by state
        match result.new_state {
            ValidationState::Ok => ok_count += 1,
            ValidationState::Degraded => degraded_count += 1,
            ValidationState::NeedsRebind => needs_rebind_count += 1,
            ValidationState::Unsafe => unsafe_count += 1,
        }

        // Log state changes
        if previous_state != result.new_state {
            warn!(
                pwm_id = %pwm_id,
                previous = ?previous_state,
                new = ?result.new_state,
                confidence = format!("{:.2}", result.confidence),
                "Binding state changed"
            );
        }

        results.push(result);
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
        total_bindings: results.len(),
        ok_count,
        degraded_count,
        needs_rebind_count,
        unsafe_count,
        results,
        validated_at: now,
    }
}

/// Validate a single binding
fn validate_single_binding(
    pwm_id: &str,
    binding: &ValidatedPwmFanBinding,
    chips: &HashMap<String, ChipFingerprint>,
) -> BindingValidationResult {
    let mut reasons = Vec::new();
    let mut total_score = 0.0f32;
    let mut max_score = 0.0f32;
    let mut resolved_pwm_path = None;
    let mut resolved_fan_path = None;

    let chip_id = &binding.pwm_fingerprint.channel.chip_fingerprint_id;

    // Step 1: Find and validate the parent chip
    if let Some(chip_fp) = chips.get(chip_id) {
        max_score += 40.0;

        if let Some((hwmon_path, chip_conf)) = find_matching_hwmon(chip_fp) {
            total_score += 40.0 * chip_conf;
            reasons.push(format!(
                "Chip found at {:?} (confidence: {:.0}%)",
                hwmon_path,
                chip_conf * 100.0
            ));

            // Step 2: Validate PWM channel within the chip
            max_score += 30.0;
            let (pwm_state, pwm_conf, pwm_reasons) =
                validate_channel_fingerprint(&binding.pwm_fingerprint.channel, &hwmon_path);

            total_score += 30.0 * pwm_conf;
            reasons.extend(pwm_reasons);

            if pwm_state != ValidationState::Unsafe {
                // Resolve current PWM path
                let pwm_name = &binding.pwm_fingerprint.channel.original_name;
                let pwm_path = hwmon_path.join(pwm_name);
                if pwm_path.exists() {
                    resolved_pwm_path = Some(pwm_path);
                }
            }

            // Step 3: Validate fan channel if paired
            if let Some(fan_fp) = &binding.fan_fingerprint {
                max_score += 20.0;
                let (fan_state, fan_conf, fan_reasons) =
                    validate_channel_fingerprint(fan_fp, &hwmon_path);

                total_score += 20.0 * fan_conf;
                reasons.extend(fan_reasons);

                if fan_state != ValidationState::Unsafe {
                    let fan_name = &fan_fp.original_name;
                    let fan_path = hwmon_path.join(format!("{}_input", fan_name));
                    if fan_path.exists() {
                        resolved_fan_path = Some(fan_path);
                    }
                }
            }

            // Step 4: Check probe data consistency if available
            if let Some(probe) = &binding.pwm_fingerprint.probe_data {
                max_score += 10.0;
                if probe.write_capability {
                    total_score += 5.0;
                    reasons.push("PWM write capability confirmed".to_string());
                }
                if !probe.control_authority_override {
                    total_score += 5.0;
                } else {
                    reasons.push("Warning: BIOS/EC override detected".to_string());
                }
            }
        } else {
            reasons.push(format!("Chip {} not found in current system", chip_id));
        }
    } else {
        reasons.push(format!("Chip fingerprint {} missing from store", chip_id));
    }

    // Calculate final confidence
    let confidence = if max_score > 0.0 {
        total_score / max_score
    } else {
        0.0
    };

    let new_state = confidence_to_state(confidence);

    debug!(
        pwm_id = %pwm_id,
        confidence = format!("{:.2}", confidence),
        state = ?new_state,
        "Single binding validation complete"
    );

    BindingValidationResult {
        pwm_id: pwm_id.to_string(),
        previous_state: binding.validation_state,
        new_state,
        confidence,
        reasons,
        resolved_pwm_path,
        resolved_fan_path,
    }
}

// ============================================================================
// Discovery and Fingerprinting
// ============================================================================

/// Discover all sensors and create fingerprints for the current system
pub fn discover_and_fingerprint_system(store: &mut BindingStore) -> Result<(), String> {
    let hwmon_base = Path::new("/sys/class/hwmon");
    if !hwmon_base.exists() {
        return Err("Hwmon sysfs not found".to_string());
    }

    info!("Discovering and fingerprinting all hwmon devices");

    let entries = fs::read_dir(hwmon_base)
        .map_err(|e| format!("Failed to read hwmon directory: {}", e))?;

    for entry in entries.flatten() {
        let hwmon_path = entry.path();
        
        // Extract chip fingerprint
        if let Some(chip_fp) = extract_chip_fingerprint(&hwmon_path) {
            let chip_id = store.register_chip(chip_fp.clone());
            debug!(chip = %chip_fp.driver_name, id = %chip_id, "Registered chip");

            // Scan for sensors in this chip
            if let Ok(files) = fs::read_dir(&hwmon_path) {
                for file in files.flatten() {
                    let name = file.file_name();
                    let name_str = name.to_string_lossy();

                    // Temperature sensors
                    if name_str.starts_with("temp") && name_str.ends_with("_input") {
                        let base = name_str.trim_end_matches("_input");
                        let temp_fp = extract_channel_fingerprint(
                            &chip_fp,
                            ChannelType::Temperature,
                            base,
                            &file.path(),
                        );
                        store.register_temp_channel(temp_fp);
                    }

                    // Fan sensors
                    if name_str.starts_with("fan") && name_str.ends_with("_input") {
                        let base = name_str.trim_end_matches("_input");
                        let fan_fp = extract_channel_fingerprint(
                            &chip_fp,
                            ChannelType::Fan,
                            base,
                            &file.path(),
                        );
                        store.register_fan_channel(fan_fp);
                    }

                    // PWM controllers
                    if name_str.starts_with("pwm") && !name_str.contains('_') {
                        let enable_path = hwmon_path.join(format!("{}_enable", name_str));
                        let pwm_fp = extract_pwm_fingerprint(
                            &chip_fp,
                            &name_str,
                            &file.path(),
                            &enable_path,
                        );
                        store.register_pwm_channel(pwm_fp);
                    }
                }
            }
        }
    }

    info!(
        chips = store.chips.len(),
        pwm = store.pwm_channels.len(),
        fans = store.fan_channels.len(),
        temps = store.temp_channels.len(),
        "System fingerprinting complete"
    );

    Ok(())
}

// ============================================================================
// Safe Fallback Application
// ============================================================================

/// Apply safe fallback policies to all unsafe or degraded bindings
pub fn apply_safe_fallbacks(store: &BindingStore) -> Vec<FallbackAction> {
    let mut actions = Vec::new();

    for (pwm_id, binding) in &store.bindings {
        // Only apply fallback to unsafe bindings or degraded without user ack
        let needs_fallback = binding.validation_state == ValidationState::Unsafe
            || (binding.validation_state == ValidationState::Degraded && !binding.user_override_ack)
            || (binding.validation_state == ValidationState::NeedsRebind
                && !binding.user_override_ack);

        if needs_fallback {
            let action = FallbackAction {
                pwm_id: pwm_id.clone(),
                policy: binding.pwm_fingerprint.safe_fallback_policy,
                reason: format!(
                    "Validation state: {:?}, confidence: {:.0}%",
                    binding.validation_state,
                    binding.confidence_score * 100.0
                ),
            };
            actions.push(action);
        }
    }

    actions
}

/// A fallback action to be taken for an unsafe binding
#[derive(Debug, Clone)]
pub struct FallbackAction {
    pub pwm_id: String,
    pub policy: SafeFallbackPolicy,
    pub reason: String,
}

/// Execute a fallback action (set PWM to safe value)
pub fn execute_fallback(action: &FallbackAction, pwm_path: &Path) -> Result<(), String> {
    let value: u8 = match action.policy {
        SafeFallbackPolicy::FullSpeed => 255,
        SafeFallbackPolicy::MediumSpeed => 128,
        SafeFallbackPolicy::CustomPercent(p) => (p as f32 * 2.55) as u8,
        SafeFallbackPolicy::RestoreAuto => {
            // Write "2" to enable file to restore automatic control
            let file_name = match pwm_path.file_name() {
                Some(name) => name.to_string_lossy(),
                None => {
                    return Err(format!("Invalid PWM path (no filename): {:?}", pwm_path));
                }
            };
            let enable_path = pwm_path.with_file_name(format!("{}_enable", file_name));
            if enable_path.exists() {
                fs::write(&enable_path, "2")
                    .map_err(|e| format!("Failed to restore auto control: {}", e))?;
            }
            return Ok(());
        }
        SafeFallbackPolicy::KeepCurrent => return Ok(()),
    };

    fs::write(pwm_path, value.to_string())
        .map_err(|e| format!("Failed to set PWM fallback value: {}", e))?;

    info!(
        path = ?pwm_path,
        value = value,
        policy = ?action.policy,
        "Applied safe fallback"
    );

    Ok(())
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Get current timestamp in milliseconds
fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Convert confidence score to validation state
fn confidence_to_state(confidence: f32) -> ValidationState {
    if confidence >= fp_const::CONFIDENCE_OK {
        ValidationState::Ok
    } else if confidence >= fp_const::CONFIDENCE_DEGRADED {
        ValidationState::Degraded
    } else if confidence >= fp_const::CONFIDENCE_NEEDS_REBIND {
        ValidationState::NeedsRebind
    } else {
        ValidationState::Unsafe
    }
}

/// Calculate confidence for a new binding
fn calculate_binding_confidence(
    pwm_fp: &PwmChannelFingerprint,
    fan_fp: &Option<ChannelFingerprint>,
    probe_data: &Option<PwmProbeData>,
) -> (f32, Vec<String>) {
    let mut score = 0.0f32;
    let mut max_score = 0.0f32;
    let mut reasons = Vec::new();

    // PWM channel exists and is writable
    max_score += 30.0;
    if pwm_fp.pwm_write_capability {
        score += 30.0;
        reasons.push("PWM is writable".to_string());
    } else {
        reasons.push("PWM write capability not confirmed".to_string());
    }

    // PWM has enable file for manual control
    max_score += 10.0;
    if pwm_fp.has_enable_file {
        score += 10.0;
        reasons.push("Manual control enable available".to_string());
    }

    // No BIOS override
    max_score += 10.0;
    if pwm_fp.control_authority.is_none() {
        score += 10.0;
    } else {
        reasons.push(format!(
            "Control authority warning: {:?}",
            pwm_fp.control_authority
        ));
    }

    // Fan sensor paired
    if fan_fp.is_some() {
        max_score += 20.0;
        score += 20.0;
        reasons.push("Fan sensor paired".to_string());
    }

    // RPM feedback available
    max_score += 15.0;
    if pwm_fp.has_rpm_feedback {
        score += 15.0;
        reasons.push("RPM feedback available for closed-loop control".to_string());
    }

    // Probe data quality
    if let Some(probe) = probe_data {
        max_score += 15.0;
        if !probe.response_map.is_empty() {
            score += 10.0;
            reasons.push("PWM-fan response map available".to_string());
        }
        if probe.rpm_delta_on_step.is_some() {
            score += 5.0;
            reasons.push("RPM response to PWM step confirmed".to_string());
        }
    }

    let confidence = if max_score > 0.0 {
        score / max_score
    } else {
        0.0
    };

    (confidence, reasons)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_confidence_to_state() {
        assert_eq!(confidence_to_state(0.95), ValidationState::Ok);
        assert_eq!(confidence_to_state(0.90), ValidationState::Ok);
        assert_eq!(confidence_to_state(0.85), ValidationState::Degraded);
        assert_eq!(confidence_to_state(0.70), ValidationState::Degraded);
        assert_eq!(confidence_to_state(0.50), ValidationState::NeedsRebind);
        assert_eq!(confidence_to_state(0.30), ValidationState::Unsafe);
        assert_eq!(confidence_to_state(0.0), ValidationState::Unsafe);
    }

    #[test]
    fn test_binding_store_new() {
        let store = BindingStore::new();
        assert!(store.chips.is_empty());
        assert!(store.bindings.is_empty());
        assert_eq!(store.version, BindingStore::CURRENT_VERSION);
    }
}
