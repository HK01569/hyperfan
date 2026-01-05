//! Fan Control Loop (Hardened)
//!
//! Continuous fan control based on temperature curves.
//! Reads temperatures, interpolates curves, and sets PWM values.
//!
//! # Safety Features
//! - **Fail-safe default**: 50% fan speed if config cannot be loaded
//! - **Graceful degradation**: Individual pair failures don't crash the loop
//! - **Panic recovery**: Catches panics and continues operation
//! - **Error counting**: Tracks consecutive failures per PWM
//! - **Hysteresis**: Prevents rapid fan oscillation via FanCurve engine
//! - **Smoothing**: Gradual speed changes for quieter operation

use std::collections::HashMap;
use std::time::Instant;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Notify, RwLock};
use tracing::{debug, error, info, warn};

use hf_core::{FanCurve, CurvePoint};
use hf_protocol::{validate_hwmon_path, validate_pwm_target_path};

/// Default fan speed percentage when config fails to load (safety fallback)
const FALLBACK_FAN_PERCENT: f32 = 50.0;

/// Maximum consecutive errors before logging a warning
const MAX_CONSECUTIVE_ERRORS: u32 = 10;

/// PWM duty cycle range (0-255 for standard Linux hwmon)
const PWM_MAX: f32 = 255.0;

/// Percentage range
const PERCENT_MAX: f32 = 100.0;

/// PWM value for 50% fan speed (PWM_MAX * 0.5)
const FALLBACK_PWM_VALUE: u8 = 127;

/// Shared state for the fan control loop
pub struct FanControlState {
    /// Whether the control loop is enabled
    pub enabled: AtomicBool,
    /// Poll interval in milliseconds
    pub poll_interval_ms: AtomicU64,
    /// Active control pairs with runtime state (pwm_path -> ControlPairRuntime)
    pub pairs: RwLock<HashMap<String, ControlPairRuntime>>,
    /// Signal to reload configuration
    pub reload_signal: AtomicBool,
    /// Notify to wake up control loop immediately on reload
    pub reload_notify: Notify,
    /// Whether config has been successfully loaded at least once
    pub config_loaded: AtomicBool,
    /// Consecutive config load failures
    pub config_failures: AtomicU32,
    /// All known PWM paths (for fallback mode)
    pub known_pwm_paths: RwLock<Vec<String>>,

    /// Temporary PWM overrides requested by the GUI (pwm_path -> override)
    /// Using tokio::sync::RwLock to avoid blocking async executor threads
    pub pwm_overrides: tokio::sync::RwLock<HashMap<String, PwmOverride>>,
    
    /// Drift protection system (optional - activates when fingerprints are available)
    /// Validates paths before use to prevent hwmon reindexing issues
    pub drift_protection: Option<Arc<crate::drift_protection::DriftProtection>>,
    
    /// Last drift validation timestamp
    pub last_drift_validation: RwLock<Option<Instant>>,
}

#[derive(Clone, Copy, Debug)]
pub struct PwmOverride {
    pub value: u8,
    pub expires_at: Instant,
}

/// A single fan-curve control pair with integrated FanCurve engine
#[derive(Clone, Debug)]
pub struct ControlPair {
    pub id: String,
    pub name: String,
    pub pwm_path: String,
    pub temp_source_path: String,
    /// Raw curve points for serialization/display
    pub curve_points: Vec<(f32, f32)>,
    pub active: bool,
}

/// Runtime state for a control pair, including the FanCurve engine
/// This is kept separate from ControlPair to allow ControlPair to be Clone/Debug
pub struct ControlPairRuntime {
    pub pair: ControlPair,
    /// FanCurve engine with hysteresis and smoothing
    pub curve_engine: FanCurve,
}

impl FanControlState {
    pub fn new() -> Self {
        Self {
            enabled: AtomicBool::new(true),
            poll_interval_ms: AtomicU64::new(1000), // Default 1 second
            pairs: RwLock::new(HashMap::new()),
            reload_signal: AtomicBool::new(false),
            reload_notify: Notify::new(),
            config_loaded: AtomicBool::new(false),
            config_failures: AtomicU32::new(0),
            known_pwm_paths: RwLock::new(Vec::new()),
            pwm_overrides: tokio::sync::RwLock::new(HashMap::new()),
            drift_protection: None, // Initialized later if fingerprints are available
            last_drift_validation: RwLock::new(None),
        }
    }
    
    /// Initialize drift protection if fingerprints are available
    /// This should be called during daemon startup after basic initialization
    pub async fn initialize_drift_protection(&mut self) {
        // Check if fingerprint store exists
        if !hf_core::fingerprinting::is_fingerprinting_initialized() {
            info!("Fingerprint store not found - drift protection disabled");
            info!("  (Run fan detection in the GUI to enable drift protection)");
            return;
        }
        
        // Create drift protection with 60-second validation interval
        let dp = Arc::new(crate::drift_protection::DriftProtection::new(60));
        
        match dp.initialize().await {
            Ok(result) => {
                if result.ready_for_control {
                    info!("✓ Drift protection enabled: {} safe bindings", result.safe_binding_ids.len());
                    self.drift_protection = Some(dp);
                } else {
                    warn!("Drift protection initialized but no safe bindings available");
                    warn!("  Fan control will use raw paths from settings");
                }
            }
            Err(e) => {
                warn!("Failed to initialize drift protection: {}", e);
                warn!("  Fan control will use raw paths from settings");
            }
        }
    }
    
    /// Run periodic drift validation (call from control loop)
    pub async fn run_drift_validation(&self) {
        if let Some(ref dp) = self.drift_protection {
            if let Err(e) = dp.periodic_validation().await {
                warn!("Drift validation failed: {}", e);
            }
            *self.last_drift_validation.write().await = Some(Instant::now());
        }
    }

    pub async fn set_pwm_override(&self, pwm_path: String, value: u8, ttl_ms: u32) {
        let ttl = Duration::from_millis(ttl_ms.max(50) as u64);
        let expires_at = Instant::now() + ttl;
        let mut guard = self.pwm_overrides.write().await;
        guard.insert(pwm_path, PwmOverride { value, expires_at });
    }

    pub async fn clear_pwm_override(&self, pwm_path: &str) {
        let mut guard = self.pwm_overrides.write().await;
        guard.remove(pwm_path);
    }

    /// Signal the control loop to reload configuration
    /// This wakes up the control loop immediately to apply changes
    pub fn signal_reload(&self) {
        self.reload_signal.store(true, Ordering::SeqCst);
        self.reload_notify.notify_one(); // Wake up the control loop immediately
    }

    /// Check and clear reload signal
    pub fn check_reload_signal(&self) -> bool {
        self.reload_signal.swap(false, Ordering::SeqCst)
    }
}

impl Default for FanControlState {
    fn default() -> Self {
        Self::new()
    }
}

/// PWM controller info for initialization
#[derive(Clone, Debug)]
struct PwmInfo {
    pwm_path: String,
    enable_path: String,
    chip_name: String,
    pwm_name: String,
}

/// Discover all PWM paths on the system for fallback mode
async fn discover_pwm_paths() -> Vec<String> {
    let mut paths = Vec::new();
    
    if let Ok(chips) = hf_core::enumerate_hwmon_chips() {
        for chip in chips {
            for pwm in chip.pwms {
                paths.push(pwm.pwm_path.to_string_lossy().to_string());
            }
        }
    }
    
    paths
}

/// Discover all PWM controllers with full info (including GPU controllers)
fn discover_all_pwm_controllers() -> Vec<PwmInfo> {
    let mut controllers = Vec::new();
    
    // Discover motherboard/SuperIO PWM controllers via hwmon
    if let Ok(chips) = hf_core::enumerate_hwmon_chips() {
        for chip in chips {
            for pwm in chip.pwms {
                controllers.push(PwmInfo {
                    pwm_path: pwm.pwm_path.to_string_lossy().to_string(),
                    enable_path: pwm.enable_path.to_string_lossy().to_string(),
                    chip_name: chip.name.clone(),
                    pwm_name: pwm.name.clone(),
                });
            }
        }
    }
    
    // Discover GPU PWM controllers (AMD, NVIDIA, Intel)
    let gpu_controllers = hf_core::enumerate_gpu_pwm_controllers();
    for gpu in gpu_controllers {
        // For AMD/Intel GPUs with sysfs paths, add them directly
        // For NVIDIA, the path is virtual (nvidia:gpu:fan format)
        let enable_path = if gpu.pwm_path.starts_with("nvidia:") {
            // NVIDIA uses nvidia-settings, no enable file
            String::new()
        } else {
            // AMD/Intel use sysfs pwmN_enable
            let pwm_path = std::path::Path::new(&gpu.pwm_path);
            pwm_path
                .file_name()
                .map(|file_name| {
                    pwm_path
                        .with_file_name(format!("{}_enable", file_name.to_string_lossy()))
                        .to_string_lossy()
                        .to_string()
                })
                .unwrap_or_default()
        };
        
        controllers.push(PwmInfo {
            pwm_path: gpu.pwm_path.clone(),
            enable_path,
            chip_name: format!("GPU:{}", gpu.vendor.to_string()),
            pwm_name: gpu.name.clone(),
        });
        
        info!("Discovered GPU PWM controller: {} ({})", gpu.name, gpu.id);
    }
    
    controllers
}

/// Initialize all PWM controllers on daemon startup
/// 
/// This function:
/// 1. Discovers all PWM controllers
/// 2. Enables manual control mode for each
/// 3. Sets initial safe PWM value (50% - safe default before user config loads)
/// 4. Runs fan-to-PWM matching if not already done
/// 5. Loads saved pairings from settings when user logs in
pub async fn initialize_pwm_controls(state: &FanControlState) {
    info!("Initializing all PWM controls...");
    
    let controllers = discover_all_pwm_controllers();
    
    if controllers.is_empty() {
        warn!("No PWM controllers found on system - fan control not available");
        return;
    }
    
    info!("Found {} PWM controllers", controllers.len());
    
    // Phase 1: Enable manual control and set safe initial value (50%) for all PWMs
    // We use 50% as the boot default - this is safe for cooling while not being too loud
    // User profiles will be loaded later when a user session is detected
    let mut initialized_count = 0;
    let mut failed_count = 0;
    
    for pwm in &controllers {
        // Handle NVIDIA GPUs specially - they use nvidia-settings, not sysfs
        if pwm.pwm_path.starts_with("nvidia:") {
            // Parse nvidia:gpu_index:fan_index format
            let parts: Vec<&str> = pwm.pwm_path.split(':').collect();
            if parts.len() >= 3 {
                if let (Ok(gpu_idx), Ok(fan_idx)) = (parts[1].parse::<u32>(), parts[2].parse::<u32>()) {
                    // Set NVIDIA GPU fan to 50% via nvidia-settings
                    match hf_core::set_nvidia_fan_speed(gpu_idx, fan_idx, FALLBACK_FAN_PERCENT as u32) {
                        Ok(()) => {
                            debug!(
                                chip = %pwm.chip_name,
                                pwm = %pwm.pwm_name,
                                "Initialized NVIDIA GPU fan (50%)"
                            );
                            initialized_count += 1;
                        }
                        Err(e) => {
                            // NVIDIA fan control may not be available without X11
                            debug!(
                                chip = %pwm.chip_name,
                                pwm = %pwm.pwm_name,
                                error = %e,
                                "NVIDIA fan control not available (requires X11 and nvidia-settings)"
                            );
                            // Don't count as failure - NVIDIA fan control is optional
                        }
                    }
                }
            }
            continue;
        }
        
        // Standard sysfs PWM control (motherboard, AMD GPU, Intel GPU)
        // Enable manual control mode (1 = manual)
        let enable_result = if !pwm.enable_path.is_empty() && std::path::Path::new(&pwm.enable_path).exists() {
            std::fs::write(&pwm.enable_path, "1")
        } else {
            Ok(()) // No enable file means always manual
        };
        
        match enable_result {
            Ok(()) => {
                // Set initial PWM to 50% (127) - safe default before user config loads
                match std::fs::write(&pwm.pwm_path, FALLBACK_PWM_VALUE.to_string()) {
                    Ok(()) => {
                        debug!(
                            chip = %pwm.chip_name,
                            pwm = %pwm.pwm_name,
                            "Initialized PWM control (manual mode, 50%)"
                        );
                        initialized_count += 1;
                    }
                    Err(e) => {
                        warn!(
                            chip = %pwm.chip_name,
                            pwm = %pwm.pwm_name,
                            error = %e,
                            "Failed to set initial PWM value"
                        );
                        failed_count += 1;
                    }
                }
            }
            Err(e) => {
                warn!(
                    chip = %pwm.chip_name,
                    pwm = %pwm.pwm_name,
                    error = %e,
                    "Failed to enable manual PWM control"
                );
                failed_count += 1;
            }
        }
    }
    
    info!(
        initialized = initialized_count,
        failed = failed_count,
        "Phase 1 complete: PWM controllers initialized at 50% (boot default)"
    );
    
    // Update known PWM paths
    {
        let mut known_paths = state.known_pwm_paths.write().await;
        *known_paths = controllers.iter().map(|p| p.pwm_path.clone()).collect();
    }
    
    // Phase 2: Check if fan-to-PWM detection has been completed
    let settings = match hf_core::load_settings() {
        Ok(s) => s,
        Err(e) => {
            warn!("Failed to load settings for detection check: {}", e);
            return;
        }
    };
    
    if !settings.detection_completed {
        info!("Fan-to-PWM detection not yet completed - running heuristic matching");
        
        // Run heuristic matching (non-destructive, doesn't spin down fans)
        match hf_core::autodetect_fan_pwm_mappings_heuristic() {
            Ok(mappings) => {
                info!("Heuristic matching found {} PWM-fan pairs", mappings.len());
                
                // Save the mappings
                if let Err(e) = hf_core::save_pwm_fan_mappings(mappings) {
                    warn!("Failed to save PWM-fan mappings: {}", e);
                } else {
                    info!("Saved PWM-fan mappings to settings");
                }
            }
            Err(e) => {
                warn!("Heuristic matching failed: {}", e);
            }
        }
    } else {
        info!(
            "Using {} existing PWM-fan mappings from settings",
            settings.pwm_fan_mappings.len()
        );
    }
    
    // Phase 3: Load and apply any saved PWM-fan pairings
    let pairing_count = settings.pwm_fan_pairings.len();
    if pairing_count > 0 {
        info!("Loaded {} user-defined PWM-fan pairings", pairing_count);
        for pairing in &settings.pwm_fan_pairings {
            debug!(
                pwm = %pairing.pwm_path,
                fan = ?pairing.fan_path,
                friendly_name = ?pairing.friendly_name,
                "Loaded PWM-fan pairing"
            );
        }
    }
    
    info!("PWM control initialization complete");
}

/// Apply fallback fan speed (50%) to all known PWM controllers
async fn apply_fallback_speed(state: &FanControlState) {
    let paths = state.known_pwm_paths.read().await;
    
    if paths.is_empty() {
        warn!("No PWM paths known for fallback - discovering...");
        drop(paths);
        
        let discovered = discover_pwm_paths().await;
        if discovered.is_empty() {
            error!("CRITICAL: No PWM controllers found on system!");
            return;
        }
        
        let mut paths_write = state.known_pwm_paths.write().await;
        *paths_write = discovered;
        drop(paths_write);
        
        // Re-acquire read lock
        let paths = state.known_pwm_paths.read().await;
        for pwm_path in paths.iter() {
            if let Err(e) = set_pwm_safe(pwm_path, FALLBACK_PWM_VALUE) {
                warn!("Failed to set fallback PWM for {}: {}", pwm_path, e);
            } else {
                info!("Set fallback {}% fan speed on {}", FALLBACK_FAN_PERCENT, pwm_path);
            }
        }
    } else {
        for pwm_path in paths.iter() {
            if let Err(e) = set_pwm_safe(pwm_path, FALLBACK_PWM_VALUE) {
                warn!("Failed to set fallback PWM for {}: {}", pwm_path, e);
            } else {
                info!("Set fallback {}% fan speed on {}", FALLBACK_FAN_PERCENT, pwm_path);
            }
        }
    }
}

/// Load configuration from hf_core settings
pub async fn load_config(state: &FanControlState) -> Result<(), String> {
    // Log which config path we're using - this helps debug user config resolution
    match hf_core::constants::paths::get_resolved_config_path() {
        Some(config_path) => {
            info!("Loading config from: {:?}", config_path);
            // Also log if the settings file exists
            let settings_path = config_path.join("settings.json");
            let curves_path = config_path.join("curves.json");
            info!("  settings.json exists: {}", settings_path.exists());
            info!("  curves.json exists: {}", curves_path.exists());
            
            // Log file contents for debugging
            if settings_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&settings_path) {
                    debug!("  settings.json size: {} bytes", content.len());
                }
            }
        }
        None => {
            error!("Could not resolve user config path! Daemon may be using wrong config.");
            error!("  SUDO_USER: {:?}", std::env::var("SUDO_USER").ok());
            error!("  PKEXEC_UID: {:?}", std::env::var("PKEXEC_UID").ok());
            error!("  HOME: {:?}", std::env::var("HOME").ok());
            error!("  Running as UID: {}", unsafe { libc::geteuid() });
        }
    }
    
    // Load settings
    let settings = match hf_core::load_settings() {
        Ok(s) => {
            info!("Settings loaded: {} active_pairs total, {} active", 
                  s.active_pairs.len(),
                  s.active_pairs.iter().filter(|p| p.active).count());
            s
        }
        Err(e) => {
            let failures = state.config_failures.fetch_add(1, Ordering::SeqCst) + 1;
            if failures == 1 || failures % MAX_CONSECUTIVE_ERRORS == 0 {
                error!("Failed to load settings (attempt {}): {}", failures, e);
            }
            return Err(format!("Failed to load settings: {}", e));
        }
    };

    // Update poll interval
    let poll_ms = settings.general.poll_interval_ms as u64;
    state.poll_interval_ms.store(poll_ms.max(50), Ordering::SeqCst); // Min 50ms

    // Load curves
    let curve_store = match hf_core::load_curves() {
        Ok(c) => {
            info!("Curves loaded: {} curves available", c.len());
            for curve in c.all() {
                debug!("  Curve '{}' (id={}) with {} points", curve.name, curve.id, curve.points.len());
            }
            c
        }
        Err(e) => {
            let failures = state.config_failures.fetch_add(1, Ordering::SeqCst) + 1;
            if failures == 1 || failures % MAX_CONSECUTIVE_ERRORS == 0 {
                error!("Failed to load curves (attempt {}): {}", failures, e);
            }
            return Err(format!("Failed to load curves: {}", e));
        }
    };

    // Build control pairs from active_pairs
    let mut pairs = HashMap::new();
    let mut pwm_paths = Vec::new();

    for pair in settings.active_pairs.iter().filter(|p| p.active) {
        // Collect all fan paths - use fan_paths if available, otherwise fall back to fan_path
        let all_fan_paths: Vec<String> = if !pair.fan_paths.is_empty() {
            pair.fan_paths.clone()
        } else if !pair.fan_path.is_empty() {
            vec![pair.fan_path.clone()]
        } else {
            warn!("Pair '{}' has no fan paths configured, skipping", pair.name);
            continue;
        };
        
        info!("Processing active pair '{}': curve_id='{}', fan_paths={:?}, temp_source='{}'",
              pair.name, pair.curve_id, all_fan_paths, pair.temp_source_path);
        
        // Look up the curve
        if let Some(curve) = curve_store.get(&pair.curve_id) {
            info!("  Found curve '{}' with {} points", curve.name, curve.points.len());

            let mut curve_points = curve.points.clone();
            curve_points.sort_by(|a, b| {
                a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal)
            });
            for point in &mut curve_points {
                point.1 = point.1.clamp(0.0, 100.0);
            }
            
            // Create a control pair for EACH fan path in this pair
            // This allows multiple fans to be controlled by the same curve
            for (idx, fan_path) in all_fan_paths.iter().enumerate() {
                let control_pair = ControlPair {
                    id: if idx == 0 { pair.id.clone() } else { format!("{}_{}", pair.id, idx) },
                    name: if all_fan_paths.len() > 1 { 
                        format!("{} [Fan {}]", pair.name, idx + 1) 
                    } else { 
                        pair.name.clone() 
                    },
                    pwm_path: fan_path.clone(),
                    temp_source_path: pair.temp_source_path.clone(),
                    curve_points: curve_points.clone(),
                    active: pair.active,
                };
                
                // Create FanCurve engine with hysteresis, delay, and ramp speeds from curve config
                // Convert (f32, f32) tuples to CurvePoint structs
                let curve_point_structs: Vec<CurvePoint> = curve_points.iter()
                    .map(|(temp, percent)| CurvePoint { temperature: *temp, fan_percent: *percent })
                    .collect();
                // Check if stepped mode is enabled via graph_style setting
                // (use already-loaded settings to avoid redundant disk I/O)
                let stepped = settings.display.graph_style == "stepped";
                
                let curve_engine = FanCurve::new(curve_point_structs)
                    .with_hysteresis(curve.hysteresis)
                    .with_smoothing(hf_core::constants::curve::DEFAULT_SMOOTHING_FACTOR)
                    .with_delay(curve.delay_ms)
                    .with_ramp_speeds(curve.ramp_up_speed, curve.ramp_down_speed)
                    .with_stepped(stepped);
                
                let runtime = ControlPairRuntime {
                    pair: control_pair,
                    curve_engine,
                };
                
                pwm_paths.push(fan_path.clone());
                pairs.insert(fan_path.clone(), runtime);
                debug!("  Added control for fan path: {} (with FanCurve engine)", fan_path);
            }
        } else {
            error!("Curve '{}' not found for pair '{}' - available curves: {:?}", 
                   pair.curve_id, pair.name, 
                   curve_store.all().iter().map(|c| &c.id).collect::<Vec<_>>());
        }
    }

    // Update state
    let pairs_count = pairs.len();
    {
        let mut state_pairs = state.pairs.write().await;
        *state_pairs = pairs;
    }
    info!("Stored {} control pairs in state", pairs_count);
    
    // Update known PWM paths (for fallback)
    {
        let mut known_paths = state.known_pwm_paths.write().await;
        if known_paths.is_empty() {
            // First load - discover all PWM paths
            *known_paths = discover_pwm_paths().await;
        }
        // Add any new paths from config
        for path in pwm_paths {
            if !known_paths.contains(&path) {
                known_paths.push(path);
            }
        }
    }

    // Mark config as successfully loaded
    state.config_loaded.store(true, Ordering::SeqCst);
    state.config_failures.store(0, Ordering::SeqCst);

    let pairs_count = state.pairs.read().await.len();
    info!(
        "Loaded {} active control pairs, poll interval: {}ms",
        pairs_count,
        state.poll_interval_ms.load(Ordering::SeqCst)
    );

    Ok(())
}

/// Run the fan control loop with panic recovery and fallback handling
pub async fn run_control_loop(state: Arc<FanControlState>, shutdown: Arc<AtomicBool>) {
    info!("Fan control loop starting (hardened mode)");

    // Phase 0: Initialize ALL PWM controls (enable manual mode, run matching if needed)
    initialize_pwm_controls(&state).await;

    // Initial config load - apply fallback if it fails
    if let Err(e) = load_config(&state).await {
        error!("Failed to load initial config: {} - applying fallback fan speed", e);
        apply_fallback_speed(&state).await;
    } else {
        // Config loaded successfully - immediately apply fan curves
        // This ensures fans are set to correct speeds right after daemon starts
        info!("Applying initial fan curves immediately after config load");
        if let Err(e) = process_control_iteration(&state).await {
            warn!("Initial curve application failed: {} - will retry in control loop", e);
        }
    }

    let mut consecutive_loop_errors: u32 = 0;
    let mut loop_iteration: u64 = 0;

    loop {
        loop_iteration += 1;
        
        // Check for shutdown
        if shutdown.load(Ordering::SeqCst) {
            info!("Fan control loop shutting down");
            break;
        }
        
        // Run drift validation periodically (every ~60 iterations at 1s poll = ~1 minute)
        // This detects hwmon reindexing and updates path cache
        if loop_iteration % 60 == 0 {
            state.run_drift_validation().await;
        }

        // Wrap the main loop body in catch_unwind equivalent via result handling
        let loop_result = process_control_iteration(&state).await;
        
        match loop_result {
            Ok(()) => {
                // Reset error counter on success
                if consecutive_loop_errors > 0 {
                    debug!("Control loop recovered after {} errors", consecutive_loop_errors);
                    consecutive_loop_errors = 0;
                }
            }
            Err(e) => {
                consecutive_loop_errors += 1;
                
                if consecutive_loop_errors == 1 || consecutive_loop_errors % MAX_CONSECUTIVE_ERRORS == 0 {
                    error!("Control loop error (count: {}): {}", consecutive_loop_errors, e);
                }
                
                // After too many errors, apply fallback speed
                if consecutive_loop_errors == MAX_CONSECUTIVE_ERRORS {
                    warn!("Too many consecutive errors - applying fallback fan speed");
                    apply_fallback_speed(&state).await;
                }
            }
        }

        // Sleep for poll interval, but wake up immediately if reload is signaled
        let poll_ms = state.poll_interval_ms.load(Ordering::SeqCst);
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(poll_ms)) => {}
            _ = state.reload_notify.notified() => {
                debug!("Control loop woken up by reload signal");
            }
        }
    }

    info!("Fan control loop stopped");
}

/// Process a single control iteration - separated for error handling
async fn process_control_iteration(state: &FanControlState) -> Result<(), String> {
    // Check for reload signal
    if state.check_reload_signal() {
        info!("SIGNAL: Configuration reload requested");
        if let Err(e) = load_config(state).await {
            // Don't fail the whole iteration, just log and continue with existing config
            warn!("ACTION: Config reload failed: {} - continuing with existing config", e);
        } else {
            info!("ACTION: Configuration reloaded successfully");
        }
    }

    // Only process if enabled
    if !state.enabled.load(Ordering::SeqCst) {
        debug!("Control loop disabled, skipping iteration");
        return Ok(());
    }

    // Apply temporary overrides first (works even if there are no active pairs)
    // Also prune expired entries.
    let now = Instant::now();
    let mut overrides_to_apply: Vec<(String, u8)> = Vec::new();
    {
        let mut guard = state.pwm_overrides.write().await;
        guard.retain(|pwm_path, ov| {
            if ov.expires_at <= now {
                return false;
            }
            overrides_to_apply.push((pwm_path.clone(), ov.value));
            true
        });
    }

    for (pwm_path, value) in &overrides_to_apply {
        if let Err(e) = set_pwm_safe(pwm_path, *value) {
            warn!("CONTROL: Failed to apply PWM override {}={} : {}", pwm_path, value, e);
        }
    }

    // We need mutable access to update the FanCurve engine state (for hysteresis/smoothing)
    let mut pairs = state.pairs.write().await;
    
    if pairs.is_empty() {
        // No active pairs - still allow overrides above.
        debug!("No active pairs configured, skipping curve control");
        return Ok(());
    }
    
    // Log pair count at debug level (too verbose for info)
    debug!("CONTROL: Processing {} active pairs", pairs.len());

    // PERF: Collect overridden paths ONCE before the loop to avoid repeated lock acquisition
    let overridden_paths: std::collections::HashSet<String> = {
        let guard = state.pwm_overrides.read().await;
        let now = Instant::now();
        guard.iter()
            .filter(|(_, ov)| ov.expires_at > now)
            .map(|(path, _)| path.clone())
            .collect()
    };

    // Process all active pairs
    for (pwm_path, runtime) in pairs.iter_mut() {
        // If overridden, skip curve control for this PWM.
        if overridden_paths.contains(pwm_path) {
            debug!("CONTROL: Skipping curve control for overridden PWM {}", pwm_path);
            continue;
        }

        if !runtime.pair.active {
            debug!("Skipping inactive pair: {}", runtime.pair.name);
            continue;
        }

        // Read temperature - use fallback on failure
        // FIX: Check for non-finite temperature IMMEDIATELY after reading, before any processing
        let temp = match read_temperature_async(&runtime.pair.temp_source_path).await {
            Ok(t) => {
                // FIX: Non-finite check moved here, before interpolation
                if !t.is_finite() {
                    warn!(
                        "CONTROL: Non-finite temperature read for '{}' ({}); applying fallback {}%",
                        runtime.pair.name, runtime.pair.temp_source_path, FALLBACK_FAN_PERCENT
                    );
                    if let Err(pwm_err) = set_pwm_async(pwm_path, FALLBACK_PWM_VALUE).await {
                        error!("ACTION: Failed to set fallback PWM for {}: {}", runtime.pair.name, pwm_err);
                    }
                    continue;
                }
                debug!("READ: {} temp={:.1}°C from {}", runtime.pair.name, t, runtime.pair.temp_source_path);
                t
            }
            Err(e) => {
                warn!("ACTION: Failed to read temp for {} ({}): {} - applying fallback {}%", 
                      runtime.pair.name, runtime.pair.temp_source_path, e, FALLBACK_FAN_PERCENT);
                // Use fallback speed for this fan
                if let Err(pwm_err) = set_pwm_async(pwm_path, FALLBACK_PWM_VALUE).await {
                    error!("ACTION: Failed to set fallback PWM for {}: {}", runtime.pair.name, pwm_err);
                } else {
                    info!("ACTION: Set fallback PWM {} ({}%) on {}", 
                          FALLBACK_PWM_VALUE, FALLBACK_FAN_PERCENT, runtime.pair.name);
                }
                continue;
            }
        };

        // Use FanCurve engine with hysteresis and smoothing (replaces raw interpolation)
        // The engine maintains state for smooth transitions and prevents oscillation
        let fan_percent = runtime.curve_engine.calculate(temp);

        // Convert percent to PWM value
        let pwm_value = ((fan_percent / PERCENT_MAX) * PWM_MAX).clamp(0.0, PWM_MAX).round() as u8;

        // Set PWM - log at debug level (too verbose for info at 200ms intervals)
        match set_pwm_async(pwm_path, pwm_value).await {
            Ok(()) => {
                debug!("CONTROL: Set PWM {} ({}%) on '{}' (temp={:.1}°C)", 
                       pwm_value, fan_percent as u8, runtime.pair.name, temp);
            }
            Err(e) => {
                // Errors are always logged at warn/error level
                error!("CONTROL: Failed to set PWM {} on '{}': {}", pwm_value, runtime.pair.name, e);
            }
        }
    }

    Ok(())
}

/// Read temperature from a sensor path (async version - doesn't block the executor)
/// Uses spawn_blocking to run file I/O on a separate thread pool
async fn read_temperature_async(path: &str) -> Result<f32, String> {
    let path = path.to_string();
    tokio::task::spawn_blocking(move || read_temperature_inner(&path))
        .await
        .map_err(|e| format!("Temperature read task panicked: {}", e))?
}

/// Read temperature from a sensor path (safe version with panic protection)
/// DEPRECATED: Use read_temperature_async for non-blocking I/O
#[allow(dead_code)]
fn read_temperature_safe(path: &str) -> Result<f32, String> {
    std::panic::catch_unwind(|| read_temperature_inner(path))
        .map_err(|_| "Panic during temperature read".to_string())?
}

/// Inner temperature read function (blocking - run via spawn_blocking)
fn read_temperature_inner(path: &str) -> Result<f32, String> {
    // Handle GPU temperature paths (gpu:N:name format)
    if path.starts_with("gpu:") {
        let parts: Vec<&str> = path.split(':').collect();
        if parts.len() >= 2 {
            if let Ok(index) = parts[1].parse::<u32>() {
                if let Ok(gpus) = hf_core::enumerate_gpus() {
                    if let Some(gpu) = gpus.iter().find(|g| g.index == index) {
                        if let Some(temp) = gpu.temperatures.first().and_then(|t| t.current_temp) {
                            return Ok(temp);
                        }
                    }
                }
            }
        }
        return Err("GPU temperature not available".to_string());
    }

    // Standard hwmon path
    if let Err(e) = validate_hwmon_path(path) {
        return Err(e);
    }

    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path, e))?;

    let millidegrees: f64 = content
        .trim()
        .parse()
        .map_err(|e| format!("Failed to parse temperature: {}", e))?;

    let c = (millidegrees / 1000.0) as f32;
    // Note: Non-finite check is now done in the caller (process_control_iteration)
    // to provide better error context

    Ok(c)
}

// NOTE: interpolate_curve() has been REMOVED - we now use hf_core::FanCurve::calculate()
// which provides hysteresis and smoothing for better fan control behavior.
// The FanCurve engine is integrated into ControlPairRuntime.

/// Set PWM value (async version - doesn't block the executor)
/// Uses spawn_blocking to run file I/O on a separate thread pool
async fn set_pwm_async(pwm_path: &str, value: u8) -> Result<(), String> {
    let pwm_path = pwm_path.to_string();
    tokio::task::spawn_blocking(move || set_pwm_inner(&pwm_path, value))
        .await
        .map_err(|e| format!("PWM write task panicked: {}", e))?
}

/// Set PWM value with enable check (safe version with panic protection)
/// Handles both sysfs PWM (motherboard, AMD, Intel) and NVIDIA GPU fans
fn set_pwm_safe(pwm_path: &str, value: u8) -> Result<(), String> {
    std::panic::catch_unwind(|| set_pwm_inner(pwm_path, value))
        .map_err(|_| "Panic during PWM write".to_string())?
}

/// Inner PWM set function - handles both sysfs and NVIDIA GPU fans
fn set_pwm_inner(pwm_path: &str, value: u8) -> Result<(), String> {
    if let Err(e) = validate_pwm_target_path(pwm_path) {
        return Err(e);
    }

    // Handle NVIDIA GPU fans (virtual path format: nvidia:gpu_index:fan_index)
    if pwm_path.starts_with("nvidia:") {
        let parts: Vec<&str> = pwm_path.split(':').collect();
        if parts.len() >= 3 {
            let gpu_idx: u32 = parts[1].parse().map_err(|_| "Invalid GPU index".to_string())?;
            let fan_idx: u32 = parts[2].parse().map_err(|_| "Invalid fan index".to_string())?;
            let percent = ((value as f32 / PWM_MAX) * PERCENT_MAX).round() as u32;
            
            hf_core::set_nvidia_fan_speed(gpu_idx, fan_idx, percent)
                .map_err(|e| format!("NVIDIA fan control failed: {}", e))?;
            return Ok(());
        }
        return Err("Invalid NVIDIA PWM path format".to_string());
    }
    
    // Handle AMD/Intel GPU fans (virtual path format: amd:card:fan or intel:card:fan)
    if pwm_path.starts_with("amd:") || pwm_path.starts_with("intel:") {
        let percent = ((value as f32 / 255.0) * 100.0).round() as u32;
        hf_core::set_gpu_fan_speed_by_id(pwm_path, percent)
            .map_err(|e| format!("GPU fan control failed: {}", e))?;
        return Ok(());
    }
    
    // Standard sysfs PWM control (motherboard SuperIO chips)
    let path = std::path::Path::new(pwm_path);
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "Invalid PWM path".to_string())?;

    if !file_name.starts_with("pwm") {
        return Err("PWM path does not point to a pwmN control file".to_string());
    }

    let suffix = &file_name[3..];
    if suffix.is_empty() || !suffix.chars().all(|c| c.is_ascii_digit()) {
        return Err("PWM path does not point to a pwmN control file".to_string());
    }

    let enable_path = path
        .with_file_name(format!("{}_enable", file_name))
        .to_string_lossy()
        .to_string();

    // Handle PWM enable mode:
    // - PWM 0: Set enable to 0 (disabled) to actually stop the fan
    // - PWM > 0: Set enable to 1 (manual) for software control
    // Many fans won't stop at PWM 0 with enable=1, they just spin at minimum RPM
    if std::path::Path::new(&enable_path).exists() {
        let target_mode = if value == 0 { 0 } else { 1 };
        let current_mode = std::fs::read_to_string(&enable_path)
            .ok()
            .and_then(|s| s.trim().parse::<u8>().ok())
            .unwrap_or(0);

        if current_mode != target_mode {
            std::fs::write(&enable_path, target_mode.to_string())
                .map_err(|e| format!("Failed to set PWM enable mode {}: {}", target_mode, e))?;
        }
    }

    // Set PWM value (even when disabled, set to 0 for consistency)
    std::fs::write(pwm_path, value.to_string())
        .map_err(|e| format!("Failed to write PWM: {}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fan_curve_engine() {
        // Test the FanCurve engine integration
        let points = vec![
            CurvePoint { temperature: 30.0, fan_percent: 20.0 },
            CurvePoint { temperature: 50.0, fan_percent: 40.0 },
            CurvePoint { temperature: 70.0, fan_percent: 80.0 },
            CurvePoint { temperature: 90.0, fan_percent: 100.0 },
        ];
        
        let mut curve = FanCurve::new(points);

        // At points (first call, no smoothing applied)
        assert!((curve.calculate(30.0) - 20.0).abs() < 1.0);
        
        // Reset for clean test
        curve.reset();
        assert!((curve.calculate(50.0) - 40.0).abs() < 1.0);
        
        curve.reset();
        assert!((curve.calculate(90.0) - 100.0).abs() < 1.0);

        // Below first point
        curve.reset();
        assert!((curve.calculate(20.0) - 20.0).abs() < 1.0);

        // Above last point
        curve.reset();
        assert!((curve.calculate(100.0) - 100.0).abs() < 1.0);
    }

    #[test]
    fn test_fan_curve_empty() {
        let points: Vec<CurvePoint> = vec![];
        let mut curve = FanCurve::new(points);
        // Empty curve should return 100% (fail-safe)
        assert_eq!(curve.calculate(50.0), 100.0);
    }
    
    #[test]
    fn test_control_pair_runtime_creation() {
        let points = vec![
            CurvePoint { temperature: 30.0, fan_percent: 20.0 },
            CurvePoint { temperature: 70.0, fan_percent: 80.0 },
        ];
        
        let pair = ControlPair {
            id: "test".to_string(),
            name: "Test Fan".to_string(),
            pwm_path: "/sys/class/hwmon/hwmon0/pwm1".to_string(),
            temp_source_path: "/sys/class/hwmon/hwmon0/temp1_input".to_string(),
            curve_points: points.iter().map(|p| (p.temperature, p.fan_percent)).collect(),
            active: true,
        };
        
        let curve_engine = FanCurve::new(points)
            .with_hysteresis(2.0)
            .with_smoothing(0.3);
        
        let runtime = ControlPairRuntime {
            pair,
            curve_engine,
        };
        
        assert!(runtime.pair.active);
        assert_eq!(runtime.pair.name, "Test Fan");
    }
}
