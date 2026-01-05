//! Daemon Drift Protection
//!
//! This module ensures the daemon NEVER breaks due to hwmon reindexing.
//! It integrates with the core fingerprinting system and validates all
//! paths before applying fan control.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use hf_core::fingerprinting::*;

/// Daemon-side drift protection state
pub struct DriftProtection {
    /// Loaded fingerprint store
    store: Arc<RwLock<FingerprintStore>>,
    
    /// Cached path mappings (fingerprint_id -> current_path)
    path_cache: Arc<RwLock<HashMap<String, PathBuf>>>,
    
    /// Last validation timestamp
    last_validation: Arc<RwLock<std::time::Instant>>,
    
    /// Validation interval
    validation_interval: std::time::Duration,
}

impl DriftProtection {
    /// Create new drift protection system
    pub fn new(validation_interval_secs: u64) -> Self {
        Self {
            store: Arc::new(RwLock::new(FingerprintStore::new())),
            path_cache: Arc::new(RwLock::new(HashMap::new())),
            last_validation: Arc::new(RwLock::new(std::time::Instant::now())),
            validation_interval: std::time::Duration::from_secs(validation_interval_secs),
        }
    }
    
    /// Initialize drift protection on daemon startup
    pub async fn initialize(&self) -> Result<StartupResult, String> {
        info!("Initializing daemon drift protection");
        
        // Initialize fingerprinting system
        let result = initialize_fingerprinting_system()?;
        
        // Store the loaded store
        *self.store.write().await = result.store.clone();
        
        // Build initial path cache
        self.rebuild_path_cache().await?;
        
        // Log startup status
        if result.ready_for_control {
            info!(
                "✓ Drift protection initialized: {} safe bindings",
                result.safe_binding_ids.len()
            );
        } else {
            warn!("⚠ No safe bindings available - fan control disabled");
        }
        
        // Print drift report if corrections were applied
        if result.drift_result.corrections_applied {
            let report = generate_drift_report(&result.drift_result);
            info!("Drift corrections applied:\n{}", report);
        }
        
        Ok(result)
    }
    
    /// Rebuild path cache from current store
    async fn rebuild_path_cache(&self) -> Result<(), String> {
        let store = self.store.read().await;
        let mut cache = self.path_cache.write().await;
        cache.clear();
        
        // Cache all PWM paths
        for (pwm_id, pwm_fp) in &store.pwm_channels {
            cache.insert(pwm_id.clone(), pwm_fp.channel.original_path.clone());
        }
        
        // Cache all channel paths (fans, temps, etc.)
        for (channel_id, channel_fp) in &store.channels {
            cache.insert(channel_id.clone(), channel_fp.original_path.clone());
        }
        
        debug!("Path cache rebuilt: {} entries", cache.len());
        Ok(())
    }
    
    /// Validate a PWM path before use
    pub async fn validate_pwm_path(&self, pwm_id: &str) -> Result<PathBuf, String> {
        // Check cache first
        let cache = self.path_cache.read().await;
        if let Some(path) = cache.get(pwm_id) {
            // Quick validation - does file exist?
            if path.exists() {
                return Ok(path.clone());
            }
        }
        drop(cache);
        
        // Cache miss or file doesn't exist - trigger revalidation
        warn!("PWM path cache miss or invalid for {}, revalidating", pwm_id);
        self.revalidate_binding(pwm_id).await
    }
    
    /// Validate a fan path before use
    pub async fn validate_fan_path(&self, fan_id: &str) -> Result<PathBuf, String> {
        let cache = self.path_cache.read().await;
        if let Some(path) = cache.get(fan_id) {
            if path.exists() {
                return Ok(path.clone());
            }
        }
        drop(cache);
        
        warn!("Fan path cache miss or invalid for {}, revalidating", fan_id);
        self.revalidate_sensor(fan_id).await
    }
    
    /// Validate a temperature path before use
    pub async fn validate_temp_path(&self, temp_id: &str) -> Result<PathBuf, String> {
        let cache = self.path_cache.read().await;
        if let Some(path) = cache.get(temp_id) {
            if path.exists() {
                return Ok(path.clone());
            }
        }
        drop(cache);
        
        warn!("Temp path cache miss or invalid for {}, revalidating", temp_id);
        self.revalidate_sensor(temp_id).await
    }
    
    /// Revalidate a specific binding and update cache
    async fn revalidate_binding(&self, pwm_id: &str) -> Result<PathBuf, String> {
        let mut store = self.store.write().await;
        
        // Get binding
        let binding = store.bindings.get(pwm_id)
            .ok_or_else(|| format!("Binding {} not found", pwm_id))?
            .clone();
        
        // Validate binding
        let result = validate_binding(&binding, &store);
        
        if result.safe_for_control {
            // Update store with validation results
            store.update_binding_validation(pwm_id, result.new_state, result.confidence)
                .map_err(|e| format!("Failed to update validation: {}", e))?;
            
            // Update cache with new path
            if let Some(ref path) = result.resolved_pwm_path {
                let mut cache = self.path_cache.write().await;
                cache.insert(pwm_id.to_string(), path.clone());
                
                info!("✓ Revalidated PWM {}: {:?}", pwm_id, path);
                return Ok(path.clone());
            }
        }
        
        Err(format!("Binding {} validation failed: confidence {:.0}%", 
                   pwm_id, result.confidence * 100.0))
    }
    
    /// Revalidate a specific sensor and update cache
    async fn revalidate_sensor(&self, sensor_id: &str) -> Result<PathBuf, String> {
        let store = self.store.read().await;
        
        // Try to find sensor in channels
        if let Some(channel_fp) = store.channels.get(sensor_id) {
            // Get chip
            let chip_fp = store.chips.get(&channel_fp.chip_id)
                .ok_or_else(|| format!("Chip {} not found", channel_fp.chip_id))?;
            
            // Find chip in current system
            let chip_match = find_chip_by_fingerprint(chip_fp)
                .map_err(|e| format!("Chip not found: {}", e))?;
            
            // Find channel in chip
            let channel_match = find_channel_by_fingerprint(channel_fp, &chip_match.hwmon_path)
                .map_err(|e| format!("Channel not found: {}", e))?;
            
            if let Some(path) = channel_match.sensor_path {
                // Update cache
                let mut cache = self.path_cache.write().await;
                cache.insert(sensor_id.to_string(), path.clone());
                
                info!("✓ Revalidated sensor {}: {:?}", sensor_id, path);
                return Ok(path);
            }
        }
        
        Err(format!("Sensor {} not found", sensor_id))
    }
    
    /// Periodic validation check (called from control loop)
    pub async fn periodic_validation(&self) -> Result<(), String> {
        let last = *self.last_validation.read().await;
        
        if last.elapsed() < self.validation_interval {
            return Ok(());
        }
        
        info!("Running periodic drift validation");
        
        let mut store = self.store.write().await;
        let report = validate_all_bindings(&mut store);
        
        if report.has_problems() {
            warn!(
                "⚠ Validation found issues: {} degraded, {} unsafe",
                report.degraded_count,
                report.unsafe_count
            );
            
            // Rebuild cache to pick up any path changes
            drop(store);
            self.rebuild_path_cache().await?;
        }
        
        *self.last_validation.write().await = std::time::Instant::now();
        Ok(())
    }
    
    /// Get all safe bindings for fan control
    pub async fn get_safe_bindings(&self) -> Vec<String> {
        let store = self.store.read().await;
        get_safe_bindings(&store)
    }
    
    /// Get binding information
    pub async fn get_binding_info(&self, pwm_id: &str) -> Option<BindingInfo> {
        let store = self.store.read().await;
        get_binding_info(&store, pwm_id)
    }
    
    /// Force immediate drift detection and correction
    pub async fn force_drift_correction(&self) -> Result<DriftDetectionResult, String> {
        info!("Forcing drift detection and correction");
        
        let mut store = self.store.write().await;
        let result = detect_and_correct_drift(&mut store);
        
        if result.corrections_applied {
            // Rebuild cache with corrected paths
            drop(store);
            self.rebuild_path_cache().await?;
            
            info!("Drift corrections applied and cache rebuilt");
        }
        
        Ok(result)
    }
}

/// Convert fingerprint binding to daemon control pair
pub async fn binding_to_control_pair(
    drift_protection: &DriftProtection,
    pwm_id: &str,
    curve_points: Vec<(f32, f32)>,
) -> Result<super::fan_control::ControlPair, String> {
    let info = drift_protection.get_binding_info(pwm_id).await
        .ok_or_else(|| format!("Binding {} not found", pwm_id))?;
    
    // Validate PWM path
    let pwm_path = drift_protection.validate_pwm_path(pwm_id).await?;
    
    // Get temperature source path (from binding or default)
    let temp_path = if let Some(binding) = drift_protection.store.read().await.bindings.get(pwm_id) {
        if let Some(ref temp_id) = binding.temp_channel_id {
            drift_protection.validate_temp_path(temp_id).await?
        } else {
            // No temp sensor configured - use chip's first temp sensor
            return Err("No temperature sensor configured for binding".to_string());
        }
    } else {
        return Err(format!("Binding {} not found", pwm_id));
    };
    
    Ok(super::fan_control::ControlPair {
        id: pwm_id.to_string(),
        name: info.user_label.unwrap_or_else(|| info.pwm_name.clone()),
        pwm_path: pwm_path.to_string_lossy().to_string(),
        temp_source_path: temp_path.to_string_lossy().to_string(),
        curve_points,
        active: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_drift_protection_creation() {
        let dp = DriftProtection::new(60);
        assert!(dp.path_cache.read().await.is_empty());
    }
}
