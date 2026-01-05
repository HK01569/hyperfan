//! System Tray Icon
//!
//! Provides a system tray icon using the StatusNotifierItem protocol (ksni).
//! Works with KDE, GNOME (with extension), and other desktop environments.

use ksni::{self, Tray, Handle};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;
use tracing::{debug, info, warn};

/// Global flags for communication between tray and GTK
static TRAY_RUNNING: AtomicBool = AtomicBool::new(false);
static SHOW_WINDOW_FLAG: AtomicBool = AtomicBool::new(false);
static QUIT_FLAG: AtomicBool = AtomicBool::new(false);
static STOP_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Global handle to control the tray service
static TRAY_HANDLE: Mutex<Option<Handle<HyperfanTray>>> = Mutex::new(None);

/// Check if user requested to show window (clears flag)
pub fn should_show_window() -> bool {
    SHOW_WINDOW_FLAG.swap(false, Ordering::SeqCst)
}

/// Check if user requested to quit
pub fn should_quit() -> bool {
    QUIT_FLAG.load(Ordering::SeqCst)
}

/// Check if tray is currently running
pub fn is_tray_running() -> bool {
    TRAY_RUNNING.load(Ordering::SeqCst)
}

/// Hyperfan tray icon implementation
struct HyperfanTray;

impl HyperfanTray {
    /// Build menu items for active fan/temp pairs
    /// PERFORMANCE: Uses cached settings and runtime sensor data to avoid blocking I/O
    fn build_status_items() -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        let mut items = Vec::new();
        
        // PERFORMANCE: Use cached settings (no disk I/O)
        let settings = hf_core::get_cached_settings();
        
        // PERFORMANCE: Try to get cached sensor data from runtime (non-blocking)
        let cached_sensors = crate::runtime::get_sensors();
        
        for pair in &settings.active_pairs {
            // Try cached temperature first, fallback to direct read only if needed
            let temp = cached_sensors.as_ref()
                .and_then(|data| {
                    data.temperatures.iter()
                        .find(|t| t.path == pair.temp_source_path)
                        .map(|t| t.temp_celsius as f64)
                })
                .or_else(|| {
                    // Fallback: direct read (should be rare)
                    std::fs::read_to_string(&pair.temp_source_path)
                        .ok()
                        .and_then(|s| s.trim().parse::<f64>().ok())
                        .map(|t| t / 1000.0)
                });
            
            // Try cached fan RPM first
            let rpm = cached_sensors.as_ref()
                .and_then(|data| {
                    // Find fan associated with this PWM
                    settings.pwm_fan_pairings.iter()
                        .find(|p| p.pwm_path == pair.fan_path)
                        .and_then(|p| p.fan_path.as_ref())
                        .and_then(|fan_path| {
                            data.fans.iter()
                                .find(|f| f.path == *fan_path)
                                .and_then(|f| f.rpm)
                        })
                })
                .or_else(|| {
                    // Fallback: direct read
                    settings.pwm_fan_pairings.iter()
                        .find(|p| p.pwm_path == pair.fan_path)
                        .and_then(|p| p.fan_path.as_ref())
                        .and_then(|fan_path| std::fs::read_to_string(fan_path).ok())
                        .and_then(|s| s.trim().parse::<u32>().ok())
                });
            
            // Build label with friendly name or pair name
            let name = settings.pwm_fan_pairings.iter()
                .find(|p| p.pwm_path == pair.fan_path)
                .and_then(|p| p.friendly_name.clone())
                .unwrap_or_else(|| pair.name.clone());
            
            let label = match (temp, rpm) {
                (Some(t), Some(r)) => format!("{}: {} / {} RPM", name, hf_core::display::format_temp(t as f32), r),
                (Some(t), None) => format!("{}: {}", name, hf_core::display::format_temp(t as f32)),
                (None, Some(r)) => format!("{}: {} RPM", name, r),
                (None, None) => format!("{}: --", name),
            };
            
            items.push(StandardItem {
                label: label.into(),
                enabled: false, // Display only, not clickable
                ..Default::default()
            }.into());
        }
        
        items
    }
}

impl Tray for HyperfanTray {
    fn icon_name(&self) -> String {
        "preferences-system-power-symbolic".to_string()
    }

    fn title(&self) -> String {
        "Hyperfan Fan Control".to_string()
    }

    fn id(&self) -> String {
        "hyperfan".to_string()
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        
        let mut items = vec![
            StandardItem {
                label: "Show Hyperfan".into(),
                activate: Box::new(|_| {
                    SHOW_WINDOW_FLAG.store(true, Ordering::SeqCst);
                }),
                ..Default::default()
            }
            .into(),
        ];
        
        // Add status items if any active pairs exist
        let status_items = Self::build_status_items();
        if !status_items.is_empty() {
            items.push(MenuItem::Separator);
            items.extend(status_items);
        }
        
        items.push(MenuItem::Separator);
        items.push(StandardItem {
            label: "Quit".into(),
            activate: Box::new(|_| {
                QUIT_FLAG.store(true, Ordering::SeqCst);
            }),
            ..Default::default()
        }.into());
        
        items
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        SHOW_WINDOW_FLAG.store(true, Ordering::SeqCst);
    }
}

/// Start the system tray icon in a background thread
pub fn start_tray() {
    if TRAY_RUNNING.load(Ordering::SeqCst) {
        debug!("Tray already running");
        return;
    }
    
    // Clear stop request flag
    STOP_REQUESTED.store(false, Ordering::SeqCst);
    TRAY_RUNNING.store(true, Ordering::SeqCst);

    thread::spawn(|| {
        let service = ksni::TrayService::new(HyperfanTray);
        let handle = service.handle();
        
        // Store handle globally for stop_tray() to use
        if let Ok(mut guard) = TRAY_HANDLE.lock() {
            *guard = Some(handle);
        }

        info!("System tray icon started");
        
        // Run the tray service (blocks until shutdown is called)
        let _ = service.run();
        
        // Clean up
        if let Ok(mut guard) = TRAY_HANDLE.lock() {
            *guard = None;
        }
        TRAY_RUNNING.store(false, Ordering::SeqCst);
        debug!("Tray service stopped");
    });
}

/// Stop the system tray icon
pub fn stop_tray() {
    if !TRAY_RUNNING.load(Ordering::SeqCst) {
        debug!("Tray not running");
        return;
    }
    
    STOP_REQUESTED.store(true, Ordering::SeqCst);
    
    // Use the handle to shutdown the tray service
    // Hold lock for entire operation to prevent race condition
    match TRAY_HANDLE.lock() {
        Ok(guard) => {
            if let Some(ref handle) = *guard {
                handle.shutdown();
                info!("Tray shutdown requested");
            } else {
                debug!("Tray handle not available for shutdown");
            }
        }
        Err(e) => {
            warn!("Failed to acquire tray handle lock for shutdown: {}", e);
        }
    }
}
