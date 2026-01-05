//! JSON persistence for fan curves
//!
//! Automatically saves and loads fan curve configurations.

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tracing::{debug, info, warn};

use crate::constants::paths;

/// Check if a string is a valid UUID format (xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx)
fn is_valid_uuid(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 5 {
        return false;
    }
    let expected_lens = [8, 4, 4, 4, 12];
    for (part, &expected_len) in parts.iter().zip(expected_lens.iter()) {
        if part.len() != expected_len || !part.chars().all(|c| c.is_ascii_hexdigit()) {
            return false;
        }
    }
    true
}

/// Persisted curve data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedCurve {
    pub id: String,
    pub name: String,
    pub temp_source_path: String,
    pub temp_source_label: String,
    pub points: Vec<(f32, f32)>,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub updated_at: u64,
    /// Hysteresis in degrees Celsius (prevents oscillation)
    #[serde(default = "default_hysteresis")]
    pub hysteresis: f32,
    /// Delay in milliseconds before responding to temperature changes
    #[serde(default = "default_delay_ms")]
    pub delay_ms: u32,
    /// Ramp up speed in percent per second (how fast fan speeds up)
    #[serde(default = "default_ramp_up_speed")]
    pub ramp_up_speed: f32,
    /// Ramp down speed in percent per second (how fast fan slows down)
    #[serde(default = "default_ramp_down_speed")]
    pub ramp_down_speed: f32,
}

fn default_hysteresis() -> f32 {
    crate::constants::curve::DEFAULT_HYSTERESIS_CELSIUS
}

fn default_delay_ms() -> u32 {
    crate::constants::curve::DEFAULT_DELAY_MS
}

fn default_ramp_up_speed() -> f32 {
    crate::constants::curve::DEFAULT_RAMP_UP_SPEED
}

fn default_ramp_down_speed() -> f32 {
    crate::constants::curve::DEFAULT_RAMP_DOWN_SPEED
}

/// Collection of all persisted curves
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct CurveStore {
    pub version: u32,
    pub curves: HashMap<String, PersistedCurve>,
}

impl CurveStore {
    /// Create a new empty store
    pub fn new() -> Self {
        Self {
            version: 1,
            curves: HashMap::new(),
        }
    }

    /// Add or update a curve
    pub fn upsert(&mut self, curve: PersistedCurve) {
        let now = current_timestamp();
        let mut curve = curve;
        
        if let Some(existing) = self.curves.get(&curve.id) {
            curve.created_at = existing.created_at;
        } else {
            curve.created_at = now;
        }
        curve.updated_at = now;
        
        self.curves.insert(curve.id.clone(), curve);
    }

    /// Remove a curve by ID
    pub fn remove(&mut self, id: &str) -> Option<PersistedCurve> {
        self.curves.remove(id)
    }

    /// Get a curve by ID
    pub fn get(&self, id: &str) -> Option<&PersistedCurve> {
        self.curves.get(id)
    }

    /// Get all curves
    pub fn all(&self) -> Vec<&PersistedCurve> {
        self.curves.values().collect()
    }

    /// Check if store is empty
    pub fn is_empty(&self) -> bool {
        self.curves.is_empty()
    }

    /// Number of curves
    pub fn len(&self) -> usize {
        self.curves.len()
    }
}

/// Get the path to the curves JSON file
pub fn get_curves_path() -> PathBuf {
    paths::user_config_dir()
        .unwrap_or_else(|| PathBuf::from(".").join("hyperfan"))
        .join("curves.json")
}

/// Load curves from disk
pub fn load_curves() -> Result<CurveStore> {
    let path = get_curves_path();

    if !path.exists() {
        debug!("No curves file found at {:?}, returning empty store", path);
        return Ok(CurveStore::new());
    }

    let contents = fs::read_to_string(&path)
        .map_err(|e| crate::error::HyperfanError::FileRead { path: path.clone(), source: e })?;

    let mut store: CurveStore = serde_json::from_str(&contents)?;

    // MIGRATION: Ensure all curves have valid UUIDs
    // This handles legacy configs with empty IDs or old-style "curve_timestamp" IDs
    let mut needs_save = false;
    for curve in store.curves.values_mut() {
        if curve.id.is_empty() || !is_valid_uuid(&curve.id) {
            // Migrate old-style IDs to proper UUIDs
            let new_id = crate::settings::generate_guid();
            debug!("Migrating curve ID: {} -> {}", curve.id, new_id);
            curve.id = new_id;
            needs_save = true;
        }
    }
    
    // Rebuild HashMap with new IDs if migration occurred
    if needs_save {
        let curves: Vec<_> = store.curves.drain().map(|(_, v)| v).collect();
        for curve in curves {
            store.curves.insert(curve.id.clone(), curve);
        }
        // Save migrated store
        if let Ok(json) = serde_json::to_string_pretty(&store) {
            let _ = fs::write(&path, json);
        }
        info!("Migrated curve IDs to UUIDs");
    }

    info!("Loaded {} curves from {:?}", store.len(), path);
    Ok(store)
}

/// Save curves to disk
pub fn save_curves(store: &CurveStore) -> Result<()> {
    let path = get_curves_path();

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let json = serde_json::to_string_pretty(store)?;

    // CRITICAL: Atomic write - write to temp file then rename
    use std::io::Write;
    let temp_path = path.with_extension("json.tmp");
    
    let mut file = fs::File::create(&temp_path)
        .map_err(|e| crate::error::HyperfanError::FileWrite { path: temp_path.clone(), source: e })?;
    
    file.write_all(json.as_bytes())
        .map_err(|e| crate::error::HyperfanError::FileWrite { path: temp_path.clone(), source: e })?;
    
    file.sync_all()
        .map_err(|e| crate::error::HyperfanError::FileWrite { path: temp_path.clone(), source: e })?;
    
    drop(file);
    
    // Atomic rename
    fs::rename(&temp_path, &path)
        .map_err(|e| crate::error::HyperfanError::FileWrite { path: path.clone(), source: e })?;

    debug!("Saved {} curves to {:?}", store.len(), path);
    Ok(())
}

/// Save a single curve (loads, updates, saves)
pub fn save_curve(curve: PersistedCurve) -> Result<()> {
    let mut store = load_curves().unwrap_or_else(|e| {
        warn!("Failed to load existing curves: {}, starting fresh", e);
        CurveStore::new()
    });

    store.upsert(curve);
    save_curves(&store)
}

/// Delete a curve by ID
pub fn delete_curve(id: &str) -> Result<bool> {
    let mut store = load_curves()?;
    let removed = store.remove(id).is_some();
    
    if removed {
        save_curves(&store)?;
        info!("Deleted curve {}", id);
    }
    
    Ok(removed)
}

/// Update curve points only
pub fn update_curve_points(id: &str, points: Vec<(f32, f32)>) -> Result<bool> {
    let mut store = load_curves()?;
    
    if let Some(curve) = store.curves.get_mut(id) {
        curve.points = points;
        curve.updated_at = current_timestamp();
        save_curves(&store)?;
        debug!("Updated points for curve {}", id);
        Ok(true)
    } else {
        Ok(false)
    }
}

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_curve_store_upsert() {
        let mut store = CurveStore::new();
        
        let curve = PersistedCurve {
            id: "test1".to_string(),
            name: "Test Curve".to_string(),
            temp_source_path: "/sys/class/hwmon/hwmon0/temp1_input".to_string(),
            temp_source_label: "CPU".to_string(),
            points: vec![(30.0, 20.0), (80.0, 100.0)],
            created_at: 0,
            updated_at: 0,
        };
        
        store.upsert(curve);
        assert_eq!(store.len(), 1);
        assert!(store.get("test1").is_some());
    }

    #[test]
    fn test_curve_store_remove() {
        let mut store = CurveStore::new();
        
        let curve = PersistedCurve {
            id: "test1".to_string(),
            name: "Test".to_string(),
            temp_source_path: "/test".to_string(),
            temp_source_label: "Test".to_string(),
            points: vec![],
            created_at: 0,
            updated_at: 0,
        };
        
        store.upsert(curve);
        assert_eq!(store.len(), 1);
        
        store.remove("test1");
        assert!(store.is_empty());
    }
}
