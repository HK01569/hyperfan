//! GPU-related constants

/// PCI vendor ID for AMD GPUs
pub const AMD_VENDOR_ID: &str = "0x1002";

/// PCI vendor ID for NVIDIA GPUs
pub const NVIDIA_VENDOR_ID: &str = "0x10de";

/// PCI vendor ID for Intel GPUs
pub const INTEL_VENDOR_ID: &str = "0x8086";

/// Path to DRM (Direct Rendering Manager) devices
pub const DRM_PATH: &str = "/sys/class/drm";

/// Number of temperature sensors to scan on Intel GPUs
pub const INTEL_TEMP_SENSOR_COUNT: usize = 3;

/// Microwatts per watt (for power conversion)
pub const MICROWATTS_PER_WATT: f32 = 1_000_000.0;

/// Bytes per megabyte (for VRAM conversion)
pub const BYTES_PER_MB: u64 = 1024 * 1024;

/// Temperature readings are in millidegrees, divide by this to get Celsius
pub const MILLIDEGREE_DIVISOR: f32 = 1000.0;

/// Maximum number of fans per GPU (safety cap)
pub const MAX_FANS_PER_GPU: u32 = 4;

/// PWM constants
pub mod pwm {
    /// Convert percentage (0-100) to PWM value (0-255)
    #[inline]
    pub fn from_percent(percent: f32) -> u8 {
        ((percent.clamp(0.0, 100.0) / 100.0) * 255.0).round() as u8
    }

    /// Convert PWM value (0-255) to percentage (0-100)
    #[inline]
    pub fn to_percent(value: u8) -> f32 {
        (value as f32 / 255.0) * 100.0
    }
}
