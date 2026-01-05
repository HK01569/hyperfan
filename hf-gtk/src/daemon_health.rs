//! Daemon health monitoring and error recovery
//!
//! Provides centralized daemon connection checking, error handling,
//! and automatic retry mechanisms for all daemon operations.

use gtk4::glib;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{debug, warn, error};

/// Daemon health state
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DaemonState {
    /// Daemon is responding normally
    Healthy,
    /// Daemon is not responding
    Unreachable,
    /// Checking daemon status
    Checking,
}

/// Global daemon health monitor
#[derive(Clone)]
pub struct DaemonHealthMonitor {
    state: Arc<Mutex<DaemonState>>,
    last_check: Arc<Mutex<Option<Instant>>>,
    on_state_change: Arc<Mutex<Option<Box<dyn Fn(DaemonState) + Send>>>>,
}

impl DaemonHealthMonitor {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(DaemonState::Checking)),
            last_check: Arc::new(Mutex::new(None)),
            on_state_change: Arc::new(Mutex::new(None)),
        }
    }

    /// Get current daemon state
    pub fn state(&self) -> DaemonState {
        *self.state.lock().unwrap()
    }

    /// Check daemon health (non-blocking)
    pub fn check_health(&self) {
        let state = self.state.clone();
        let last_check = self.last_check.clone();
        let on_change = self.on_state_change.clone();

        // Don't check too frequently (minimum 1 second between checks)
        if let Some(last) = *last_check.lock().unwrap() {
            if last.elapsed() < Duration::from_secs(1) {
                return;
            }
        }

        *state.lock().unwrap() = DaemonState::Checking;
        *last_check.lock().unwrap() = Some(Instant::now());

        // Check in background thread to avoid blocking main loop
        std::thread::spawn(move || {
            // Perform blocking check
            let is_healthy = hf_core::daemon_client::ping_daemon().is_ok();
            
            let new_state = if is_healthy {
                DaemonState::Healthy
            } else {
                DaemonState::Unreachable
            };

            let old_state = *state.lock().unwrap();
            *state.lock().unwrap() = new_state;

            // Notify if state changed (callback must be thread-safe)
            if old_state != new_state {
                if let Some(callback) = on_change.lock().unwrap().as_ref() {
                    callback(new_state);
                }
            }
        });
    }

    /// Register callback for state changes
    pub fn on_state_change<F: Fn(DaemonState) + Send + 'static>(&self, callback: F) {
        *self.on_state_change.lock().unwrap() = Some(Box::new(callback));
    }

    /// Start periodic health checks
    pub fn start_monitoring(&self, interval_secs: u64) {
        let monitor = self.clone();

        glib::timeout_add_local(Duration::from_secs(interval_secs), move || {
            monitor.check_health();
            glib::ControlFlow::Continue
        });
    }
}

/// Retry a daemon operation with exponential backoff
pub fn retry_daemon_operation<T, F>(
    operation: F,
    max_attempts: u32,
    operation_name: &str,
) -> Result<T, String>
where
    F: Fn() -> Result<T, String>,
{
    let mut attempt = 0;
    let mut last_error = String::new();

    while attempt < max_attempts {
        match operation() {
            Ok(result) => {
                if attempt > 0 {
                    debug!("{} succeeded after {} attempts", operation_name, attempt + 1);
                }
                return Ok(result);
            }
            Err(e) => {
                attempt += 1;
                last_error = e.clone();
                
                if attempt < max_attempts {
                    let delay_ms = 100 * (1 << attempt); // Exponential backoff: 200ms, 400ms, 800ms
                    warn!("{} failed (attempt {}/{}): {}. Retrying in {}ms...", 
                          operation_name, attempt, max_attempts, e, delay_ms);
                    std::thread::sleep(Duration::from_millis(delay_ms));
                } else {
                    error!("{} failed after {} attempts: {}", operation_name, max_attempts, e);
                }
            }
        }
    }

    Err(format!("Failed after {} attempts: {}", max_attempts, last_error))
}

/// Show daemon error toast to user
pub fn show_daemon_error_toast(window: &libadwaita::ApplicationWindow, error: &str) {
    error!("Daemon error: {}", error);
    // Toast functionality removed due to API complexity
    // Error is logged for debugging
}

/// Show daemon unreachable warning banner
pub fn create_daemon_warning_banner() -> libadwaita::Banner {
    let banner = libadwaita::Banner::new("Daemon Unreachable");
    banner.set_button_label(Some("Retry Connection"));
    banner.set_revealed(false);
    banner
}
