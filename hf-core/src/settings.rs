//! Application Settings
//!
//! Persistent settings stored as JSON in ~/.config/hyperfan/settings.json

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

use crate::data::FanMapping;
use crate::error::Result;
use crate::hw::binding::BindingStore;
use crate::HyperfanError;

// ============================================================================
// Cached Settings (PERFORMANCE: avoid disk I/O on every access)
// ============================================================================

/// Global cached settings - avoids repeated disk reads
/// Updated only when settings are explicitly saved or invalidated
static SETTINGS_CACHE: OnceLock<RwLock<Option<AppSettings>>> = OnceLock::new();

fn get_cache() -> &'static RwLock<Option<AppSettings>> {
    SETTINGS_CACHE.get_or_init(|| RwLock::new(None))
}

/// Get cached settings (fast, no disk I/O)
/// Falls back to loading from disk if cache is empty
pub fn get_cached_settings() -> AppSettings {
    // Try read lock first (fast path)
    if let Ok(guard) = get_cache().read() {
        if let Some(ref settings) = *guard {
            return settings.clone();
        }
    }
    
    // Cache miss - load from disk and populate cache
    let settings = load_settings().unwrap_or_default();
    if let Ok(mut guard) = get_cache().write() {
        *guard = Some(settings.clone());
    }
    settings
}

/// Get just the graph style (most common hot-path access)
/// PERFORMANCE: This is called from draw functions - must be fast
pub fn get_graph_style() -> String {
    get_cached_settings().display.graph_style
}

/// Get just the graph smoothing setting
pub fn get_graph_smoothing() -> String {
    get_cached_settings().display.graph_smoothing
}

/// Get just the frame rate setting (most common hot-path access for animations)
/// PERFORMANCE: This is called from animation tick callbacks - must be fast
pub fn get_frame_rate() -> u32 {
    get_cached_settings().display.frame_rate
}

/// Invalidate the settings cache (call after saving settings)
pub fn invalidate_settings_cache() {
    if let Ok(mut guard) = get_cache().write() {
        *guard = None;
    }
}

/// Update the settings cache with new settings (call after saving)
fn update_cache(settings: &AppSettings) {
    if let Ok(mut guard) = get_cache().write() {
        *guard = Some(settings.clone());
    }
}

/// Application settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    /// General settings
    #[serde(default)]
    pub general: GeneralSettings,
    
    /// Display settings
    #[serde(default)]
    pub display: DisplaySettings,
    
    /// Advanced settings (dangerous features)
    #[serde(default)]
    pub advanced: AdvancedSettings,
    
    /// Active fan-curve pairs
    #[serde(default)]
    pub active_pairs: Vec<FanCurvePair>,
    
    /// Detected PWM-to-fan mappings (from calibration)
    #[serde(default)]
    pub pwm_fan_mappings: Vec<FanMapping>,
    
    /// Whether initial PWM-fan detection has been completed
    #[serde(default)]
    pub detection_completed: bool,
    
    /// Manual PWM-to-fan pairings (user-defined overrides)
    #[serde(default)]
    pub pwm_fan_pairings: Vec<PwmFanPairing>,
    
    /// User-defined friendly names for temperature sensors
    #[serde(default)]
    pub sensor_friendly_names: Vec<SensorFriendlyName>,
}

/// General application settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralSettings {
    /// Start fan control at system boot
    #[serde(default)]
    pub start_at_boot: bool,
    
    /// Sensor polling interval in milliseconds
    #[serde(default = "default_poll_interval")]
    pub poll_interval_ms: u32,
    
    /// Apply curves automatically on startup
    #[serde(default = "default_true")]
    pub apply_curves_on_startup: bool,
    
    /// Default page to show on startup: "dashboard", "curves", "fan_pairing", "sensors", "graphs"
    #[serde(default = "default_page")]
    pub default_page: String,
    
    /// Rate limit: maximum requests per 10-second window (1500-9999)
    /// Applied to both client and daemon when changed
    #[serde(default = "default_rate_limit")]
    pub rate_limit: u32,
}

/// Advanced settings (dangerous features)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AdvancedSettings {
    /// Enable direct EC (Embedded Controller) access
    /// WARNING: This is EXTREMELY DANGEROUS and can damage hardware
    #[serde(default)]
    pub ec_direct_control_enabled: bool,
    
    /// User has acknowledged the EC danger warning
    #[serde(default)]
    pub ec_danger_acknowledged: bool,
    
    /// Timestamp when EC was enabled (for audit)
    #[serde(default)]
    pub ec_enabled_at: Option<u64>,
}

/// Display settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplaySettings {
    /// Temperature unit: "celsius" or "fahrenheit"
    #[serde(default = "default_temp_unit")]
    pub temperature_unit: String,
    
    /// Fan control metric: "percent" or "pwm"
    #[serde(default = "default_fan_metric")]
    pub fan_control_metric: String,
    
    /// Show system tray icon
    #[serde(default)]
    pub show_tray_icon: bool,
    
    /// Graph style: "line", "filled", "stepped"
    #[serde(default = "default_graph_style")]
    pub graph_style: String,
    
    /// Color scheme: "system", "light", "dark"
    #[serde(default = "default_color_scheme")]
    pub color_scheme: String,
    
    /// Display backend: "auto", "wayland", "x11"
    #[serde(default = "default_display_backend")]
    pub display_backend: String,
    
    /// Window manager/desktop environment: "auto", "gnome", "kde"
    /// "auto" detects from current session
    #[serde(default = "default_window_manager")]
    pub window_manager: String,
    
    /// Graph line smoothing: "direct" or "smoothed"
    /// "direct" draws straight lines between points
    /// "smoothed" draws curved bezier lines between points
    #[serde(default = "default_graph_smoothing")]
    pub graph_smoothing: String,
    
    /// Animation frame rate in FPS
    /// Options: 24, 30, 60, 90, 120, or 0 for native monitor refresh rate
    /// Default: 60 FPS
    #[serde(default = "default_frame_rate")]
    pub frame_rate: u32,
    
    /// Window width (saved on close)
    #[serde(default)]
    pub window_width: Option<i32>,
    
    /// Window height (saved on close)
    #[serde(default)]
    pub window_height: Option<i32>,
    
    /// Window X position (saved on close)
    #[serde(default)]
    pub window_x: Option<i32>,
    
    /// Window Y position (saved on close)
    #[serde(default)]
    pub window_y: Option<i32>,
    
    /// Window maximized state (saved on close)
    #[serde(default)]
    pub window_maximized: Option<bool>,
}

/// A manual PWM-to-fan pairing (user-defined)
/// CRITICAL: Contains hardware identification to prevent mispairings after reboot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PwmFanPairing {
    /// Unique GUID identifier for this pairing
    #[serde(default = "generate_guid")]
    pub id: String,
    
    /// Stable UUID for the PWM control (derived from chip+sensor name, survives hwmon reindexing)
    #[serde(default)]
    pub pwm_uuid: Option<String>,
    
    /// PWM control path (may change on reboot due to hwmon reindexing)
    pub pwm_path: String,
    
    /// Stable UUID for the fan sensor (derived from chip+sensor name, survives hwmon reindexing)
    #[serde(default)]
    pub fan_uuid: Option<String>,
    
    /// Fan sensor path (None if unpaired)
    pub fan_path: Option<String>,
    
    /// Fan display name
    pub fan_name: Option<String>,
    
    /// User-defined friendly name for this pairing
    #[serde(default)]
    pub friendly_name: Option<String>,
    
    // ========================================================================
    // HARDWARE IDENTIFICATION (stable across reboots)
    // ========================================================================
    
    /// Driver name from /sys/class/hwmon/hwmonX/name (CRITICAL - stable anchor)
    #[serde(default)]
    pub driver_name: Option<String>,
    
    /// Resolved device symlink target (stable across hwmon reindexing)
    #[serde(default)]
    pub device_path: Option<String>,
    
    /// PWM channel index from filename (e.g., "1" from "pwm1")
    #[serde(default)]
    pub pwm_index: Option<u32>,
    
    /// Fan channel index from filename (e.g., "1" from "fan1_input")
    #[serde(default)]
    pub fan_index: Option<u32>,
    
    /// PWM label if available (from pwmX_label or fanX_label)
    #[serde(default)]
    pub pwm_label: Option<String>,
    
    /// Fan label if available
    #[serde(default)]
    pub fan_label: Option<String>,
    
    /// PCI address if applicable (e.g., "0000:01:00.0" - very stable)
    #[serde(default)]
    pub pci_address: Option<String>,
    
    /// PCI vendor ID (e.g., "0x1002" for AMD)
    #[serde(default)]
    pub pci_vendor_id: Option<String>,
    
    /// PCI device ID
    #[serde(default)]
    pub pci_device_id: Option<String>,
    
    /// Modalias string for driver matching
    #[serde(default)]
    pub modalias: Option<String>,
    
    /// Timestamp when pairing was created
    #[serde(default)]
    pub created_at: Option<u64>,
    
    /// Last validation timestamp
    #[serde(default)]
    pub last_validated_at: Option<u64>,
    
    /// Whether this pairing has been validated on current boot
    #[serde(default)]
    pub validated_this_session: bool,
    
    // ========================================================================
    // GPU-SPECIFIC IDENTIFICATION (for AMD, NVIDIA, Intel discrete GPUs)
    // ========================================================================
    
    /// GPU vendor type ("nvidia", "amd", "intel", or None for motherboard)
    #[serde(default)]
    pub gpu_vendor: Option<String>,
    
    /// GPU index (for multi-GPU systems)
    #[serde(default)]
    pub gpu_index: Option<u32>,
    
    /// Fan index within the GPU (for multi-fan GPUs)
    #[serde(default)]
    pub gpu_fan_index: Option<u32>,
    
    /// GPU name (e.g., "NVIDIA GeForce RTX 3080")
    #[serde(default)]
    pub gpu_name: Option<String>,
    
    /// GPU controller ID (e.g., "nvidia:0:0", "amd:0:0") - CRITICAL stable anchor
    #[serde(default)]
    pub gpu_controller_id: Option<String>,
    
    /// DRM card number (e.g., 0 from /sys/class/drm/card0)
    #[serde(default)]
    pub drm_card_number: Option<u32>,
}

/// User-defined friendly name for a temperature sensor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorFriendlyName {
    /// Sensor path (unique identifier)
    pub path: String,
    
    /// User-defined friendly name
    pub friendly_name: String,
}

/// A fan-curve pair binding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FanCurvePair {
    /// Unique ID
    pub id: String,
    
    /// Display name
    pub name: String,
    
    /// Curve ID to use
    pub curve_id: String,
    
    /// Temperature source path
    pub temp_source_path: String,
    
    /// Fan/PWM controller path (primary fan for backward compatibility)
    pub fan_path: String,
    
    /// Multiple fan/PWM controller paths (supports multiple fans per control)
    #[serde(default)]
    pub fan_paths: Vec<String>,
    
    /// Hysteresis delay in milliseconds (prevents rapid fan speed changes)
    #[serde(default)]
    pub hysteresis_ms: u32,
    
    /// Whether this pair is currently active
    #[serde(default = "default_true")]
    pub active: bool,
}

// Default value functions
fn default_poll_interval() -> u32 { 100 }
fn default_true() -> bool { true }
fn default_temp_unit() -> String { "celsius".to_string() }
fn default_fan_metric() -> String { "percent".to_string() }
fn default_graph_style() -> String { "filled".to_string() }
fn default_color_scheme() -> String { "system".to_string() }
fn default_display_backend() -> String { "auto".to_string() }
fn default_window_manager() -> String { "auto".to_string() }
fn default_graph_smoothing() -> String { "direct".to_string() }
fn default_frame_rate() -> u32 { 60 }
fn default_page() -> String { "dashboard".to_string() }
fn default_rate_limit() -> u32 { 1500 }

/// Check if a string is a valid UUID format (xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx)
fn is_valid_uuid(s: &str) -> bool {
    // UUID format: 8-4-4-4-12 hex chars separated by dashes
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

/// Generate a UUID v4-like GUID for entity identification
pub fn generate_guid() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    // Format: xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx
    let rand_part = timestamp ^ (timestamp >> 32);
    let rand2 = timestamp.wrapping_mul(0x5851F42D4C957F2D);
    format!(
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        (rand_part & 0xFFFFFFFF) as u32,
        ((rand_part >> 32) & 0xFFFF) as u16,
        ((rand2 >> 48) & 0x0FFF) as u16,
        (0x8000 | ((rand2 >> 32) & 0x3FFF)) as u16,
        (rand2 & 0xFFFFFFFFFFFF) as u64
    )
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            general: GeneralSettings::default(),
            display: DisplaySettings::default(),
            advanced: AdvancedSettings::default(),
            active_pairs: Vec::new(),
            pwm_fan_mappings: Vec::new(),
            detection_completed: false,
            pwm_fan_pairings: Vec::new(),
            sensor_friendly_names: Vec::new(),
        }
    }
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            start_at_boot: false,
            poll_interval_ms: 100,
            apply_curves_on_startup: true,
            default_page: "dashboard".to_string(),
            rate_limit: 1500,
        }
    }
}

impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            temperature_unit: "celsius".to_string(),
            fan_control_metric: "percent".to_string(),
            show_tray_icon: false,
            graph_style: "filled".to_string(),
            color_scheme: "system".to_string(),
            display_backend: "auto".to_string(),
            window_manager: "auto".to_string(),
            graph_smoothing: "direct".to_string(),
            frame_rate: 60,
            window_width: None,
            window_height: None,
            window_x: None,
            window_y: None,
            window_maximized: None,
        }
    }
}

/// Get the settings file path
/// Linux/BSD: ~/.config/hyperfan/settings.json
/// 
/// Uses the centralized user_config_dir() from constants.rs which handles:
/// - SUDO_USER for sudo-elevated processes
/// - PKEXEC_UID for pkexec-elevated processes  
/// - Finding first regular user's config when running as daemon (root)
/// - XDG_CONFIG_HOME and HOME fallbacks
pub fn get_settings_path() -> Result<PathBuf> {
    // Use the centralized config path resolution from constants.rs
    // This handles daemon running as root needing to access user's config
    let hyperfan_dir = crate::constants::paths::user_config_dir()
        .ok_or_else(|| HyperfanError::config("Could not determine config directory"))?;
    
    // Create directory if it doesn't exist
    if !hyperfan_dir.exists() {
        fs::create_dir_all(&hyperfan_dir).map_err(|e| {
            HyperfanError::config(format!("Failed to create config directory: {}", e))
        })?;
    }
    
    Ok(hyperfan_dir.join("settings.json"))
}

/// Load settings from JSON file
pub fn load_settings() -> Result<AppSettings> {
    let path = get_settings_path()?;
    
    if !path.exists() {
        // Return defaults if no settings file exists
        return Ok(AppSettings::default());
    }
    
    let content = fs::read_to_string(&path).map_err(|e| {
        HyperfanError::config(format!("Failed to read settings file: {}", e))
    })?;
    
    let mut settings: AppSettings = serde_json::from_str(&content).map_err(|e| {
        HyperfanError::config(format!("Failed to parse settings JSON: {}", e))
    })?;
    
    // MIGRATION: Ensure all entities have valid UUIDs
    // This handles legacy configs with empty IDs or old-style "pair_timestamp" IDs
    let mut needs_save = false;
    
    // Migrate PWM-fan pairings without proper UUIDs
    for pairing in &mut settings.pwm_fan_pairings {
        if pairing.id.is_empty() || !is_valid_uuid(&pairing.id) {
            pairing.id = generate_guid();
            needs_save = true;
        }
    }
    
    // Migrate fan-curve pairs without proper UUIDs
    for pair in &mut settings.active_pairs {
        if pair.id.is_empty() || !is_valid_uuid(&pair.id) {
            pair.id = generate_guid();
            needs_save = true;
        }
    }
    
    // Save migrated settings if any changes were made
    if needs_save {
        // Use a separate save to avoid recursion - write directly
        if let Ok(json) = serde_json::to_string_pretty(&settings) {
            let _ = fs::write(&path, json);
        }
    }
    
    Ok(settings)
}

/// Save settings to JSON file
/// Uses atomic write (temp file + rename) to prevent corruption on crash
pub fn save_settings(settings: &AppSettings) -> Result<()> {
    use std::io::Write;
    
    let path = get_settings_path()?;
    
    let json = serde_json::to_string_pretty(settings).map_err(|e| {
        HyperfanError::config(format!("Failed to serialize settings: {}", e))
    })?;
    
    // CRITICAL: Atomic write - write to temp file then rename
    // This prevents corruption if process crashes during write
    let temp_path = path.with_extension("json.tmp");
    
    let mut file = fs::File::create(&temp_path).map_err(|e| {
        HyperfanError::config(format!("Failed to create temp file: {}", e))
    })?;
    
    file.write_all(json.as_bytes()).map_err(|e| {
        HyperfanError::config(format!("Failed to write to temp file: {}", e))
    })?;
    
    file.sync_all().map_err(|e| {
        HyperfanError::config(format!("Failed to sync temp file: {}", e))
    })?;
    
    drop(file);
    
    // Atomic rename - this is the critical operation that makes the write atomic
    fs::rename(&temp_path, &path).map_err(|e| {
        HyperfanError::config(format!("Failed to rename temp file: {}", e))
    })?;
    
    // Update cache after successful save
    update_cache(settings);
    
    Ok(())
}

/// Update a single setting value and save
pub fn update_setting<F>(updater: F) -> Result<AppSettings>
where
    F: FnOnce(&mut AppSettings),
{
    let mut settings = load_settings()?;
    updater(&mut settings);
    save_settings(&settings)?;
    Ok(settings)
}

/// Add or update a fan-curve pair
pub fn save_pair(pair: FanCurvePair) -> Result<()> {
    update_setting(|settings| {
        // Remove existing pair with same ID
        settings.active_pairs.retain(|p| p.id != pair.id);
        settings.active_pairs.push(pair);
    })?;
    Ok(())
}

/// Remove a fan-curve pair
pub fn delete_pair(pair_id: &str) -> Result<()> {
    update_setting(|settings| {
        settings.active_pairs.retain(|p| p.id != pair_id);
    })?;
    Ok(())
}

/// Get all active pairs
pub fn get_active_pairs() -> Result<Vec<FanCurvePair>> {
    let settings = load_settings()?;
    Ok(settings.active_pairs.into_iter().filter(|p| p.active).collect())
}

/// Check if PWM-fan detection has been completed
pub fn is_detection_completed() -> Result<bool> {
    let settings = load_settings()?;
    Ok(settings.detection_completed)
}

/// Save PWM-fan mappings from detection
pub fn save_pwm_fan_mappings(mappings: Vec<FanMapping>) -> Result<()> {
    update_setting(|settings| {
        settings.pwm_fan_mappings = mappings;
        settings.detection_completed = true;
    })?;
    Ok(())
}

/// Get saved PWM-fan mappings
pub fn get_pwm_fan_mappings() -> Result<Vec<FanMapping>> {
    let settings = load_settings()?;
    Ok(settings.pwm_fan_mappings)
}

/// Clear PWM-fan mappings (to re-run detection)
pub fn clear_pwm_fan_mappings() -> Result<()> {
    update_setting(|settings| {
        settings.pwm_fan_mappings.clear();
        settings.detection_completed = false;
    })?;
    Ok(())
}

// ============================================================================
// PWM-Fan Pairing CRUD Operations (UUID-based)
// ============================================================================

/// Save or update a PWM-fan pairing (upsert by ID)
/// If a pairing with the same ID exists, it will be replaced.
/// If no ID match but same pwm_path exists, the old one is removed first.
pub fn save_pwm_pairing(pairing: PwmFanPairing) -> Result<()> {
    update_setting(|settings| {
        // Remove existing pairing with same ID (update case)
        settings.pwm_fan_pairings.retain(|p| p.id != pairing.id);
        // Also remove any pairing with same pwm_path to prevent duplicates
        settings.pwm_fan_pairings.retain(|p| p.pwm_path != pairing.pwm_path);
        settings.pwm_fan_pairings.push(pairing);
    })?;
    Ok(())
}

/// Delete a PWM-fan pairing by ID
pub fn delete_pwm_pairing(pairing_id: &str) -> Result<bool> {
    let mut removed = false;
    update_setting(|settings| {
        let before = settings.pwm_fan_pairings.len();
        settings.pwm_fan_pairings.retain(|p| p.id != pairing_id);
        removed = settings.pwm_fan_pairings.len() < before;
    })?;
    Ok(removed)
}

/// Delete a PWM-fan pairing by PWM path (legacy compatibility)
pub fn delete_pwm_pairing_by_path(pwm_path: &str) -> Result<bool> {
    let mut removed = false;
    update_setting(|settings| {
        let before = settings.pwm_fan_pairings.len();
        settings.pwm_fan_pairings.retain(|p| p.pwm_path != pwm_path);
        removed = settings.pwm_fan_pairings.len() < before;
    })?;
    Ok(removed)
}

/// Get a PWM-fan pairing by ID
pub fn get_pwm_pairing(pairing_id: &str) -> Result<Option<PwmFanPairing>> {
    let settings = load_settings()?;
    Ok(settings.pwm_fan_pairings.into_iter().find(|p| p.id == pairing_id))
}

/// Get a PWM-fan pairing by PWM path
pub fn get_pwm_pairing_by_path(pwm_path: &str) -> Result<Option<PwmFanPairing>> {
    let settings = load_settings()?;
    Ok(settings.pwm_fan_pairings.into_iter().find(|p| p.pwm_path == pwm_path))
}

/// Get all PWM-fan pairings
pub fn get_all_pwm_pairings() -> Result<Vec<PwmFanPairing>> {
    let settings = load_settings()?;
    Ok(settings.pwm_fan_pairings)
}

/// Update a PWM-fan pairing's friendly name by ID
pub fn update_pwm_pairing_name(pairing_id: &str, friendly_name: Option<&str>) -> Result<bool> {
    let mut updated = false;
    update_setting(|settings| {
        if let Some(pairing) = settings.pwm_fan_pairings.iter_mut().find(|p| p.id == pairing_id) {
            pairing.friendly_name = friendly_name.map(String::from);
            updated = true;
        }
    })?;
    Ok(updated)
}

// ============================================================================
// Binding Store Management
// ============================================================================

/// Load the binding store from disk
pub fn load_binding_store() -> Result<BindingStore> {
    BindingStore::load().map_err(|e| HyperfanError::config(e))
}

/// Save the binding store to disk
pub fn save_binding_store(store: &BindingStore) -> Result<()> {
    store.save().map_err(|e| HyperfanError::config(e))
}

/// Get the binding store path
pub fn get_binding_store_path() -> Result<PathBuf> {
    BindingStore::get_store_path()
        .ok_or_else(|| HyperfanError::config("Could not determine binding store path"))
}

/// Check if binding store exists
pub fn binding_store_exists() -> bool {
    BindingStore::get_store_path()
        .map(|p| p.exists())
        .unwrap_or(false)
}

// ============================================================================
// Sensor Friendly Names
// ============================================================================

/// Get the friendly name for a sensor, if one exists
pub fn get_sensor_friendly_name(path: &str) -> Result<Option<String>> {
    let settings = load_settings()?;
    Ok(settings.sensor_friendly_names
        .iter()
        .find(|s| s.path == path)
        .map(|s| s.friendly_name.clone()))
}

/// Set a friendly name for a sensor
pub fn set_sensor_friendly_name(path: &str, friendly_name: &str) -> Result<()> {
    update_setting(|settings| {
        // Remove existing entry if present
        settings.sensor_friendly_names.retain(|s| s.path != path);
        
        // Add new entry (only if name is not empty)
        if !friendly_name.is_empty() {
            settings.sensor_friendly_names.push(SensorFriendlyName {
                path: path.to_string(),
                friendly_name: friendly_name.to_string(),
            });
        }
    })?;
    Ok(())
}

/// Get all sensor friendly names
pub fn get_all_sensor_friendly_names() -> Result<Vec<SensorFriendlyName>> {
    let settings = load_settings()?;
    Ok(settings.sensor_friendly_names)
}

// ============================================================================
// Window Manager Detection
// ============================================================================

/// Detected window manager / desktop environment
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowManager {
    Gnome,
    Kde,
    Unknown,
}

impl std::fmt::Display for WindowManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WindowManager::Gnome => write!(f, "GNOME"),
            WindowManager::Kde => write!(f, "KDE"),
            WindowManager::Unknown => write!(f, "Unknown"),
        }
    }
}

impl WindowManager {
    /// Convert to settings string value
    pub fn to_setting_value(&self) -> &'static str {
        match self {
            WindowManager::Gnome => "gnome",
            WindowManager::Kde => "kde",
            WindowManager::Unknown => "auto",
        }
    }
    
    /// Parse from settings string value
    pub fn from_setting_value(value: &str) -> Self {
        match value.to_lowercase().as_str() {
            "gnome" => WindowManager::Gnome,
            "kde" => WindowManager::Kde,
            _ => WindowManager::Unknown,
        }
    }
}

/// Detect the current desktop environment from session variables
pub fn detect_desktop_environment() -> WindowManager {
    fn tokens(s: &str) -> impl Iterator<Item = String> + '_ {
        s.split(|c: char| c == ':' || c == ';' || c == ',' || c.is_whitespace())
            .filter(|t| !t.is_empty())
            .map(|t| t.to_lowercase())
    }

    // Check XDG_CURRENT_DESKTOP first (most reliable)
    if let Ok(desktop) = std::env::var("XDG_CURRENT_DESKTOP") {
        let any_gnome = tokens(&desktop).any(|t| t.contains("gnome") || t.contains("unity"));
        if any_gnome {
            return WindowManager::Gnome;
        }
        let any_kde = tokens(&desktop).any(|t| t == "kde" || t.contains("plasma"));
        if any_kde {
            return WindowManager::Kde;
        }
    }
    
    // Check DESKTOP_SESSION
    if let Ok(session) = std::env::var("DESKTOP_SESSION") {
        let any_gnome = tokens(&session).any(|t| t.contains("gnome"));
        if any_gnome {
            return WindowManager::Gnome;
        }
        let any_kde = tokens(&session).any(|t| t == "kde" || t.contains("plasma"));
        if any_kde {
            return WindowManager::Kde;
        }
    }
    
    // Check XDG_SESSION_DESKTOP
    if let Ok(session) = std::env::var("XDG_SESSION_DESKTOP") {
        let any_gnome = tokens(&session).any(|t| t.contains("gnome"));
        if any_gnome {
            return WindowManager::Gnome;
        }
        let any_kde = tokens(&session).any(|t| t == "kde" || t.contains("plasma"));
        if any_kde {
            return WindowManager::Kde;
        }
    }
    
    // Check for KDE-specific env var
    if std::env::var("KDE_FULL_SESSION").is_ok() {
        return WindowManager::Kde;
    }
    
    // Check for GNOME-specific env var
    if std::env::var("GNOME_DESKTOP_SESSION_ID").is_ok() {
        return WindowManager::Gnome;
    }
    
    WindowManager::Unknown
}

/// Get the effective window manager to use based on settings
/// If "auto", detects from session; otherwise uses the configured value
pub fn get_effective_window_manager() -> WindowManager {
    let settings = load_settings().unwrap_or_default();
    
    match settings.display.window_manager.as_str() {
        "gnome" => WindowManager::Gnome,
        "kde" => WindowManager::Kde,
        _ => detect_desktop_environment(), // "auto" or unknown -> detect
    }
}

// ============================================================================
// PWM Hardware Identification Extraction
// ============================================================================

/// Extract hardware identification from a PWM path for safe pairing storage
/// This is CRITICAL for preventing fan mispairings after hwmon reindexing
/// 
/// Handles both sysfs paths (/sys/class/hwmon/...) and GPU virtual paths:
/// - nvidia:gpu_index:fan_index (e.g., "nvidia:0:0")
/// - amd:card_num:fan_index (e.g., "amd:0:0")
/// - intel:card_num:fan_index (e.g., "intel:0:0")
pub fn extract_pwm_hardware_id(pwm_path: &str) -> PwmHardwareId {
    use std::path::Path;
    
    let mut hw_id = PwmHardwareId::default();
    
    // Handle GPU virtual paths (nvidia:X:Y, amd:X:Y, intel:X:Y)
    if pwm_path.starts_with("nvidia:") || pwm_path.starts_with("amd:") || pwm_path.starts_with("intel:") {
        let parts: Vec<&str> = pwm_path.split(':').collect();
        if parts.len() >= 3 {
            let vendor = parts[0];
            // BUG FIX: Use match instead of unwrap_or(0) to avoid silent failures
            let gpu_idx: u32 = match parts[1].parse() {
                Ok(idx) => idx,
                Err(e) => {
                    tracing::warn!("Failed to parse GPU index from '{}': {}", parts[1], e);
                    return hw_id;
                }
            };
            let fan_idx: u32 = match parts[2].parse() {
                Ok(idx) => idx,
                Err(e) => {
                    tracing::warn!("Failed to parse fan index from '{}': {}", parts[2], e);
                    return hw_id;
                }
            };
            
            hw_id.gpu_vendor = Some(vendor.to_string());
            hw_id.gpu_index = Some(gpu_idx);
            hw_id.gpu_fan_index = Some(fan_idx);
            hw_id.gpu_controller_id = Some(pwm_path.to_string());
            
            // For AMD/Intel, try to get additional info from DRM
            if vendor == "amd" || vendor == "intel" {
                hw_id.drm_card_number = Some(gpu_idx);
                
                // Try to read PCI info from DRM device
                let drm_device_path = format!("/sys/class/drm/card{}/device", gpu_idx);
                let device_path = Path::new(&drm_device_path);
                if device_path.exists() {
                    hw_id.pci_vendor_id = read_sysfs_file(&device_path.join("vendor"));
                    hw_id.pci_device_id = read_sysfs_file(&device_path.join("device"));
                    
                    if let Ok(real_path) = std::fs::canonicalize(device_path) {
                        hw_id.device_path = Some(real_path.to_string_lossy().to_string());
                        if let Some(addr) = extract_pci_address(&real_path.to_string_lossy()) {
                            hw_id.pci_address = Some(addr);
                        }
                    }
                }
                
                // Set driver name based on vendor
                hw_id.driver_name = Some(if vendor == "amd" { "amdgpu".to_string() } else { "i915".to_string() });
            } else if vendor == "nvidia" {
                // For NVIDIA, get GPU name via nvidia-smi if possible
                hw_id.driver_name = Some("nvidia".to_string());
                hw_id.pci_vendor_id = Some("0x10de".to_string());
                
                // Try to get GPU name from nvidia-smi
                if let Ok(output) = std::process::Command::new("nvidia-smi")
                    .args(["--query-gpu=name,pci.bus_id", "--format=csv,noheader,nounits", "-i", &gpu_idx.to_string()])
                    .output()
                {
                    if output.status.success() {
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        let parts: Vec<&str> = stdout.trim().split(',').map(|s| s.trim()).collect();
                        if !parts.is_empty() {
                            hw_id.gpu_name = Some(parts[0].to_string());
                        }
                        if parts.len() > 1 && !parts[1].is_empty() && parts[1] != "[N/A]" {
                            hw_id.pci_address = Some(parts[1].to_string());
                        }
                    }
                }
            }
            
            return hw_id;
        }
    }
    
    // Standard sysfs path handling
    let path = Path::new(pwm_path);
    
    // Get parent hwmon directory
    let hwmon_dir = path.parent();
    
    if let Some(hwmon) = hwmon_dir {
        // Read driver name (CRITICAL - stable anchor)
        let name_path = hwmon.join("name");
        if let Ok(name) = std::fs::read_to_string(&name_path) {
            hw_id.driver_name = Some(name.trim().to_string());
        }
        
        // Resolve device symlink (stable across hwmon reindexing)
        let device_path = hwmon.join("device");
        if device_path.exists() {
            if let Ok(real_path) = std::fs::canonicalize(&device_path) {
                hw_id.device_path = Some(real_path.to_string_lossy().to_string());
                
                // Extract PCI info if applicable
                let path_str = real_path.to_string_lossy();
                if path_str.contains("/pci") {
                    // Extract PCI address (e.g., "0000:01:00.0")
                    if let Some(addr) = extract_pci_address(&path_str) {
                        hw_id.pci_address = Some(addr);
                    }
                    
                    // Read PCI vendor/device IDs
                    hw_id.pci_vendor_id = read_sysfs_file(&real_path.join("vendor"));
                    hw_id.pci_device_id = read_sysfs_file(&real_path.join("device"));
                }
            }
        }
        
        // Read modalias
        let modalias_path = hwmon.join("device/modalias");
        if let Ok(modalias) = std::fs::read_to_string(&modalias_path) {
            hw_id.modalias = Some(modalias.trim().to_string());
        }
    }
    
    // Extract PWM index from filename (e.g., "pwm1" -> 1)
    if let Some(filename) = path.file_name().and_then(|f| f.to_str()) {
        let index_str: String = filename.chars().filter(|c| c.is_ascii_digit()).collect();
        if let Ok(idx) = index_str.parse::<u32>() {
            hw_id.pwm_index = Some(idx);
        }
        
        // Try to read PWM label
        if let Some(hwmon) = hwmon_dir {
            let label_path = hwmon.join(format!("{}_label", filename.replace("_input", "")));
            if let Ok(label) = std::fs::read_to_string(&label_path) {
                hw_id.pwm_label = Some(label.trim().to_string());
            }
        }
    }
    
    hw_id
}

/// Extract hardware identification from a fan path
pub fn extract_fan_hardware_id(fan_path: &str) -> FanHardwareId {
    use std::path::Path;
    
    let path = Path::new(fan_path);
    let mut hw_id = FanHardwareId::default();
    
    // Extract fan index from filename (e.g., "fan1_input" -> 1)
    if let Some(filename) = path.file_name().and_then(|f| f.to_str()) {
        let index_str: String = filename.chars().filter(|c| c.is_ascii_digit()).collect();
        if let Ok(idx) = index_str.parse::<u32>() {
            hw_id.fan_index = Some(idx);
        }
        
        // Try to read fan label
        if let Some(hwmon) = path.parent() {
            let base_name = filename.replace("_input", "");
            let label_path = hwmon.join(format!("{}_label", base_name));
            if let Ok(label) = std::fs::read_to_string(&label_path) {
                hw_id.fan_label = Some(label.trim().to_string());
            }
        }
    }
    
    hw_id
}

/// Hardware identification for a PWM channel
#[derive(Debug, Clone, Default)]
pub struct PwmHardwareId {
    pub driver_name: Option<String>,
    pub device_path: Option<String>,
    pub pwm_index: Option<u32>,
    pub pwm_label: Option<String>,
    pub pci_address: Option<String>,
    pub pci_vendor_id: Option<String>,
    pub pci_device_id: Option<String>,
    pub modalias: Option<String>,
    // GPU-specific fields
    pub gpu_vendor: Option<String>,
    pub gpu_index: Option<u32>,
    pub gpu_fan_index: Option<u32>,
    pub gpu_name: Option<String>,
    pub gpu_controller_id: Option<String>,
    pub drm_card_number: Option<u32>,
}

/// Hardware identification for a fan channel
#[derive(Debug, Clone, Default)]
pub struct FanHardwareId {
    pub fan_index: Option<u32>,
    pub fan_label: Option<String>,
}

/// Helper to extract PCI address from path
fn extract_pci_address(path: &str) -> Option<String> {
    // Look for pattern like "0000:01:00.0"
    let re_pattern = regex::Regex::new(r"([0-9a-fA-F]{4}:[0-9a-fA-F]{2}:[0-9a-fA-F]{2}\.[0-9])").ok()?;
    re_pattern.find(path).map(|m| m.as_str().to_string())
}

/// Helper to read sysfs file
fn read_sysfs_file(path: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

/// Create a fully fingerprinted PwmFanPairing from a PWM path
/// This ensures ZERO DRIFT by capturing all stable hardware identifiers
pub fn create_fingerprinted_pairing(
    pwm_path: &str,
    fan_path: Option<&str>,
    fan_name: Option<&str>,
    friendly_name: Option<&str>,
) -> PwmFanPairing {
    let hw_id = extract_pwm_hardware_id(pwm_path);
    let fan_hw_id = fan_path.map(extract_fan_hardware_id);
    
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .ok();
    
    PwmFanPairing {
        id: generate_guid(),
        pwm_uuid: None, // Will be set by caller if available
        pwm_path: pwm_path.to_string(),
        fan_uuid: None, // Will be set by caller if available
        fan_path: fan_path.map(|s| s.to_string()),
        fan_name: fan_name.map(|s| s.to_string()),
        friendly_name: friendly_name.map(|s| s.to_string()),
        // Hardware identification
        driver_name: hw_id.driver_name,
        device_path: hw_id.device_path,
        pwm_index: hw_id.pwm_index,
        fan_index: fan_hw_id.as_ref().and_then(|f| f.fan_index),
        pwm_label: hw_id.pwm_label,
        fan_label: fan_hw_id.and_then(|f| f.fan_label),
        pci_address: hw_id.pci_address,
        pci_vendor_id: hw_id.pci_vendor_id,
        pci_device_id: hw_id.pci_device_id,
        modalias: hw_id.modalias,
        created_at: now,
        last_validated_at: now,
        validated_this_session: true,
        // GPU-specific fields
        gpu_vendor: hw_id.gpu_vendor,
        gpu_index: hw_id.gpu_index,
        gpu_fan_index: hw_id.gpu_fan_index,
        gpu_name: hw_id.gpu_name,
        gpu_controller_id: hw_id.gpu_controller_id,
        drm_card_number: hw_id.drm_card_number,
    }
}

/// Validation result for a PWM-fan pairing
#[derive(Debug, Clone)]
pub struct PairingValidation {
    /// Whether the pairing is valid
    pub is_valid: bool,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f32,
    /// Resolved PWM path (may differ from stored if hwmon reindexed)
    pub resolved_pwm_path: Option<String>,
    /// Validation messages
    pub messages: Vec<String>,
}

/// Validate a stored pairing against current system state
/// Returns the resolved PWM path if validation succeeds
/// 
/// This is CRITICAL for preventing fan mispairings after hwmon reindexing
/// REQUIRES 100% CONFIDENCE - ALL fingerprint fields must match exactly
pub fn validate_pairing(pairing: &PwmFanPairing) -> PairingValidation {
    use std::path::Path;
    
    let mut messages = Vec::new();
    let mut matched_fields = 0u32;
    let mut total_fields = 0u32;
    
    // GPU pairings use stable controller IDs - validate ALL fields for 100% match
    if let Some(ref controller_id) = pairing.gpu_controller_id {
        // For GPU pairings, validate by re-enumerating GPU controllers
        let gpu_controllers = crate::enumerate_gpu_pwm_controllers();
        
        for controller in &gpu_controllers {
            if controller.id == *controller_id {
                // Found matching controller - validate ALL fields for 100% confidence
                total_fields += 1;
                matched_fields += 1;
                messages.push(format!("✓ GPU controller ID matches: {}", controller_id));
                
                // Validate GPU vendor (REQUIRED)
                total_fields += 1;
                if let Some(ref stored_vendor) = pairing.gpu_vendor {
                    let current_vendor = controller.vendor.to_string().to_lowercase();
                    if stored_vendor.to_lowercase() == current_vendor {
                        matched_fields += 1;
                        messages.push(format!("✓ GPU vendor matches: {}", stored_vendor));
                    } else {
                        messages.push(format!("✗ GPU vendor mismatch: stored={}, current={}", stored_vendor, current_vendor));
                    }
                } else {
                    messages.push("✗ GPU vendor not stored in fingerprint".to_string());
                }
                
                // Validate GPU index (REQUIRED)
                total_fields += 1;
                if pairing.gpu_index == Some(controller.gpu_index) {
                    matched_fields += 1;
                    messages.push(format!("✓ GPU index matches: {}", controller.gpu_index));
                } else {
                    messages.push(format!("✗ GPU index mismatch: stored={:?}, current={}", pairing.gpu_index, controller.gpu_index));
                }
                
                // Validate fan index (REQUIRED)
                total_fields += 1;
                if pairing.gpu_fan_index == Some(controller.fan_index) {
                    matched_fields += 1;
                    messages.push(format!("✓ Fan index matches: {}", controller.fan_index));
                } else {
                    messages.push(format!("✗ Fan index mismatch: stored={:?}, current={}", pairing.gpu_fan_index, controller.fan_index));
                }
                
                // Validate PCI bus ID (REQUIRED if available)
                if let Some(ref stored_pci) = pairing.pci_address {
                    total_fields += 1;
                    if let Some(ref current_pci) = controller.pci_bus_id {
                        if stored_pci == current_pci {
                            matched_fields += 1;
                            messages.push(format!("✓ PCI address matches: {}", stored_pci));
                        } else {
                            messages.push(format!("✗ PCI address mismatch: stored={}, current={}", stored_pci, current_pci));
                        }
                    } else {
                        messages.push("✗ PCI address not available on current controller".to_string());
                    }
                }
                
                // Validate PCI vendor ID (REQUIRED if available)
                if let Some(ref stored_vendor_id) = pairing.pci_vendor_id {
                    total_fields += 1;
                    // For GPU controllers, we know the vendor IDs
                    let expected_vendor = match controller.vendor {
                        crate::data::GpuVendor::Nvidia => "0x10de",
                        crate::data::GpuVendor::Amd => "0x1002",
                        crate::data::GpuVendor::Intel => "0x8086",
                    };
                    if stored_vendor_id == expected_vendor {
                        matched_fields += 1;
                        messages.push(format!("✓ PCI vendor ID matches: {}", stored_vendor_id));
                    } else {
                        messages.push(format!("✗ PCI vendor ID mismatch: stored={}, expected={}", stored_vendor_id, expected_vendor));
                    }
                }
                
                // Calculate confidence - MUST be 100% for valid pairing
                let confidence = if total_fields > 0 {
                    matched_fields as f32 / total_fields as f32
                } else {
                    0.0
                };
                
                let is_valid = matched_fields == total_fields; // 100% match required
                
                if is_valid {
                    messages.push(format!("✓ VALIDATED: {}/{} fields match (100%)", matched_fields, total_fields));
                } else {
                    messages.push(format!("✗ REJECTED: {}/{} fields match ({:.0}%) - 100% required", 
                        matched_fields, total_fields, confidence * 100.0));
                }
                
                return PairingValidation {
                    is_valid,
                    confidence,
                    resolved_pwm_path: if is_valid { Some(controller.pwm_path.clone()) } else { None },
                    messages,
                };
            }
        }
        
        // GPU controller not found
        messages.push(format!("✗ GPU controller {} not found on system", controller_id));
        return PairingValidation {
            is_valid: false,
            confidence: 0.0,
            resolved_pwm_path: None,
            messages,
        };
    }
    
    // Standard sysfs pairing validation - REQUIRES 100% MATCH
    let stored_path = Path::new(&pairing.pwm_path);
    
    // Check if stored path still exists (REQUIRED)
    if stored_path.exists() {
        total_fields += 1;
        matched_fields += 1;
        messages.push("✓ Stored path exists".to_string());
        
        // Validate driver name (REQUIRED)
        if let Some(ref stored_driver) = pairing.driver_name {
            total_fields += 1;
            if let Some(hwmon) = stored_path.parent() {
                let name_path = hwmon.join("name");
                if let Ok(current_driver) = std::fs::read_to_string(&name_path) {
                    if current_driver.trim() == stored_driver {
                        matched_fields += 1;
                        messages.push(format!("✓ Driver name matches: {}", stored_driver));
                    } else {
                        messages.push(format!("✗ Driver mismatch: stored={}, current={}", stored_driver, current_driver.trim()));
                    }
                } else {
                    messages.push("✗ Cannot read driver name".to_string());
                }
            }
        }
        
        // Validate PWM index (REQUIRED)
        if let Some(stored_idx) = pairing.pwm_index {
            total_fields += 1;
            if let Some(filename) = stored_path.file_name().and_then(|f| f.to_str()) {
                let current_idx: String = filename.chars().filter(|c| c.is_ascii_digit()).collect();
                if let Ok(idx) = current_idx.parse::<u32>() {
                    if idx == stored_idx {
                        matched_fields += 1;
                        messages.push(format!("✓ PWM index matches: {}", stored_idx));
                    } else {
                        messages.push(format!("✗ PWM index mismatch: stored={}, current={}", stored_idx, idx));
                    }
                }
            }
        }
        
        // Validate PCI address (REQUIRED if available)
        if let Some(ref stored_pci) = pairing.pci_address {
            total_fields += 1;
            let current_hw_id = extract_pwm_hardware_id(&pairing.pwm_path);
            if let Some(ref current_pci) = current_hw_id.pci_address {
                if stored_pci == current_pci {
                    matched_fields += 1;
                    messages.push(format!("✓ PCI address matches: {}", stored_pci));
                } else {
                    messages.push(format!("✗ PCI address mismatch: stored={}, current={}", stored_pci, current_pci));
                }
            } else {
                messages.push("✗ PCI address not available on current device".to_string());
            }
        }
        
        // Validate device path (REQUIRED if available)
        if let Some(ref stored_device) = pairing.device_path {
            total_fields += 1;
            let current_hw_id = extract_pwm_hardware_id(&pairing.pwm_path);
            if let Some(ref current_device) = current_hw_id.device_path {
                if stored_device == current_device {
                    matched_fields += 1;
                    messages.push("✓ Device path matches".to_string());
                } else {
                    messages.push(format!("✗ Device path mismatch"));
                }
            }
        }
        
        // Calculate confidence - MUST be 100% for valid pairing
        let confidence = if total_fields > 0 {
            matched_fields as f32 / total_fields as f32
        } else {
            0.0
        };
        
        let is_valid = matched_fields == total_fields; // 100% match required
        
        if is_valid {
            messages.push(format!("✓ VALIDATED: {}/{} fields match (100%)", matched_fields, total_fields));
        } else {
            messages.push(format!("✗ REJECTED: {}/{} fields match ({:.0}%) - 100% required", 
                matched_fields, total_fields, confidence * 100.0));
        }
        
        return PairingValidation {
            is_valid,
            confidence,
            resolved_pwm_path: if is_valid { Some(pairing.pwm_path.clone()) } else { None },
            messages,
        };
    }
    
    // Path doesn't exist - try to resolve by fingerprint (100% match required)
    messages.push("✗ Stored path does not exist, attempting fingerprint resolution".to_string());
    
    // Try to find matching hwmon by driver name AND PCI address AND PWM index (ALL must match)
    if let (Some(ref driver), Some(ref pci_addr), Some(pwm_idx)) = (&pairing.driver_name, &pairing.pci_address, pairing.pwm_index) {
        if let Ok(chips) = crate::enumerate_hwmon_chips() {
            for chip in chips {
                // Driver name must match exactly
                if chip.name != *driver {
                    continue;
                }
                
                // PCI address must match exactly
                let device_path = chip.path.join("device");
                if let Ok(real_path) = std::fs::canonicalize(&device_path) {
                    if let Some(current_pci) = extract_pci_address(&real_path.to_string_lossy()) {
                        if current_pci != *pci_addr {
                            continue;
                        }
                        
                        // PWM path must exist
                        let new_pwm_path = chip.path.join(format!("pwm{}", pwm_idx));
                        if !new_pwm_path.exists() {
                            continue;
                        }
                        
                        // Validate device path if stored
                        if let Some(ref stored_device) = pairing.device_path {
                            if real_path.to_string_lossy() != *stored_device {
                                messages.push("✗ Device path mismatch during resolution".to_string());
                                continue;
                            }
                        }
                        
                        // ALL fields matched - 100% confidence
                        messages.push(format!("✓ Driver matches: {}", driver));
                        messages.push(format!("✓ PCI address matches: {}", pci_addr));
                        messages.push(format!("✓ PWM index matches: {}", pwm_idx));
                        messages.push(format!("✓ Resolved to new path: {:?}", new_pwm_path));
                        messages.push("✓ VALIDATED: 100% fingerprint match after hwmon reindexing".to_string());
                        
                        return PairingValidation {
                            is_valid: true,
                            confidence: 1.0, // 100% match
                            resolved_pwm_path: Some(new_pwm_path.to_string_lossy().to_string()),
                            messages,
                        };
                    }
                }
            }
        }
    }
    
    messages.push("✗ Could not resolve pairing to current hardware - 100% match required".to_string());
    PairingValidation {
        is_valid: false,
        confidence: 0.0,
        resolved_pwm_path: None,
        messages,
    }
}

// ============================================================================
// Temperature Graph Persistence
// ============================================================================

/// Persisted temperature graph configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedGraph {
    /// Unique identifier
    pub id: String,
    
    /// User-defined name
    pub name: String,
    
    /// Temperature source path (hwmon path or "gpu:<index>:<temp_name>")
    pub temp_source_path: String,
    
    /// Human-readable label for the temperature source
    pub temp_source_label: String,
}

/// Get the temperature graphs file path
/// Linux/BSD: ~/.config/hyperfan/temp_graphs.json
pub fn get_temp_graphs_path() -> Result<PathBuf> {
    let config_dir = if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg)
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".config")
    } else {
        dirs::config_dir()
            .ok_or_else(|| HyperfanError::config("Could not determine config directory"))?
    };
    
    let hyperfan_dir = config_dir.join("hyperfan");
    
    if !hyperfan_dir.exists() {
        fs::create_dir_all(&hyperfan_dir).map_err(|e| {
            HyperfanError::config(format!("Failed to create config directory: {}", e))
        })?;
    }
    
    Ok(hyperfan_dir.join("temp_graphs.json"))
}

/// Load temperature graphs from JSON file
pub fn load_temp_graphs() -> Result<Vec<PersistedGraph>> {
    let path = get_temp_graphs_path()?;
    
    if !path.exists() {
        return Ok(Vec::new());
    }
    
    let content = fs::read_to_string(&path).map_err(|e| {
        HyperfanError::config(format!("Failed to read temp graphs file: {}", e))
    })?;
    
    let graphs: Vec<PersistedGraph> = serde_json::from_str(&content).map_err(|e| {
        HyperfanError::config(format!("Failed to parse temp graphs JSON: {}", e))
    })?;
    
    Ok(graphs)
}

/// Save temperature graphs to JSON file
/// Uses atomic write (temp file + rename) to prevent corruption on crash
pub fn save_temp_graphs(graphs: &[PersistedGraph]) -> Result<()> {
    use std::io::Write;
    
    let path = get_temp_graphs_path()?;
    
    let json = serde_json::to_string_pretty(graphs).map_err(|e| {
        HyperfanError::config(format!("Failed to serialize temp graphs: {}", e))
    })?;
    
    // CRITICAL: Atomic write - write to temp file then rename
    let temp_path = path.with_extension("json.tmp");
    
    let mut file = fs::File::create(&temp_path).map_err(|e| {
        HyperfanError::config(format!("Failed to create temp file: {}", e))
    })?;
    
    file.write_all(json.as_bytes()).map_err(|e| {
        HyperfanError::config(format!("Failed to write to temp file: {}", e))
    })?;
    
    file.sync_all().map_err(|e| {
        HyperfanError::config(format!("Failed to sync temp file: {}", e))
    })?;
    
    drop(file);
    
    // Atomic rename
    fs::rename(&temp_path, &path).map_err(|e| {
        HyperfanError::config(format!("Failed to rename temp file: {}", e))
    })?;
    
    Ok(())
}

/// Add a new temperature graph
pub fn add_temp_graph(graph: PersistedGraph) -> Result<()> {
    let mut graphs = load_temp_graphs()?;
    
    // Remove existing graph with same ID (update case)
    graphs.retain(|g| g.id != graph.id);
    graphs.push(graph);
    
    save_temp_graphs(&graphs)
}

/// Remove a temperature graph by ID
pub fn remove_temp_graph(graph_id: &str) -> Result<()> {
    let mut graphs = load_temp_graphs()?;
    graphs.retain(|g| g.id != graph_id);
    save_temp_graphs(&graphs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_settings() {
        let settings = AppSettings::default();
        assert_eq!(settings.general.poll_interval_ms, 100);
        assert_eq!(settings.display.temperature_unit, "celsius");
        assert!(settings.active_pairs.is_empty());
    }

    #[test]
    fn test_settings_serialization() {
        let settings = AppSettings::default();
        let json = serde_json::to_string(&settings)
            .expect("Default settings should serialize to JSON");
        let parsed: AppSettings = serde_json::from_str(&json)
            .expect("Serialized settings should deserialize back to AppSettings");
        assert_eq!(parsed.general.poll_interval_ms, settings.general.poll_interval_ms);
    }
}
