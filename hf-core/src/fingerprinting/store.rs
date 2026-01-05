//! Fingerprint Storage and Persistence
//!
//! This module manages the persistent storage of sensor fingerprints and bindings.
//! The store is saved to disk and loaded on startup to maintain sensor pairings
//! across reboots and system changes.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tracing::{debug, info, warn};

use super::anchors::*;

// ============================================================================
// Fingerprint Store
// ============================================================================

/// Persistent store for all fingerprints and validated bindings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FingerprintStore {
    /// Store format version for migration
    pub version: u32,
    
    /// All known chip fingerprints, keyed by fingerprint ID
    pub chips: HashMap<String, ChipFingerprint>,
    
    /// All known channel fingerprints, keyed by fingerprint ID
    pub channels: HashMap<String, ChannelFingerprint>,
    
    /// All known PWM channel fingerprints, keyed by fingerprint ID
    pub pwm_channels: HashMap<String, PwmChannelFingerprint>,
    
    /// Validated PWM-to-fan bindings
    pub bindings: HashMap<String, StoredBinding>,
    
    /// System DMI for hardware change detection (v3 anti-drift)
    /// This is CRITICAL for detecting motherboard replacement
    pub system_dmi: Option<DmiAnchor>,
    
    /// Store creation timestamp
    pub created_at: u64,
    
    /// Last validation timestamp
    pub last_validated_at: Option<u64>,
    
    /// Last modification timestamp
    pub last_modified_at: u64,
}

impl FingerprintStore {
    pub const CURRENT_VERSION: u32 = 1;
    pub const STORE_FILENAME: &'static str = "sensor_fingerprints.json";

    /// Create a new empty store
    pub fn new() -> Self {
        let now = current_timestamp_ms();
        Self {
            version: Self::CURRENT_VERSION,
            chips: HashMap::new(),
            channels: HashMap::new(),
            pwm_channels: HashMap::new(),
            bindings: HashMap::new(),
            system_dmi: None,
            created_at: now,
            last_validated_at: None,
            last_modified_at: now,
        }
    }

    /// Get the store file path
    pub fn get_store_path() -> Result<PathBuf, String> {
        let config_dir = crate::constants::paths::user_config_dir()
            .ok_or("Could not determine config directory")?;
        Ok(config_dir.join(Self::STORE_FILENAME))
    }

    /// Load store from disk
    pub fn load() -> Result<Self, String> {
        let path = Self::get_store_path()?;

        if !path.exists() {
            debug!("No fingerprint store found, creating new");
            return Ok(Self::new());
        }

        // SECURITY: Check file size before reading (prevent DoS)
        const MAX_STORE_SIZE: u64 = 10 * 1024 * 1024; // 10 MB
        let metadata = fs::metadata(&path)
            .map_err(|e| format!("Failed to read store metadata: {}", e))?;
        
        if metadata.len() > MAX_STORE_SIZE {
            return Err(format!("Store file too large: {} bytes (max {})", 
                             metadata.len(), MAX_STORE_SIZE));
        }

        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read fingerprint store: {}", e))?;

        let mut store: Self = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse fingerprint store: {}", e))?;

        // Check version and migrate if needed
        if store.version < Self::CURRENT_VERSION {
            warn!(
                old_version = store.version,
                new_version = Self::CURRENT_VERSION,
                "Migrating fingerprint store"
            );
            store = Self::migrate(store)?;
        }

        info!(
            chips = store.chips.len(),
            channels = store.channels.len(),
            pwm_channels = store.pwm_channels.len(),
            bindings = store.bindings.len(),
            "Loaded fingerprint store"
        );

        Ok(store)
    }

    /// Save store to disk
    pub fn save(&mut self) -> Result<(), String> {
        let path = Self::get_store_path()?;

        // Update modification timestamp
        self.last_modified_at = current_timestamp_ms();

        // Ensure directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create config directory: {}", e))?;
        }

        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize fingerprint store: {}", e))?;

        // SECURITY: Atomic write - write to temp file then rename
        use std::io::Write;
        let temp_path = path.with_extension("json.tmp");
        
        // Write to temp file
        let mut file = fs::File::create(&temp_path)
            .map_err(|e| format!("Failed to create temp file: {}", e))?;
        
        file.write_all(json.as_bytes())
            .map_err(|e| format!("Failed to write to temp file: {}", e))?;
        
        // Ensure data is flushed to disk
        file.sync_all()
            .map_err(|e| format!("Failed to sync temp file: {}", e))?;
        
        drop(file);
        
        // Atomic rename
        fs::rename(&temp_path, &path)
            .map_err(|e| format!("Failed to rename temp file: {}", e))?;

        debug!(path = ?path, "Saved fingerprint store");
        Ok(())
    }

    /// Migrate store from old version
    fn migrate(mut store: Self) -> Result<Self, String> {
        // Future migration logic goes here
        store.version = Self::CURRENT_VERSION;
        Ok(store)
    }

    /// Register a chip fingerprint
    pub fn register_chip(&mut self, chip: ChipFingerprint) -> String {
        let id = chip.id.clone();
        self.chips.insert(id.clone(), chip);
        self.last_modified_at = current_timestamp_ms();
        id
    }

    /// Register a channel fingerprint
    pub fn register_channel(&mut self, channel: ChannelFingerprint) -> String {
        let id = channel.id.clone();
        self.channels.insert(id.clone(), channel);
        self.last_modified_at = current_timestamp_ms();
        id
    }

    /// Register a PWM channel fingerprint
    pub fn register_pwm_channel(&mut self, pwm: PwmChannelFingerprint) -> String {
        let id = pwm.channel.id.clone();
        self.pwm_channels.insert(id.clone(), pwm);
        self.last_modified_at = current_timestamp_ms();
        id
    }

    /// Create a new binding between PWM and fan
    pub fn create_binding(
        &mut self,
        pwm_id: String,
        fan_id: Option<String>,
        temp_id: Option<String>,
        user_label: Option<String>,
    ) -> Result<String, String> {
        // Validate PWM exists
        if !self.pwm_channels.contains_key(&pwm_id) {
            return Err(format!("PWM channel {} not found in store", pwm_id));
        }

        // Validate fan exists if specified
        if let Some(ref fid) = fan_id {
            if !self.channels.contains_key(fid) {
                return Err(format!("Fan channel {} not found in store", fid));
            }
        }

        // Validate temp exists if specified
        if let Some(ref tid) = temp_id {
            if !self.channels.contains_key(tid) {
                return Err(format!("Temperature channel {} not found in store", tid));
            }
        }

        let now = current_timestamp_ms();
        let binding = StoredBinding {
            pwm_channel_id: pwm_id.clone(),
            fan_channel_id: fan_id,
            temp_channel_id: temp_id,
            user_label,
            created_at: now,
            last_validated_at: None,
            validation_count: 0,
            last_validation_state: ValidationState::Unvalidated,
            last_confidence: 0.0,
        };

        self.bindings.insert(pwm_id.clone(), binding);
        self.last_modified_at = now;

        info!(pwm_id = %pwm_id, "Created new binding");
        Ok(pwm_id)
    }

    /// Update binding validation state
    pub fn update_binding_validation(
        &mut self,
        pwm_id: &str,
        state: ValidationState,
        confidence: f32,
    ) -> Result<(), String> {
        let binding = self.bindings.get_mut(pwm_id)
            .ok_or_else(|| format!("Binding {} not found", pwm_id))?;

        binding.last_validation_state = state;
        binding.last_confidence = confidence;
        binding.last_validated_at = Some(current_timestamp_ms());
        binding.validation_count += 1;

        Ok(())
    }

    /// Remove a binding
    pub fn remove_binding(&mut self, pwm_id: &str) -> Result<(), String> {
        self.bindings.remove(pwm_id)
            .ok_or_else(|| format!("Binding {} not found", pwm_id))?;
        
        self.last_modified_at = current_timestamp_ms();
        info!(pwm_id = %pwm_id, "Removed binding");
        Ok(())
    }

    /// Get all bindings that need attention (unsafe or degraded)
    pub fn get_problematic_bindings(&self) -> Vec<&StoredBinding> {
        self.bindings
            .values()
            .filter(|b| {
                matches!(
                    b.last_validation_state,
                    ValidationState::Unsafe | ValidationState::Degraded
                )
            })
            .collect()
    }

    /// Get all safe bindings
    pub fn get_safe_bindings(&self) -> Vec<&StoredBinding> {
        self.bindings
            .values()
            .filter(|b| b.last_validation_state == ValidationState::Ok)
            .collect()
    }

    /// Clear all fingerprints and bindings (for re-detection)
    pub fn clear_all(&mut self) {
        self.chips.clear();
        self.channels.clear();
        self.pwm_channels.clear();
        self.bindings.clear();
        self.last_modified_at = current_timestamp_ms();
        warn!("Cleared all fingerprints and bindings");
    }
}

impl Default for FingerprintStore {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Stored Binding
// ============================================================================

/// A validated binding between PWM controller and fan sensor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredBinding {
    /// PWM channel fingerprint ID
    pub pwm_channel_id: String,
    
    /// Fan channel fingerprint ID (if paired)
    pub fan_channel_id: Option<String>,
    
    /// Temperature sensor fingerprint ID (control source)
    pub temp_channel_id: Option<String>,
    
    /// User-provided label for this binding
    pub user_label: Option<String>,
    
    /// Binding creation timestamp
    pub created_at: u64,
    
    /// Last validation timestamp
    pub last_validated_at: Option<u64>,
    
    /// Number of successful validations
    pub validation_count: u32,
    
    /// Last validation state
    pub last_validation_state: ValidationState,
    
    /// Last validation confidence score
    pub last_confidence: f32,
}

impl StoredBinding {
    /// Check if this binding is safe to use for fan control
    pub fn is_safe_for_control(&self) -> bool {
        self.last_validation_state == ValidationState::Ok
            && self.last_confidence >= super::MIN_CONFIDENCE_FOR_CONTROL
    }

    /// Check if this binding needs user attention
    pub fn needs_attention(&self) -> bool {
        matches!(
            self.last_validation_state,
            ValidationState::Degraded | ValidationState::NeedsRebind | ValidationState::Unsafe
        )
    }
}

// ============================================================================
// Validation State
// ============================================================================

/// Validation state of a binding
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValidationState {
    /// Never validated yet
    Unvalidated,
    /// All checks passed - safe to use
    Ok,
    /// Some checks failed but still usable with caution
    Degraded,
    /// Critical checks failed - needs rebinding
    NeedsRebind,
    /// Validation failed completely - unsafe to use
    Unsafe,
}

impl Default for ValidationState {
    fn default() -> Self {
        Self::Unvalidated
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_new() {
        let store = FingerprintStore::new();
        assert_eq!(store.version, FingerprintStore::CURRENT_VERSION);
        assert!(store.chips.is_empty());
        assert!(store.bindings.is_empty());
    }

    #[test]
    fn test_binding_safety() {
        let mut binding = StoredBinding {
            pwm_channel_id: "test".to_string(),
            fan_channel_id: None,
            temp_channel_id: None,
            user_label: None,
            created_at: 0,
            last_validated_at: None,
            validation_count: 0,
            last_validation_state: ValidationState::Ok,
            last_confidence: 0.95,
        };

        assert!(binding.is_safe_for_control());

        binding.last_confidence = 0.85;
        assert!(!binding.is_safe_for_control());

        binding.last_validation_state = ValidationState::Degraded;
        assert!(!binding.is_safe_for_control());
    }
}
