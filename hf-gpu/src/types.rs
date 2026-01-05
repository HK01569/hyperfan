//! GPU data types

use serde::{Deserialize, Serialize};

/// GPU vendor type
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
}

impl std::fmt::Display for GpuVendor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GpuVendor::Nvidia => write!(f, "NVIDIA"),
            GpuVendor::Amd => write!(f, "AMD"),
            GpuVendor::Intel => write!(f, "Intel"),
        }
    }
}

/// Represents a detected GPU with its sensors and fans
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GpuDevice {
    /// GPU index (0, 1, 2, etc.)
    pub index: u32,
    /// GPU name/model
    pub name: String,
    /// Vendor (NVIDIA, AMD, Intel)
    pub vendor: GpuVendor,
    /// PCI bus ID (e.g., "0000:01:00.0")
    pub pci_bus_id: Option<String>,
    /// VRAM total in MB
    pub vram_total_mb: Option<u32>,
    /// VRAM used in MB
    pub vram_used_mb: Option<u32>,
    /// Temperature sensors on this GPU
    pub temperatures: Vec<GpuTemperature>,
    /// Fans on this GPU
    pub fans: Vec<GpuFan>,
    /// Power usage in watts
    pub power_watts: Option<f32>,
    /// Power limit in watts
    pub power_limit_watts: Option<f32>,
    /// GPU utilization percentage
    pub utilization_percent: Option<u32>,
}

/// GPU temperature sensor
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GpuTemperature {
    /// Sensor name (e.g., "GPU Core", "Memory Junction", "Hotspot")
    pub name: String,
    /// Current temperature in Celsius
    pub current_temp: Option<f32>,
    /// Maximum recorded temperature
    pub max_temp: Option<f32>,
    /// Critical temperature threshold
    pub critical_temp: Option<f32>,
    /// Slowdown temperature threshold
    pub slowdown_temp: Option<f32>,
}

/// GPU fan information
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GpuFan {
    /// Fan index on this GPU
    pub index: u32,
    /// Fan name/label
    pub name: String,
    /// Current fan speed percentage (0-100)
    pub speed_percent: Option<u32>,
    /// Current RPM if available
    pub rpm: Option<u32>,
    /// Target speed percentage (for manual control)
    pub target_percent: Option<u32>,
    /// Whether manual control is currently active
    pub manual_control: bool,
    /// Minimum allowed speed percentage
    pub min_percent: Option<u32>,
    /// Maximum allowed speed percentage
    pub max_percent: Option<u32>,
}

/// Snapshot of all GPU data
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GpuSnapshot {
    /// Timestamp in milliseconds since Unix epoch
    pub timestamp_ms: u64,
    /// All detected GPUs
    pub gpus: Vec<GpuDevice>,
}

/// Represents a GPU PWM controller that can be used for fan pairing
#[derive(Debug, Clone)]
pub struct GpuPwmController {
    /// Unique identifier for this controller
    pub id: String,
    /// Display name (e.g., "NVIDIA RTX 3080 Fan 0")
    pub name: String,
    /// GPU vendor
    pub vendor: GpuVendor,
    /// GPU index (for NVIDIA) or card number (for AMD)
    pub gpu_index: u32,
    /// Fan index within the GPU
    pub fan_index: u32,
    /// PWM path (for AMD - sysfs path, for NVIDIA - virtual path)
    pub pwm_path: String,
    /// Fan RPM input path (if available)
    pub fan_input_path: Option<String>,
    /// Current fan speed percentage
    pub current_percent: Option<u32>,
    /// Current RPM (if available)
    pub current_rpm: Option<u32>,
    /// Whether manual control is currently enabled
    pub manual_control: bool,
    /// PCI bus ID for identification
    pub pci_bus_id: Option<String>,
}
