//! Tokio Worker Runtime
//!
//! Manages background workers for sensor reads, logic processing, and UI updates.
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────────┐     mpsc (16)    ┌──────────────────┐
//! │  Sensor Worker   │ ───────────────> │   Logic Worker   │
//! │  (1 thread)      │                  │   (1 thread)     │
//! │  100ms interval  │                  │   on-demand      │
//! └────────┬─────────┘                  └────────┬─────────┘
//!          │ broadcast                           │ broadcast
//!          v                                     v
//! ┌─────────────────────────────────────────────────────────┐
//! │                   UI Workers (2 threads)                │
//! │              Format/prepare data for GTK                │
//! └─────────────────────────────────────────────────────────┘
//!          │ Arc<RwLock<SensorData>> (shared state)
//!          v
//! ┌─────────────────────────────────────────────────────────┐
//! │              GTK Main Thread (glib::timeout)            │
//! │              Reads shared state, updates widgets        │
//! └─────────────────────────────────────────────────────────┘
//! ```

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::sync::{broadcast, mpsc, RwLock};

// ============================================================================
// Message Types
// ============================================================================

/// Sensor data from the read worker
#[derive(Clone, Debug, Default)]
pub struct SensorData {
    pub timestamp_ms: u64,
    pub temperatures: Vec<TempReading>,
    pub fans: Vec<FanReading>,
    pub gpus: Vec<GpuReading>,
}

#[derive(Clone, Debug)]
pub struct TempReading {
    pub path: String,
    pub label: String,
    pub chip_name: String,
    pub temp_celsius: f32,
}

#[derive(Clone, Debug)]
pub struct FanReading {
    pub path: String,
    pub label: String,
    pub chip_name: String,
    pub rpm: Option<u32>,
    pub percent: Option<f32>,
    pub pwm_value: Option<u8>,
}

#[derive(Clone, Debug)]
pub struct GpuReading {
    pub index: u32,
    pub name: String,
    pub vendor: String,
    pub temp: Option<f32>,
    pub temperatures: std::collections::HashMap<String, f32>, // temp_name -> value
    pub fan_percent: Option<u32>,
    pub fan_rpm: Option<u32>,
    pub power_watts: Option<f32>,
    pub utilization: Option<u32>,
    pub vram_used_mb: Option<u32>,
    pub vram_total_mb: Option<u32>,
}

/// Logic worker output - computed fan speeds
#[derive(Clone, Debug, Default)]
pub struct LogicOutput {
    pub timestamp_ms: u64,
    pub fan_targets: Vec<FanTarget>,
}

#[derive(Clone, Debug)]
pub struct FanTarget {
    pub pwm_path: String,
    pub target_percent: f32,
    pub current_temp: f32,
    pub curve_name: String,
}

/// UI update notification
#[derive(Clone, Debug)]
pub enum UiUpdate {
    SensorData(u64),  // Just timestamp - actual data in shared state
    LogicOutput(u64),
}

// ============================================================================
// Worker Runtime
// ============================================================================

/// Shared state between workers - lock-free where possible
pub struct WorkerState {
    /// Latest sensor data (RwLock for complex data)
    pub sensors: RwLock<SensorData>,
    /// Latest logic output
    pub logic: RwLock<LogicOutput>,
    /// Atomic running flag for fast shutdown checks
    pub running: AtomicBool,
    /// Sensor read count for diagnostics
    pub sensor_reads: AtomicU64,
    /// Logic cycles count
    pub logic_cycles: AtomicU64,
}

impl Default for WorkerState {
    fn default() -> Self {
        Self {
            sensors: RwLock::new(SensorData::default()),
            logic: RwLock::new(LogicOutput::default()),
            running: AtomicBool::new(true),
            sensor_reads: AtomicU64::new(0),
            logic_cycles: AtomicU64::new(0),
        }
    }
}

/// Main worker runtime manager
pub struct WorkerRuntime {
    runtime: Runtime,
    state: Arc<WorkerState>,
    ui_tx: broadcast::Sender<UiUpdate>,
    _shutdown_tx: mpsc::Sender<()>,
}

impl WorkerRuntime {
    /// Create and start the worker runtime
    pub fn new() -> Self {
        // PERF: Use 2 worker threads - we only have 2 real workers (sensor + logic)
        // More threads = more context switching overhead
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("hyperfan-worker")
            .enable_all()
            .build()
            .expect("FATAL: Failed to create Tokio runtime. This is a critical error that prevents the application from starting. Please check system resources and try again.");

        let state = Arc::new(WorkerState::default());
        let (ui_tx, _) = broadcast::channel::<UiUpdate>(64);
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>(1);

        let this = Self {
            runtime,
            state,
            ui_tx,
            _shutdown_tx: shutdown_tx,
        };

        this.spawn_workers(shutdown_rx);
        
        tracing::info!("Worker runtime initialized with 2 threads");
        this
    }

    /// Spawn all worker tasks
    fn spawn_workers(&self, mut shutdown_rx: mpsc::Receiver<()>) {
        let state = self.state.clone();
        let ui_tx = self.ui_tx.clone();

        // Channel from sensor worker to logic worker (bounded)
        let (sensor_tx, sensor_rx) = mpsc::channel::<SensorData>(16);

        // ================================================================
        // WORKER 1: Sensor Read Worker
        // ================================================================
        let state_sensor = state.clone();
        let ui_tx_sensor = ui_tx.clone();

        // Get user-configured poll interval from settings
        let poll_interval_ms = hf_core::get_cached_settings().general.poll_interval_ms as u64;
        let poll_interval_ms = poll_interval_ms.max(50); // Minimum 50ms for safety
        
        self.runtime.spawn(async move {
            tracing::info!("[Sensor Worker] Started - polling every {}ms", poll_interval_ms);
            let mut interval = tokio::time::interval(Duration::from_millis(poll_interval_ms));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                interval.tick().await;

                // Fast atomic check for shutdown
                if !state_sensor.running.load(Ordering::Relaxed) {
                    break;
                }

                // Run blocking I/O in dedicated thread pool
                let data = tokio::task::spawn_blocking(read_all_sensors_blocking)
                    .await
                    .unwrap_or_else(|e| {
                        tracing::error!("[Sensor Worker] Task panic: {}", e);
                        SensorData::default()
                    });

                let timestamp = data.timestamp_ms;

                // Update shared state
                *state_sensor.sensors.write().await = data.clone();
                state_sensor.sensor_reads.fetch_add(1, Ordering::Relaxed);

                // Send to logic worker (non-blocking, drop if full)
                let _ = sensor_tx.try_send(data);

                // Notify UI workers
                let _ = ui_tx_sensor.send(UiUpdate::SensorData(timestamp));
            }

            tracing::info!("[Sensor Worker] Stopped");
        });

        // ================================================================
        // WORKER 2: Logic Worker
        // ================================================================
        let state_logic = state.clone();
        let ui_tx_logic = ui_tx.clone();

        self.runtime.spawn(async move {
            tracing::info!("[Logic Worker] Started - processing sensor data");
            let mut sensor_rx = sensor_rx;

            while let Some(sensor_data) = sensor_rx.recv().await {
                if !state_logic.running.load(Ordering::Relaxed) {
                    break;
                }

                // Process logic (curve calculations, etc.)
                let output = process_logic(&sensor_data);
                let timestamp = output.timestamp_ms;

                // Update shared state
                *state_logic.logic.write().await = output;
                state_logic.logic_cycles.fetch_add(1, Ordering::Relaxed);

                // Notify UI workers
                let _ = ui_tx_logic.send(UiUpdate::LogicOutput(timestamp));
            }

            tracing::info!("[Logic Worker] Stopped");
        });

        // ================================================================
        // WORKERS 3-4: UI Preparation Workers
        // ================================================================
        for worker_id in 0..2 {
            let mut ui_rx = ui_tx.subscribe();
            let state_ui = state.clone();

            self.runtime.spawn(async move {
                tracing::debug!("[UI Worker {}] Started", worker_id);

                loop {
                    match ui_rx.recv().await {
                        Ok(update) => {
                            if !state_ui.running.load(Ordering::Relaxed) {
                                break;
                            }

                            // Pre-process data for UI if needed
                            match update {
                                UiUpdate::SensorData(ts) => {
                                    tracing::trace!(
                                        "[UI Worker {}] Sensor update ts={}",
                                        worker_id,
                                        ts
                                    );
                                }
                                UiUpdate::LogicOutput(ts) => {
                                    tracing::trace!(
                                        "[UI Worker {}] Logic update ts={}",
                                        worker_id,
                                        ts
                                    );
                                }
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!("[UI Worker {}] Lagged {} messages", worker_id, n);
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            break;
                        }
                    }
                }

                tracing::debug!("[UI Worker {}] Stopped", worker_id);
            });
        }

        // ================================================================
        // Shutdown Watcher
        // ================================================================
        let state_shutdown = state.clone();
        self.runtime.spawn(async move {
            let _ = shutdown_rx.recv().await;
            state_shutdown.running.store(false, Ordering::SeqCst);
            tracing::info!("[Shutdown] Signal received, stopping workers");
        });
    }

    /// Get a receiver for UI update notifications
    pub fn subscribe_ui(&self) -> broadcast::Receiver<UiUpdate> {
        self.ui_tx.subscribe()
    }

    /// Get reference to shared state for GTK main thread
    pub fn state(&self) -> &Arc<WorkerState> {
        &self.state
    }

    /// Get latest sensor data (called from GTK main thread)
    /// Uses try_read to avoid blocking the GTK main thread
    pub fn try_get_sensors(&self) -> Option<SensorData> {
        // Use try_read to avoid blocking GTK main thread
        // If the write lock is held, we'll just skip this update and get the next one
        self.state.sensors.try_read().ok().map(|guard| guard.clone())
    }

    /// Get latest logic output (called from GTK main thread)
    /// Uses try_read to avoid blocking the GTK main thread
    pub fn try_get_logic(&self) -> Option<LogicOutput> {
        // Use try_read to avoid blocking GTK main thread
        // If the write lock is held, we'll just skip this update and get the next one
        self.state.logic.try_read().ok().map(|guard| guard.clone())
    }

    /// Get diagnostic counters
    pub fn diagnostics(&self) -> (u64, u64) {
        (
            self.state.sensor_reads.load(Ordering::Relaxed),
            self.state.logic_cycles.load(Ordering::Relaxed),
        )
    }

    /// Check if runtime is running
    pub fn is_running(&self) -> bool {
        self.state.running.load(Ordering::Relaxed)
    }

    /// Shutdown the runtime gracefully
    pub fn shutdown(&self) {
        self.state.running.store(false, Ordering::SeqCst);
        tracing::info!("[Runtime] Shutdown initiated");
    }
}

impl Default for WorkerRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for WorkerRuntime {
    fn drop(&mut self) {
        // Signal shutdown - workers will stop on their next iteration
        // No blocking sleep in destructor to avoid UI freezes
        self.shutdown();
    }
}

// ============================================================================
// Sensor Reading (runs in blocking thread pool)
// ============================================================================

/// Cached GPU data for other code that might need it
static CACHED_GPU_DATA: std::sync::OnceLock<std::sync::RwLock<Vec<GpuReading>>> = std::sync::OnceLock::new();

fn get_gpu_cache() -> &'static std::sync::RwLock<Vec<GpuReading>> {
    CACHED_GPU_DATA.get_or_init(|| std::sync::RwLock::new(Vec::new()))
}

/// Read all sensors - runs in spawn_blocking to not block async runtime
fn read_all_sensors_blocking() -> SensorData {
    let timestamp_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let mut temperatures = Vec::new();
    let mut fans = Vec::new();
    let mut gpus: Vec<GpuReading> = Vec::new();

    // PERFORMANCE: Single batched IPC call for hardware + GPUs
    // This reduces IPC round-trips from 2 to 1 per poll cycle
    // Falls back to separate calls if daemon doesn't support ListAll yet
    let all_data: Result<hf_core::DaemonAllHardwareData, String> = hf_core::daemon_list_all()
        .or_else(|_| {
            // Fallback for older daemon versions
            let hardware = hf_core::daemon_list_hardware()?;
            let gpus = hf_core::daemon_list_gpus().unwrap_or_default();
            Ok(hf_core::DaemonAllHardwareData { hardware, gpus })
        });
    
    if let Ok(all_data) = all_data {
        // Process hwmon chips
        for chip in all_data.hardware.chips {
            for temp in chip.temperatures {
                temperatures.push(TempReading {
                    path: temp.path,
                    label: temp.label.unwrap_or(temp.name),
                    chip_name: chip.name.clone(),
                    temp_celsius: temp.value,
                });
            }
            
            for fan in chip.fans {
                // Try to find matching PWM controller for this fan
                // Convention: fan1_input -> pwm1, fan2_input -> pwm2, etc.
                let fan_index = fan.name.chars()
                    .find(|c| c.is_ascii_digit())
                    .and_then(|c| c.to_digit(10));
                
                let (pwm_value, percent) = fan_index
                    .and_then(|idx| {
                        let pwm_name = format!("pwm{}", idx);
                        chip.pwms.iter()
                            .find(|p| p.name == pwm_name)
                            .map(|p| {
                                let pct = (p.value as f32 / 255.0) * 100.0;
                                (Some(p.value), Some(pct))
                            })
                    })
                    .unwrap_or((None, None));
                
                fans.push(FanReading {
                    path: fan.path.clone(),
                    label: fan.label.unwrap_or(fan.name.clone()),
                    chip_name: chip.name.clone(),
                    rpm: fan.rpm,
                    percent,
                    pwm_value,
                });
            }
        }
        
        // Process GPUs from batched response
        gpus = all_data.gpus.into_iter()
            .map(|gpu| {
                let mut temperatures: std::collections::HashMap<String, f32> = std::collections::HashMap::new();
                if let Some(t) = gpu.temp {
                    temperatures.insert("GPU".to_string(), t);
                }

                GpuReading {
                    index: gpu.index,
                    name: gpu.name,
                    vendor: gpu.vendor,
                    temp: gpu.temp,
                    temperatures,
                    fan_percent: gpu.fan_percent,
                    fan_rpm: gpu.fan_rpm,
                    power_watts: None,
                    utilization: None,
                    vram_used_mb: None,
                    vram_total_mb: None,
                }
            })
            .collect();
        
        // Update GPU cache for other code that might need it
        if let Ok(mut cache) = get_gpu_cache().write() {
            *cache = gpus.clone();
        }
    } else {
        tracing::warn!("[Runtime] Failed to get hardware data from daemon");
    }

    // Add GPU temperatures to the temperatures array so they can be found by path lookup
    // This allows active controls to read GPU temps using the gpu:index:name path format
    for gpu in &gpus {
        if let Some(temp) = gpu.temp {
            let path = format!("gpu:{}:GPU", gpu.index);
            temperatures.push(TempReading {
                path,
                label: format!("{} GPU", gpu.name),
                chip_name: format!("{} ({})", gpu.name, gpu.vendor),
                temp_celsius: temp,
            });
        }
    }

    SensorData {
        timestamp_ms,
        temperatures,
        fans,
        gpus,
    }
}

// ============================================================================
// Logic Processing (runs in logic worker)
// ============================================================================

/// Process sensor data and compute fan targets
/// 
/// Note: Fan curve processing is handled by the daemon (hyperfand).
/// This function exists for future client-side preview/simulation.
fn process_logic(sensors: &SensorData) -> LogicOutput {
    // Fan curve processing is delegated to the daemon for safety
    // This stub exists for potential client-side curve preview
    LogicOutput {
        timestamp_ms: sensors.timestamp_ms,
        fan_targets: Vec::new(),
    }
}

// ============================================================================
// Global Runtime Access
// ============================================================================

use std::sync::OnceLock;

static RUNTIME: OnceLock<WorkerRuntime> = OnceLock::new();

/// Initialize the global worker runtime (call once at startup)
pub fn init_runtime() {
    RUNTIME.get_or_init(WorkerRuntime::new);
}

/// Get the global worker runtime
pub fn runtime() -> &'static WorkerRuntime {
    RUNTIME.get().expect("FATAL: Worker runtime not initialized. This indicates a programming error - init_runtime() must be called during application startup.")
}

/// Get latest sensor data (non-blocking, returns None if lock contention)
pub fn get_sensors() -> Option<SensorData> {
    RUNTIME.get().and_then(|r| r.try_get_sensors())
}

/// Get latest logic output (non-blocking)
pub fn get_logic() -> Option<LogicOutput> {
    RUNTIME.get().and_then(|r| r.try_get_logic())
}

/// Subscribe to UI update notifications
pub fn subscribe_ui() -> Option<broadcast::Receiver<UiUpdate>> {
    RUNTIME.get().map(|r| r.subscribe_ui())
}

/// Get diagnostic info (sensor_reads, logic_cycles)
pub fn diagnostics() -> Option<(u64, u64)> {
    RUNTIME.get().map(|r| r.diagnostics())
}
