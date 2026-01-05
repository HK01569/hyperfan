//! Sensor and PWM Channel Fingerprinting
//!
//! Comprehensive identification system to prevent sensor/PWM mispairing across reboots.
//! Uses multiple anchor points (stable identifiers) and guards (validation checks)
//! to ensure fans are NEVER mispaired after user configuration.
//!
//! # Stability Domains
//!
//! - **Anchor**: Stable identifiers that survive reboots and hwmon reindexing
//! - **Guard**: Validation checks that detect drift or misconfiguration
//! - **Reason**: Heuristic hints for matching when primary anchors are missing

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, trace};

// ============================================================================
// Core Enums
// ============================================================================

/// Validation state of a sensor/PWM channel binding
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValidationState {
    /// All anchors match, guards pass - safe to use
    Ok,
    /// Some anchors match but confidence reduced - user should verify
    Degraded,
    /// Critical anchors changed - requires rebinding
    NeedsRebind,
    /// Validation failed - unsafe to apply fan control
    Unsafe,
}

impl Default for ValidationState {
    fn default() -> Self {
        Self::NeedsRebind
    }
}

/// Classification of the hardware chip type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChipClass {
    /// CPU temperature/fan controller (coretemp, k10temp, etc.)
    Cpu,
    /// Discrete GPU (amdgpu, nvidia, etc.)
    Gpu,
    /// Embedded Controller (thinkpad_hwmon, dell-smm-hwmon, etc.)
    EmbeddedController,
    /// SuperIO chip (nct6775, it87, etc.)
    SuperIO,
    /// ACPI thermal zone
    AcpiThermal,
    /// NVMe drive
    Nvme,
    /// Unknown classification
    Unknown,
}

impl Default for ChipClass {
    fn default() -> Self {
        Self::Unknown
    }
}

/// Type of sensor channel
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelType {
    /// Temperature sensor (tempN_input)
    Temperature,
    /// Fan RPM sensor (fanN_input)
    Fan,
    /// PWM controller (pwmN)
    Pwm,
    /// Voltage sensor (inN_input)
    Voltage,
    /// Power sensor
    Power,
    /// Current sensor
    Current,
}

/// Semantic role of a temperature sensor
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SemanticRole {
    /// CPU package temperature
    CpuPackage,
    /// Individual CPU core
    CpuCore,
    /// GPU core temperature
    GpuCore,
    /// GPU memory/hotspot
    GpuHotspot,
    /// Motherboard/ambient
    Motherboard,
    /// Chipset temperature
    Chipset,
    /// VRM/power delivery
    Vrm,
    /// NVMe/storage
    Storage,
    /// Unknown role
    Unknown,
}

impl Default for SemanticRole {
    fn default() -> Self {
        Self::Unknown
    }
}

/// Sensor scope (granularity of measurement)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SensorScope {
    /// Package-level (entire chip)
    Package,
    /// Individual core/unit
    Core,
    /// Hotspot/junction temperature
    Hotspot,
    /// Edge/case temperature
    Edge,
    /// Ambient/environmental
    Ambient,
    /// Unknown scope
    Unknown,
}

impl Default for SensorScope {
    fn default() -> Self {
        Self::Unknown
    }
}

/// Safe fallback policy when confidence is low
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SafeFallbackPolicy {
    /// Set fans to 100% (safest)
    FullSpeed,
    /// Set fans to 50%
    MediumSpeed,
    /// Restore BIOS/automatic control
    RestoreAuto,
    /// Keep current speed (risky)
    KeepCurrent,
    /// Custom percentage
    CustomPercent(u8),
}

impl Default for SafeFallbackPolicy {
    fn default() -> Self {
        Self::FullSpeed
    }
}

/// Expected temperature unit
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExpectedUnits {
    /// Millidegrees Celsius (typical for hwmon)
    MilliCelsius,
    /// Degrees Celsius
    Celsius,
    /// RPM (for fans)
    Rpm,
    /// PWM duty cycle (0-255)
    PwmDuty,
    /// Millivolts
    Millivolts,
    /// Milliwatts
    Milliwatts,
}

impl Default for ExpectedUnits {
    fn default() -> Self {
        Self::MilliCelsius
    }
}

// ============================================================================
// Anchor Data Structures
// ============================================================================

/// PCI device identification (for GPUs, chipsets)
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PciIdentity {
    /// PCI address (e.g., "0000:01:00.0")
    pub address: Option<String>,
    /// Vendor ID (e.g., "0x1002" for AMD)
    pub vendor_id: Option<String>,
    /// Device ID
    pub device_id: Option<String>,
    /// Subsystem vendor ID
    pub subsystem_vendor_id: Option<String>,
    /// Subsystem device ID
    pub subsystem_device_id: Option<String>,
    /// Device class
    pub class: Option<String>,
}

/// I2C device identification (for SuperIO chips)
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct I2cIdentity {
    /// I2C bus number
    pub bus_number: Option<u32>,
    /// Device address on the bus (e.g., 0x2d)
    pub device_address: Option<u8>,
    /// Adapter name
    pub adapter_name: Option<String>,
}

/// ACPI path identification (for laptop EC)
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcpiIdentity {
    /// Full ACPI path (e.g., "LNXSYSTM:00/LNXSYBUS:00/...")
    pub path: Option<String>,
    /// ACPI HID
    pub hid: Option<String>,
    /// ACPI UID
    pub uid: Option<String>,
}

// ============================================================================
// Runtime Statistics
// ============================================================================

/// Runtime sampling statistics for drift detection
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeStats {
    /// Expected value range (min, max) observed during calibration
    pub expected_value_range: Option<(f32, f32)>,
    /// Initial value bucket at boot time
    pub initial_value_bucket: Option<ValueBucket>,
    /// Variance profile from sampling
    pub variance_profile: Option<VarianceProfile>,
    /// Polling delta profile
    pub polling_delta_profile: Option<DeltaProfile>,
    /// Estimated update frequency in Hz
    pub update_frequency_estimate: Option<f32>,
    /// Last seen timestamp (Unix epoch ms)
    pub last_seen_timestamp: Option<u64>,
}

/// Value bucket for initial sanity check
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValueBucket {
    /// Very cold (e.g., < 20°C)
    VeryCold,
    /// Cold (e.g., 20-35°C)
    Cold,
    /// Warm (e.g., 35-50°C)
    Warm,
    /// Hot (e.g., 50-70°C)
    Hot,
    /// VeryHot (e.g., > 70°C)
    VeryHot,
}

/// Variance profile for noise detection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VarianceProfile {
    /// Very stable (< 0.5 variance)
    Stable,
    /// Normal variance
    Normal,
    /// High variance (noisy sensor)
    Noisy,
    /// Dead/frozen (zero variance over time)
    Frozen,
}

/// Delta profile for detecting swapped sensors
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeltaProfile {
    /// Responsive to load changes
    Responsive,
    /// Slow response
    Sluggish,
    /// No response (possible stale/virtual)
    Unresponsive,
}

// ============================================================================
// PWM-Fan Relationship Probing
// ============================================================================

/// Results from active PWM probing
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PwmProbeData {
    /// PWM → fan response mapping (pwm_value -> measured_rpm)
    pub response_map: Vec<(u8, u32)>,
    /// RPM delta when stepping PWM (confirms physical control)
    pub rpm_delta_on_step: Option<i32>,
    /// Confirmed writable
    pub write_capability: bool,
    /// BIOS/EC override detected
    pub control_authority_override: bool,
    /// Response time in milliseconds
    pub response_time_ms: Option<u32>,
}

// ============================================================================
// Main Fingerprint Structures
// ============================================================================

/// Complete fingerprint for an hwmon chip
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChipFingerprint {
    // === ANCHORS (stable identifiers) ===
    
    /// Driver name from /sys/class/hwmon/hwmonX/name (High stability)
    pub driver_name: String,
    /// Realpath of device symlink (High stability)
    pub device_symlink_target: Option<String>,
    /// PCI identity if applicable (Very High stability)
    pub pci_identity: Option<PciIdentity>,
    /// I2C identity if applicable (High stability)
    pub i2c_identity: Option<I2cIdentity>,
    /// ACPI identity if applicable (Medium stability)
    pub acpi_identity: Option<AcpiIdentity>,
    
    // === REASON (heuristic hints) ===
    
    /// Modalias string (Medium stability)
    pub modalias: Option<String>,
    /// Classified chip type
    pub chip_class: ChipClass,
    
    // === METADATA ===
    
    /// Original hwmon path at discovery time
    pub original_hwmon_path: PathBuf,
    /// Fingerprint creation timestamp
    pub created_at: u64,
    /// Last validation timestamp
    pub last_validated_at: Option<u64>,
}

/// Complete fingerprint for a sensor channel (temp, fan, pwm)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChannelFingerprint {
    // === ANCHORS ===
    
    /// Parent chip fingerprint ID (hash)
    pub chip_fingerprint_id: String,
    /// Channel type (temp/fan/pwm)
    pub channel_type: ChannelType,
    /// Raw label text if present (High stability)
    pub label_text_raw: Option<String>,
    /// Normalized/canonicalized label
    pub label_text_normalized: Option<String>,
    
    // === REASON (weak hints) ===
    
    /// Channel index from filename (Low stability - never trust alone!)
    pub channel_index: u32,
    /// Semantic role (CPU, GPU, etc.)
    pub semantic_role: SemanticRole,
    /// Sensor scope (package, core, hotspot)
    pub sensor_scope: SensorScope,
    /// Monotonicity expectation (temps rise under load)
    pub monotonicity_expectation: Option<bool>,
    
    // === GUARDS (validation checks) ===
    
    /// Set of attribute files present
    pub attribute_fingerprint: HashSet<String>,
    /// Has *_input file
    pub has_input_file: bool,
    /// Has *_label file
    pub has_label_file: bool,
    /// Expected units for this channel
    pub expected_units: ExpectedUnits,
    
    // === RUNTIME ===
    
    /// Runtime statistics
    pub runtime_stats: RuntimeStats,
    
    // === METADATA ===
    
    /// Original sensor name (e.g., "temp1")
    pub original_name: String,
    /// Original path
    pub original_path: PathBuf,
}

/// Complete fingerprint for a PWM channel with fan association
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PwmChannelFingerprint {
    /// Base channel fingerprint
    pub channel: ChannelFingerprint,
    
    // === PWM-SPECIFIC GUARDS ===
    
    /// Has pwmX_enable file
    pub has_enable_file: bool,
    /// PWM write capability confirmed
    pub pwm_write_capability: bool,
    /// BIOS/EC override detected
    pub control_authority: Option<String>,
    
    // === FAN ASSOCIATION ===
    
    /// Associated fan channel fingerprint ID (if paired)
    pub paired_fan_fingerprint_id: Option<String>,
    /// Has fanX_input for closed-loop control
    pub has_rpm_feedback: bool,
    /// Probe data from active testing
    pub probe_data: Option<PwmProbeData>,
    
    // === SAFETY ===
    
    /// Safe fallback policy
    pub safe_fallback_policy: SafeFallbackPolicy,
}

/// Validated binding between PWM and fan with confidence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatedPwmFanBinding {
    /// PWM channel fingerprint
    pub pwm_fingerprint: PwmChannelFingerprint,
    /// Fan channel fingerprint (if present)
    pub fan_fingerprint: Option<ChannelFingerprint>,
    /// Temperature sensor fingerprint (control source)
    pub temp_fingerprint: Option<ChannelFingerprint>,
    
    // === VALIDATION STATE ===
    
    /// Current validation state
    pub validation_state: ValidationState,
    /// Unified confidence score (0.0 - 1.0)
    pub confidence_score: f32,
    /// User acknowledged low confidence
    pub user_override_ack: bool,
    /// Reasons for current confidence level
    pub confidence_reasons: Vec<String>,
    
    // === METADATA ===
    
    /// Binding creation timestamp
    pub created_at: u64,
    /// Last successful validation timestamp
    pub last_validated_at: Option<u64>,
    /// Number of successful validations
    pub validation_count: u32,
}

// ============================================================================
// Fingerprint Extraction Functions
// ============================================================================

/// Extract chip fingerprint from hwmon path
pub fn extract_chip_fingerprint(hwmon_path: &Path) -> Option<ChipFingerprint> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    
    // Read driver name
    let name_path = hwmon_path.join("name");
    let driver_name = fs::read_to_string(&name_path)
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string());
    
    trace!(path = ?hwmon_path, driver = %driver_name, "Extracting chip fingerprint");
    
    // Resolve device symlink
    let device_path = hwmon_path.join("device");
    let device_symlink_target = if device_path.exists() {
        fs::canonicalize(&device_path)
            .ok()
            .map(|p| p.to_string_lossy().to_string())
    } else {
        None
    };
    
    // Extract PCI identity
    let pci_identity = extract_pci_identity(hwmon_path);
    
    // Extract I2C identity
    let i2c_identity = extract_i2c_identity(hwmon_path);
    
    // Extract ACPI identity
    let acpi_identity = extract_acpi_identity(hwmon_path);
    
    // Read modalias
    let modalias_path = hwmon_path.join("device/modalias");
    let modalias = fs::read_to_string(&modalias_path)
        .ok()
        .map(|s| s.trim().to_string());
    
    // Classify chip type
    let chip_class = classify_chip(&driver_name, &pci_identity, &modalias);
    
    Some(ChipFingerprint {
        driver_name,
        device_symlink_target,
        pci_identity,
        i2c_identity,
        acpi_identity,
        modalias,
        chip_class,
        original_hwmon_path: hwmon_path.to_path_buf(),
        created_at: now,
        last_validated_at: None,
    })
}

/// Extract PCI identity from hwmon device
fn extract_pci_identity(hwmon_path: &Path) -> Option<PciIdentity> {
    let device_path = hwmon_path.join("device");
    if !device_path.exists() {
        return None;
    }
    
    // Try to get real path and check if it's a PCI device
    let real_path = fs::canonicalize(&device_path).ok()?;
    let path_str = real_path.to_string_lossy();
    
    // Check if this is a PCI device path
    if !path_str.contains("/pci") {
        return None;
    }
    
    // Extract PCI address from path (e.g., "0000:01:00.0")
    let address = extract_pci_address_from_path(&path_str);
    
    // Read PCI attributes
    let vendor_id = read_sysfs_attr(&device_path, "vendor");
    let device_id = read_sysfs_attr(&device_path, "device");
    let subsystem_vendor_id = read_sysfs_attr(&device_path, "subsystem_vendor");
    let subsystem_device_id = read_sysfs_attr(&device_path, "subsystem_device");
    let class = read_sysfs_attr(&device_path, "class");
    
    if address.is_some() || vendor_id.is_some() {
        Some(PciIdentity {
            address,
            vendor_id,
            device_id,
            subsystem_vendor_id,
            subsystem_device_id,
            class,
        })
    } else {
        None
    }
}

/// Extract PCI address from sysfs path
fn extract_pci_address_from_path(path: &str) -> Option<String> {
    // Look for pattern like "0000:01:00.0"
    let pci_addr_pattern = regex::Regex::new(r"([0-9a-fA-F]{4}:[0-9a-fA-F]{2}:[0-9a-fA-F]{2}\.[0-9a-fA-F])").ok()?;
    pci_addr_pattern.captures(path).map(|c| c[1].to_string())
}

/// Extract I2C identity from hwmon device
fn extract_i2c_identity(hwmon_path: &Path) -> Option<I2cIdentity> {
    let device_path = hwmon_path.join("device");
    if !device_path.exists() {
        return None;
    }
    
    let real_path = fs::canonicalize(&device_path).ok()?;
    let path_str = real_path.to_string_lossy();
    
    // Check if this is an I2C device path
    if !path_str.contains("/i2c-") {
        return None;
    }
    
    // Extract bus number and address from path like "/sys/devices/.../i2c-0/0-002d/..."
    let (bus_number, device_address) = extract_i2c_info_from_path(&path_str)?;
    
    // Try to get adapter name
    let adapter_name = find_i2c_adapter_name(bus_number);
    
    Some(I2cIdentity {
        bus_number: Some(bus_number),
        device_address: Some(device_address),
        adapter_name,
    })
}

/// Extract I2C bus and address from sysfs path
fn extract_i2c_info_from_path(path: &str) -> Option<(u32, u8)> {
    // Look for pattern like "i2c-0/0-002d"
    let i2c_pattern = regex::Regex::new(r"i2c-(\d+)/(\d+)-([0-9a-fA-F]{4})").ok()?;
    let caps = i2c_pattern.captures(path)?;
    
    let bus: u32 = caps[1].parse().ok()?;
    let addr_str = &caps[3];
    let addr: u8 = u8::from_str_radix(addr_str, 16).ok()?;
    
    Some((bus, addr))
}

/// Find I2C adapter name
fn find_i2c_adapter_name(bus: u32) -> Option<String> {
    let adapter_path = format!("/sys/bus/i2c/devices/i2c-{}/name", bus);
    fs::read_to_string(&adapter_path)
        .ok()
        .map(|s| s.trim().to_string())
}

/// Extract ACPI identity from hwmon device
fn extract_acpi_identity(hwmon_path: &Path) -> Option<AcpiIdentity> {
    let device_path = hwmon_path.join("device");
    if !device_path.exists() {
        return None;
    }
    
    let real_path = fs::canonicalize(&device_path).ok()?;
    let path_str = real_path.to_string_lossy();
    
    // Check if this is an ACPI device
    if !path_str.contains("LNXSYSTM") && !path_str.contains("ACPI") {
        return None;
    }
    
    // Read ACPI attributes
    let hid = read_sysfs_attr(&device_path, "hid");
    let uid = read_sysfs_attr(&device_path, "uid");
    
    Some(AcpiIdentity {
        path: Some(path_str.to_string()),
        hid,
        uid,
    })
}

/// Read a sysfs attribute file
fn read_sysfs_attr(device_path: &Path, attr: &str) -> Option<String> {
    let attr_path = device_path.join(attr);
    fs::read_to_string(&attr_path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Classify chip type based on driver name and other info
fn classify_chip(
    driver_name: &str,
    pci_identity: &Option<PciIdentity>,
    modalias: &Option<String>,
) -> ChipClass {
    let name_lower = driver_name.to_lowercase();
    
    // CPU temperature drivers
    if name_lower.contains("coretemp")
        || name_lower.contains("k10temp")
        || name_lower.contains("k8temp")
        || name_lower.contains("zenpower")
        || name_lower.contains("cpu")
    {
        return ChipClass::Cpu;
    }
    
    // GPU drivers
    if name_lower.contains("amdgpu")
        || name_lower.contains("radeon")
        || name_lower.contains("nouveau")
        || name_lower.contains("nvidia")
        || name_lower.contains("i915")
        || name_lower.contains("xe")
    {
        return ChipClass::Gpu;
    }
    
    // Check PCI class for GPU
    if let Some(pci) = pci_identity {
        if let Some(class) = &pci.class {
            // PCI class 0x03 is display controller
            if class.starts_with("0x03") {
                return ChipClass::Gpu;
            }
        }
    }
    
    // Embedded Controller drivers
    if name_lower.contains("thinkpad")
        || name_lower.contains("dell-smm")
        || name_lower.contains("dell_smm")
        || name_lower.contains("hp-wmi")
        || name_lower.contains("asus-ec")
        || name_lower.contains("applesmc")
    {
        return ChipClass::EmbeddedController;
    }
    
    // SuperIO drivers
    if name_lower.contains("nct")
        || name_lower.contains("it87")
        || name_lower.contains("w83")
        || name_lower.contains("f71")
        || name_lower.contains("sch")
        || name_lower.contains("pc87")
        || name_lower.contains("lm")
        || name_lower.contains("adt")
        || name_lower.contains("emc")
    {
        return ChipClass::SuperIO;
    }
    
    // ACPI thermal
    if name_lower.contains("acpi") || name_lower.contains("thermal") {
        return ChipClass::AcpiThermal;
    }
    
    // NVMe
    if name_lower.contains("nvme") || name_lower.contains("drivetemp") {
        return ChipClass::Nvme;
    }
    
    // Check modalias for hints
    if let Some(alias) = modalias {
        let alias_lower = alias.to_lowercase();
        if alias_lower.contains("acpi") {
            return ChipClass::AcpiThermal;
        }
        if alias_lower.contains("i2c") {
            return ChipClass::SuperIO;
        }
    }
    
    ChipClass::Unknown
}

/// Extract channel fingerprint for a sensor
pub fn extract_channel_fingerprint(
    chip_fingerprint: &ChipFingerprint,
    channel_type: ChannelType,
    sensor_name: &str,
    sensor_path: &Path,
) -> ChannelFingerprint {
    let chip_path = &chip_fingerprint.original_hwmon_path;
    let base_name = sensor_name.trim_end_matches("_input");
    
    // Extract channel index
    let channel_index = extract_channel_index(base_name).unwrap_or(0);
    
    // Read label
    let label_path = chip_path.join(format!("{}_label", base_name));
    let label_text_raw = fs::read_to_string(&label_path)
        .ok()
        .map(|s| s.trim().to_string());
    let label_text_normalized = label_text_raw.as_ref().map(|l| normalize_label(l));
    
    // Build attribute fingerprint
    let attribute_fingerprint = scan_channel_attributes(chip_path, base_name);
    
    // Check file presence
    let has_input_file = sensor_path.exists();
    let has_label_file = label_path.exists();
    
    // Determine expected units
    let expected_units = match channel_type {
        ChannelType::Temperature => ExpectedUnits::MilliCelsius,
        ChannelType::Fan => ExpectedUnits::Rpm,
        ChannelType::Pwm => ExpectedUnits::PwmDuty,
        ChannelType::Voltage => ExpectedUnits::Millivolts,
        ChannelType::Power => ExpectedUnits::Milliwatts,
        ChannelType::Current => ExpectedUnits::Millivolts, // Usually reported as milliamps
    };
    
    // Infer semantic role
    let semantic_role = infer_semantic_role(
        &chip_fingerprint.chip_class,
        &label_text_normalized,
        base_name,
    );
    
    // Infer sensor scope
    let sensor_scope = infer_sensor_scope(&label_text_normalized, base_name);
    
    // Generate chip fingerprint ID (simple hash)
    let chip_fingerprint_id = generate_chip_id(chip_fingerprint);
    
    ChannelFingerprint {
        chip_fingerprint_id,
        channel_type,
        label_text_raw,
        label_text_normalized,
        channel_index,
        semantic_role,
        sensor_scope,
        monotonicity_expectation: match channel_type {
            ChannelType::Temperature => Some(true), // Temps generally rise under load
            _ => None,
        },
        attribute_fingerprint,
        has_input_file,
        has_label_file,
        expected_units,
        runtime_stats: RuntimeStats::default(),
        original_name: base_name.to_string(),
        original_path: sensor_path.to_path_buf(),
    }
}

/// Extract numeric index from sensor name (e.g., "temp1" -> 1)
fn extract_channel_index(name: &str) -> Option<u32> {
    let digits: String = name.chars().filter(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// Normalize a label for comparison
fn normalize_label(label: &str) -> String {
    label
        .to_lowercase()
        .replace(['_', '-', ' ', '/', '.'], "")
        .trim()
        .to_string()
}

/// Scan all attribute files for a channel
fn scan_channel_attributes(chip_path: &Path, base_name: &str) -> HashSet<String> {
    let mut attrs = HashSet::new();
    
    let suffixes = [
        "_input", "_label", "_enable", "_min", "_max", "_crit", "_alarm",
        "_type", "_auto_point1_pwm", "_auto_point2_pwm", "_auto_point3_pwm",
        "_auto_point1_temp", "_auto_point2_temp", "_auto_point3_temp",
        "_pulses", "_target", "_div",
    ];
    
    for suffix in suffixes {
        let attr_name = format!("{}{}", base_name, suffix);
        let attr_path = chip_path.join(&attr_name);
        if attr_path.exists() {
            attrs.insert(suffix.to_string());
        }
    }
    
    // For PWM, also check the base file without suffix
    if base_name.starts_with("pwm") {
        let pwm_path = chip_path.join(base_name);
        if pwm_path.exists() {
            attrs.insert("_pwm".to_string());
        }
    }
    
    attrs
}

/// Infer semantic role from chip class and label
fn infer_semantic_role(
    chip_class: &ChipClass,
    label_normalized: &Option<String>,
    name: &str,
) -> SemanticRole {
    // Check label first
    if let Some(label) = label_normalized {
        let l = label.to_lowercase();
        
        if l.contains("package") || l.contains("pkg") {
            return SemanticRole::CpuPackage;
        }
        if l.contains("core") && !l.contains("gpu") {
            return SemanticRole::CpuCore;
        }
        if l.contains("gpu") || l.contains("graphics") {
            if l.contains("hotspot") || l.contains("junction") || l.contains("mem") {
                return SemanticRole::GpuHotspot;
            }
            return SemanticRole::GpuCore;
        }
        if l.contains("vrm") || l.contains("vcore") || l.contains("power") {
            return SemanticRole::Vrm;
        }
        if l.contains("chipset") || l.contains("pch") || l.contains("sb") {
            return SemanticRole::Chipset;
        }
        if l.contains("nvme") || l.contains("ssd") || l.contains("drive") {
            return SemanticRole::Storage;
        }
        if l.contains("ambient") || l.contains("systin") || l.contains("motherboard") {
            return SemanticRole::Motherboard;
        }
    }
    
    // Infer from chip class
    match chip_class {
        ChipClass::Cpu => {
            if name.contains('1') || name.ends_with("1") {
                SemanticRole::CpuPackage
            } else {
                SemanticRole::CpuCore
            }
        }
        ChipClass::Gpu => SemanticRole::GpuCore,
        ChipClass::Nvme => SemanticRole::Storage,
        ChipClass::SuperIO => SemanticRole::Motherboard,
        _ => SemanticRole::Unknown,
    }
}

/// Infer sensor scope from label
fn infer_sensor_scope(label_normalized: &Option<String>, _name: &str) -> SensorScope {
    if let Some(label) = label_normalized {
        let l = label.to_lowercase();
        
        if l.contains("package") || l.contains("pkg") {
            return SensorScope::Package;
        }
        if l.contains("core") {
            return SensorScope::Core;
        }
        if l.contains("hotspot") || l.contains("junction") || l.contains("tdie") {
            return SensorScope::Hotspot;
        }
        if l.contains("edge") || l.contains("tctl") {
            return SensorScope::Edge;
        }
        if l.contains("ambient") {
            return SensorScope::Ambient;
        }
    }
    
    SensorScope::Unknown
}

/// Generate a unique ID for a chip fingerprint
pub fn generate_chip_id(chip: &ChipFingerprint) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    
    let mut hasher = DefaultHasher::new();
    
    // Hash stable anchors
    chip.driver_name.hash(&mut hasher);
    chip.device_symlink_target.hash(&mut hasher);
    
    if let Some(pci) = &chip.pci_identity {
        pci.address.hash(&mut hasher);
        pci.vendor_id.hash(&mut hasher);
        pci.device_id.hash(&mut hasher);
    }
    
    if let Some(i2c) = &chip.i2c_identity {
        i2c.bus_number.hash(&mut hasher);
        i2c.device_address.hash(&mut hasher);
    }
    
    format!("{:016x}", hasher.finish())
}

/// Generate a unique ID for a channel fingerprint
pub fn generate_channel_id(channel: &ChannelFingerprint) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    
    let mut hasher = DefaultHasher::new();
    
    channel.chip_fingerprint_id.hash(&mut hasher);
    channel.original_name.hash(&mut hasher);
    channel.label_text_raw.hash(&mut hasher);
    
    format!("{:016x}", hasher.finish())
}

// ============================================================================
// Validation Functions
// ============================================================================

/// Validate a chip fingerprint against current system state
pub fn validate_chip_fingerprint(
    fingerprint: &ChipFingerprint,
    current_hwmon_path: &Path,
) -> (ValidationState, f32, Vec<String>) {
    let mut score = 0.0f32;
    let mut max_score = 0.0f32;
    let mut reasons = Vec::new();
    
    // Check driver name (HIGH priority anchor)
    max_score += 30.0;
    let name_path = current_hwmon_path.join("name");
    if let Ok(current_name) = fs::read_to_string(&name_path) {
        if current_name.trim() == fingerprint.driver_name {
            score += 30.0;
            reasons.push("Driver name matches".to_string());
        } else {
            reasons.push(format!(
                "Driver name mismatch: expected '{}', got '{}'",
                fingerprint.driver_name,
                current_name.trim()
            ));
        }
    } else {
        reasons.push("Could not read driver name".to_string());
    }
    
    // Check device symlink target (HIGH priority anchor)
    if fingerprint.device_symlink_target.is_some() {
        max_score += 25.0;
        let device_path = current_hwmon_path.join("device");
        if let Ok(current_target) = fs::canonicalize(&device_path) {
            let current_str = current_target.to_string_lossy().to_string();
            if Some(&current_str) == fingerprint.device_symlink_target.as_ref() {
                score += 25.0;
                reasons.push("Device symlink target matches".to_string());
            } else if let Some(expected) = &fingerprint.device_symlink_target {
                reasons.push(format!(
                    "Device path changed: expected '{}', got '{}'",
                    expected,
                    current_str
                ));
            }
        }
    }
    
    // Check PCI identity (VERY HIGH priority anchor)
    if let Some(expected_pci) = &fingerprint.pci_identity {
        let current_pci = extract_pci_identity(current_hwmon_path);
        
        if expected_pci.address.is_some() {
            max_score += 20.0;
            if current_pci.as_ref().and_then(|p| p.address.as_ref()) == expected_pci.address.as_ref() {
                score += 20.0;
                reasons.push("PCI address matches".to_string());
            } else {
                reasons.push("PCI address mismatch".to_string());
            }
        }
        
        if expected_pci.vendor_id.is_some() {
            max_score += 10.0;
            if current_pci.as_ref().and_then(|p| p.vendor_id.as_ref()) == expected_pci.vendor_id.as_ref() {
                score += 10.0;
            } else {
                reasons.push("PCI vendor ID mismatch".to_string());
            }
        }
        
        if expected_pci.device_id.is_some() {
            max_score += 10.0;
            if current_pci.as_ref().and_then(|p| p.device_id.as_ref()) == expected_pci.device_id.as_ref() {
                score += 10.0;
            } else {
                reasons.push("PCI device ID mismatch".to_string());
            }
        }
    }
    
    // Check I2C identity (HIGH priority anchor)
    if let Some(expected_i2c) = &fingerprint.i2c_identity {
        let current_i2c = extract_i2c_identity(current_hwmon_path);
        
        if expected_i2c.bus_number.is_some() {
            max_score += 15.0;
            if current_i2c.as_ref().and_then(|i| i.bus_number) == expected_i2c.bus_number {
                score += 15.0;
                reasons.push("I2C bus matches".to_string());
            } else {
                reasons.push("I2C bus mismatch".to_string());
            }
        }
        
        if expected_i2c.device_address.is_some() {
            max_score += 15.0;
            if current_i2c.as_ref().and_then(|i| i.device_address) == expected_i2c.device_address {
                score += 15.0;
                reasons.push("I2C address matches".to_string());
            } else {
                reasons.push("I2C address mismatch".to_string());
            }
        }
    }
    
    // Calculate confidence
    let confidence = if max_score > 0.0 {
        score / max_score
    } else {
        0.0
    };
    
    // Determine validation state
    let state = if confidence >= 0.9 {
        ValidationState::Ok
    } else if confidence >= 0.7 {
        ValidationState::Degraded
    } else if confidence >= 0.4 {
        ValidationState::NeedsRebind
    } else {
        ValidationState::Unsafe
    };
    
    debug!(
        driver = %fingerprint.driver_name,
        confidence = format!("{:.2}", confidence),
        state = ?state,
        "Chip validation result"
    );
    
    (state, confidence, reasons)
}

/// Validate a channel fingerprint against current system state
pub fn validate_channel_fingerprint(
    fingerprint: &ChannelFingerprint,
    current_chip_path: &Path,
) -> (ValidationState, f32, Vec<String>) {
    let mut score = 0.0f32;
    let mut max_score = 0.0f32;
    let mut reasons = Vec::new();
    
    // Check input file exists (ABSOLUTE requirement)
    let input_path = current_chip_path.join(format!("{}_input", fingerprint.original_name));
    let pwm_path = current_chip_path.join(&fingerprint.original_name);
    
    let file_exists = match fingerprint.channel_type {
        ChannelType::Pwm => pwm_path.exists(),
        _ => input_path.exists(),
    };
    
    if !file_exists {
        reasons.push(format!("Channel file missing: {}", fingerprint.original_name));
        return (ValidationState::Unsafe, 0.0, reasons);
    }
    
    max_score += 20.0;
    score += 20.0;
    reasons.push("Channel file exists".to_string());
    
    // Check label matches (HIGH priority anchor if present)
    if fingerprint.has_label_file {
        max_score += 30.0;
        let label_path = current_chip_path.join(format!("{}_label", fingerprint.original_name));
        if let Ok(current_label) = fs::read_to_string(&label_path) {
            let current_normalized = normalize_label(current_label.trim());
            if Some(&current_normalized) == fingerprint.label_text_normalized.as_ref() {
                score += 30.0;
                reasons.push("Label matches".to_string());
            } else {
                reasons.push(format!(
                    "Label mismatch: expected '{:?}', got '{}'",
                    fingerprint.label_text_normalized,
                    current_label.trim()
                ));
            }
        } else if fingerprint.label_text_raw.is_some() {
            reasons.push("Label file missing but was expected".to_string());
        }
    }
    
    // Check attribute fingerprint (HIGH priority guard)
    max_score += 20.0;
    let current_attrs = scan_channel_attributes(current_chip_path, &fingerprint.original_name);
    let attr_match_count = fingerprint.attribute_fingerprint.intersection(&current_attrs).count();
    let attr_total = fingerprint.attribute_fingerprint.len().max(1);
    let attr_ratio = attr_match_count as f32 / attr_total as f32;
    score += 20.0 * attr_ratio;
    
    if attr_ratio < 1.0 {
        let missing: Vec<_> = fingerprint.attribute_fingerprint.difference(&current_attrs).collect();
        if !missing.is_empty() {
            reasons.push(format!("Missing attributes: {:?}", missing));
        }
    }
    
    // Calculate confidence
    let confidence = if max_score > 0.0 {
        score / max_score
    } else {
        0.0
    };
    
    // Determine validation state
    let state = if confidence >= 0.9 {
        ValidationState::Ok
    } else if confidence >= 0.7 {
        ValidationState::Degraded
    } else if confidence >= 0.4 {
        ValidationState::NeedsRebind
    } else {
        ValidationState::Unsafe
    };
    
    (state, confidence, reasons)
}

// ============================================================================
// PWM-specific validation
// ============================================================================

/// Extract PWM channel fingerprint
pub fn extract_pwm_fingerprint(
    chip_fingerprint: &ChipFingerprint,
    pwm_name: &str,
    pwm_path: &Path,
    enable_path: &Path,
) -> PwmChannelFingerprint {
    let channel = extract_channel_fingerprint(
        chip_fingerprint,
        ChannelType::Pwm,
        pwm_name,
        pwm_path,
    );
    
    let has_enable_file = enable_path.exists();
    
    // Try test write to verify capability
    let pwm_write_capability = test_pwm_write_capability(pwm_path);
    
    // Check for BIOS/EC override
    let control_authority = detect_control_authority(enable_path);
    
    // Check for corresponding fan input
    let fan_name = pwm_name.replace("pwm", "fan");
    let fan_input_path = chip_fingerprint.original_hwmon_path.join(format!("{}_input", fan_name));
    let has_rpm_feedback = fan_input_path.exists();
    
    PwmChannelFingerprint {
        channel,
        has_enable_file,
        pwm_write_capability,
        control_authority,
        paired_fan_fingerprint_id: None,
        has_rpm_feedback,
        probe_data: None,
        safe_fallback_policy: SafeFallbackPolicy::FullSpeed,
    }
}

/// Test if PWM is writable (non-destructive)
fn test_pwm_write_capability(pwm_path: &Path) -> bool {
    use std::fs::OpenOptions;
    
    OpenOptions::new()
        .write(true)
        .open(pwm_path)
        .is_ok()
}

/// Detect if BIOS/EC is overriding fan control
fn detect_control_authority(enable_path: &Path) -> Option<String> {
    if !enable_path.exists() {
        return Some("No enable file - BIOS controlled".to_string());
    }
    
    if let Ok(content) = fs::read_to_string(enable_path) {
        match content.trim() {
            "0" => Some("PWM disabled".to_string()),
            "2" | "3" | "4" | "5" => Some("Automatic/thermal control active".to_string()),
            "1" => None, // Manual control
            _ => Some(format!("Unknown mode: {}", content.trim())),
        }
    } else {
        Some("Cannot read enable state".to_string())
    }
}

// ============================================================================
// Matching and Rebinding
// ============================================================================

/// Find matching hwmon path for a chip fingerprint
pub fn find_matching_hwmon(fingerprint: &ChipFingerprint) -> Option<(PathBuf, f32)> {
    let hwmon_base = Path::new("/sys/class/hwmon");
    if !hwmon_base.exists() {
        return None;
    }
    
    let mut best_match: Option<(PathBuf, f32)> = None;
    
    if let Ok(entries) = fs::read_dir(hwmon_base) {
        for entry in entries.flatten() {
            let path = entry.path();
            let (state, confidence, _) = validate_chip_fingerprint(fingerprint, &path);
            
            if state != ValidationState::Unsafe {
                if let Some((_, best_conf)) = &best_match {
                    if confidence > *best_conf {
                        best_match = Some((path, confidence));
                    }
                } else {
                    best_match = Some((path, confidence));
                }
            }
        }
    }
    
    best_match
}

/// Find matching sensor within a chip for a channel fingerprint
pub fn find_matching_channel(
    fingerprint: &ChannelFingerprint,
    chip_path: &Path,
) -> Option<(PathBuf, f32)> {
    // First try the original name
    let (state, confidence, _) = validate_channel_fingerprint(fingerprint, chip_path);
    
    if state == ValidationState::Ok || state == ValidationState::Degraded {
        let sensor_path = match fingerprint.channel_type {
            ChannelType::Pwm => chip_path.join(&fingerprint.original_name),
            _ => chip_path.join(format!("{}_input", fingerprint.original_name)),
        };
        return Some((sensor_path, confidence));
    }
    
    // If original name fails, try to find by label
    if let Some(expected_label) = &fingerprint.label_text_normalized {
        if let Ok(entries) = fs::read_dir(chip_path) {
            for entry in entries.flatten() {
                let file_name = entry.file_name();
                let name_str = file_name.to_string_lossy();
                
                if name_str.ends_with("_label") {
                    if let Ok(label) = fs::read_to_string(entry.path()) {
                        let normalized = normalize_label(label.trim());
                        if &normalized == expected_label {
                            let base_name = name_str.trim_end_matches("_label");
                            let sensor_path = match fingerprint.channel_type {
                                ChannelType::Pwm => chip_path.join(base_name),
                                _ => chip_path.join(format!("{}_input", base_name)),
                            };
                            if sensor_path.exists() {
                                return Some((sensor_path, 0.8)); // High confidence on label match
                            }
                        }
                    }
                }
            }
        }
    }
    
    None
}

// ============================================================================
// Runtime Statistics Collection
// ============================================================================

/// Classify a temperature value into a bucket
pub fn classify_temperature_bucket(celsius: f32) -> ValueBucket {
    if celsius < 20.0 {
        ValueBucket::VeryCold
    } else if celsius < 35.0 {
        ValueBucket::Cold
    } else if celsius < 50.0 {
        ValueBucket::Warm
    } else if celsius < 70.0 {
        ValueBucket::Hot
    } else {
        ValueBucket::VeryHot
    }
}

/// Update runtime stats with a new sample
pub fn update_runtime_stats(stats: &mut RuntimeStats, value: f32, timestamp_ms: u64) {
    // Update last seen
    stats.last_seen_timestamp = Some(timestamp_ms);
    
    // Update expected range
    if let Some((min, max)) = &mut stats.expected_value_range {
        *min = min.min(value);
        *max = max.max(value);
    } else {
        stats.expected_value_range = Some((value, value));
    }
    
    // Set initial bucket if not set
    if stats.initial_value_bucket.is_none() {
        stats.initial_value_bucket = Some(classify_temperature_bucket(value));
    }
}

impl Default for ChannelType {
    fn default() -> Self {
        Self::Temperature
    }
}
