//! Hardware Anchor Definitions
//!
//! This module defines all anchor types used for zero-drift sensor identification.
//! Anchors are immutable or highly stable hardware identifiers that survive reboots,
//! driver reloads, and hwmon reindexing.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;

use super::validation::*;

// Security limits to prevent DoS attacks
const MAX_STRING_LENGTH: usize = 256;
const MAX_PATH_LENGTH: usize = 4096;
const MAX_RESPONSE_CURVE_POINTS: usize = 256;
const MAX_ATTRIBUTE_FILES: usize = 64;
const MAX_LABEL_LENGTH: usize = 128;

// Timestamp limits (valid until year 2262)
#[allow(dead_code)]
const MAX_TIMESTAMP_MS: u64 = u64::MAX / 2;

// ============================================================================
// Tier 1: Hardware Anchors (Immutable)
// ============================================================================

/// PCI device hardware anchor - most stable identifier for PCI devices
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PciAnchor {
    /// PCI domain:bus:device.function (e.g., "0000:01:00.0")
    pub address: String,
    /// Vendor ID (e.g., "0x1002" for AMD, "0x10de" for NVIDIA)
    pub vendor_id: String,
    /// Device ID
    pub device_id: String,
    /// Subsystem vendor ID (motherboard/card manufacturer)
    pub subsystem_vendor_id: Option<String>,
    /// Subsystem device ID
    pub subsystem_device_id: Option<String>,
    /// PCI class (0x030000 for VGA, etc.)
    pub class: Option<String>,
    /// PCI revision
    pub revision: Option<String>,
}

impl PciAnchor {
    /// Validate PCI anchor data
    pub fn validate(&self) -> Result<(), AnchorValidationError> {
        validate_string_length(&self.address, "PCI address", MAX_STRING_LENGTH)?;
        validate_string_length(&self.vendor_id, "vendor_id", MAX_STRING_LENGTH)?;
        validate_string_length(&self.device_id, "device_id", MAX_STRING_LENGTH)?;
        
        // Validate PCI address format (basic check)
        if !self.address.chars().all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.') {
            return Err(AnchorValidationError::InvalidFormat("PCI address contains invalid characters".to_string()));
        }
        
        // Validate hex IDs
        validate_hex_string(&self.vendor_id, "vendor_id")?;
        validate_hex_string(&self.device_id, "device_id")?;
        
        if let Some(ref s) = self.subsystem_vendor_id {
            validate_string_length(s, "subsystem_vendor_id", MAX_STRING_LENGTH)?;
            validate_hex_string(s, "subsystem_vendor_id")?;
        }
        if let Some(ref s) = self.subsystem_device_id {
            validate_string_length(s, "subsystem_device_id", MAX_STRING_LENGTH)?;
            validate_hex_string(s, "subsystem_device_id")?;
        }
        if let Some(ref s) = self.class {
            validate_string_length(s, "class", MAX_STRING_LENGTH)?;
            validate_hex_string(s, "class")?;
        }
        if let Some(ref s) = self.revision {
            validate_string_length(s, "revision", MAX_STRING_LENGTH)?;
        }
        
        Ok(())
    }
}

/// I2C device hardware anchor - stable for SuperIO and SMBus devices
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct I2cAnchor {
    /// I2C bus number
    pub bus_number: u32,
    /// Device address on bus (7-bit address)
    pub device_address: u8,
    /// I2C adapter name (e.g., "NVIDIA i2c adapter")
    pub adapter_name: String,
    /// Adapter algorithm (if available)
    pub adapter_algo: Option<String>,
}

impl I2cAnchor {
    /// Validate I2C anchor data
    pub fn validate(&self) -> Result<(), AnchorValidationError> {
        // I2C bus numbers should be reasonable (0-255 typical)
        if self.bus_number > 1024 {
            return Err(AnchorValidationError::OutOfRange("I2C bus number too large".to_string()));
        }
        
        // 7-bit I2C address must be 0-127
        if self.device_address > 127 {
            return Err(AnchorValidationError::OutOfRange("I2C address must be 7-bit (0-127)".to_string()));
        }
        
        validate_string_length(&self.adapter_name, "adapter_name", MAX_STRING_LENGTH)?;
        validate_printable_string(&self.adapter_name, "adapter_name")?;
        
        if let Some(ref s) = self.adapter_algo {
            validate_string_length(s, "adapter_algo", MAX_STRING_LENGTH)?;
            validate_printable_string(s, "adapter_algo")?;
        }
        
        Ok(())
    }
}

/// ACPI device hardware anchor - stable for laptop embedded controllers
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AcpiAnchor {
    /// Full ACPI path (e.g., "LNXSYSTM:00/LNXSYBUS:00/PNP0C0A:00")
    pub path: String,
    /// ACPI Hardware ID (HID)
    pub hid: Option<String>,
    /// ACPI Unique ID (UID)
    pub uid: Option<String>,
    /// ACPI Compatible ID (CID)
    pub cid: Option<String>,
}

impl AcpiAnchor {
    /// Validate ACPI anchor data
    pub fn validate(&self) -> Result<(), AnchorValidationError> {
        validate_string_length(&self.path, "ACPI path", MAX_PATH_LENGTH)?;
        validate_printable_string(&self.path, "ACPI path")?;
        
        if let Some(ref s) = self.hid {
            validate_string_length(s, "HID", MAX_STRING_LENGTH)?;
            validate_printable_string(s, "HID")?;
        }
        if let Some(ref s) = self.uid {
            validate_string_length(s, "UID", MAX_STRING_LENGTH)?;
            validate_printable_string(s, "UID")?;
        }
        if let Some(ref s) = self.cid {
            validate_string_length(s, "CID", MAX_STRING_LENGTH)?;
            validate_printable_string(s, "CID")?;
        }
        
        Ok(())
    }
}

/// USB device hardware anchor - for external fan controllers
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UsbAnchor {
    /// USB bus number
    pub bus_number: u16,
    /// USB device address on bus
    pub device_address: u16,
    /// USB vendor ID
    pub vendor_id: String,
    /// USB product ID
    pub product_id: String,
    /// USB serial number (if available)
    pub serial_number: Option<String>,
    /// USB port path (e.g., "1-2.3")
    pub port_path: Option<String>,
}

impl UsbAnchor {
    /// Validate USB anchor data
    pub fn validate(&self) -> Result<(), AnchorValidationError> {
        validate_string_length(&self.vendor_id, "vendor_id", MAX_STRING_LENGTH)?;
        validate_string_length(&self.product_id, "product_id", MAX_STRING_LENGTH)?;
        validate_hex_string(&self.vendor_id, "vendor_id")?;
        validate_hex_string(&self.product_id, "product_id")?;
        
        if let Some(ref s) = self.serial_number {
            validate_string_length(s, "serial_number", MAX_STRING_LENGTH)?;
            validate_printable_string(s, "serial_number")?;
        }
        if let Some(ref s) = self.port_path {
            validate_string_length(s, "port_path", MAX_STRING_LENGTH)?;
            // Port path format: digits, dots, hyphens only
            if !s.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-') {
                return Err(AnchorValidationError::InvalidFormat("USB port path contains invalid characters".to_string()));
            }
        }
        
        Ok(())
    }
}

/// Platform device anchor - for ARM/embedded systems
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlatformAnchor {
    /// Platform device name
    pub device_name: String,
    /// Device tree path (if available)
    pub of_node_path: Option<String>,
    /// Platform device ID
    pub device_id: Option<i32>,
}

impl PlatformAnchor {
    /// Validate platform anchor data
    pub fn validate(&self) -> Result<(), AnchorValidationError> {
        validate_string_length(&self.device_name, "device_name", MAX_STRING_LENGTH)?;
        validate_printable_string(&self.device_name, "device_name")?;
        
        if let Some(ref s) = self.of_node_path {
            validate_string_length(s, "of_node_path", MAX_PATH_LENGTH)?;
            validate_path_string(s, "of_node_path")?;
        }
        
        Ok(())
    }
}

/// Combined hardware anchor - at least one must be present
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareAnchor {
    pub pci: Option<PciAnchor>,
    pub i2c: Option<I2cAnchor>,
    pub acpi: Option<AcpiAnchor>,
    pub usb: Option<UsbAnchor>,
    pub platform: Option<PlatformAnchor>,
}

impl HardwareAnchor {
    /// Check if this anchor has any hardware identifiers
    pub fn has_any(&self) -> bool {
        self.pci.is_some()
            || self.i2c.is_some()
            || self.acpi.is_some()
            || self.usb.is_some()
            || self.platform.is_some()
    }

    /// Get anchor strength (0.0 - 1.0)
    /// Returns the strength of the strongest single anchor, not the sum
    pub fn strength(&self) -> f32 {
        let mut max_strength = 0.0f32;
        
        if self.pci.is_some() {
            max_strength = max_strength.max(0.95); // PCI is strongest
        }
        if self.i2c.is_some() {
            max_strength = max_strength.max(0.85); // I2C is very strong
        }
        if self.acpi.is_some() {
            max_strength = max_strength.max(0.75); // ACPI is strong
        }
        if self.usb.is_some() {
            max_strength = max_strength.max(0.70); // USB is moderately strong
        }
        if self.platform.is_some() {
            max_strength = max_strength.max(0.50); // Platform is weakest
        }
        
        max_strength
    }
    
    /// Validate all anchor data
    pub fn validate(&self) -> Result<(), AnchorValidationError> {
        if let Some(ref pci) = self.pci {
            pci.validate().map_err(|e| AnchorValidationError::NestedError("PCI".to_string(), Box::new(e)))?;
        }
        if let Some(ref i2c) = self.i2c {
            i2c.validate().map_err(|e| AnchorValidationError::NestedError("I2C".to_string(), Box::new(e)))?;
        }
        if let Some(ref acpi) = self.acpi {
            acpi.validate().map_err(|e| AnchorValidationError::NestedError("ACPI".to_string(), Box::new(e)))?;
        }
        if let Some(ref usb) = self.usb {
            usb.validate().map_err(|e| AnchorValidationError::NestedError("USB".to_string(), Box::new(e)))?;
        }
        if let Some(ref platform) = self.platform {
            platform.validate().map_err(|e| AnchorValidationError::NestedError("Platform".to_string(), Box::new(e)))?;
        }
        Ok(())
    }
}

// ============================================================================
// Tier 2: Firmware Anchors (Stable)
// ============================================================================

/// Firmware-provided sensor label anchor
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SensorLabelAnchor {
    /// Raw label text from firmware
    pub raw_label: String,
    /// Normalized label (lowercase, no special chars)
    pub normalized_label: String,
    /// Label hash for quick comparison
    pub label_hash: u64,
}

impl SensorLabelAnchor {
    /// Validate sensor label anchor
    pub fn validate(&self) -> Result<(), AnchorValidationError> {
        validate_string_length(&self.raw_label, "raw_label", MAX_LABEL_LENGTH)?;
        validate_string_length(&self.normalized_label, "normalized_label", MAX_LABEL_LENGTH)?;
        validate_printable_string(&self.raw_label, "raw_label")?;
        
        // Normalized label should only contain lowercase alphanumeric
        if !self.normalized_label.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()) {
            return Err(AnchorValidationError::InvalidFormat(
                "Normalized label must be lowercase alphanumeric only".to_string()
            ));
        }
        
        Ok(())
    }
}

/// DMI/SMBIOS system anchor - for system-level identification
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DmiAnchor {
    /// System manufacturer
    pub sys_vendor: Option<String>,
    /// Product name
    pub product_name: Option<String>,
    /// Product version
    pub product_version: Option<String>,
    /// Board vendor
    pub board_vendor: Option<String>,
    /// Board name
    pub board_name: Option<String>,
    /// BIOS vendor
    pub bios_vendor: Option<String>,
    /// BIOS version
    pub bios_version: Option<String>,
}

impl DmiAnchor {
    /// Validate DMI anchor
    pub fn validate(&self) -> Result<(), AnchorValidationError> {
        let fields = [
            (&self.sys_vendor, "sys_vendor"),
            (&self.product_name, "product_name"),
            (&self.product_version, "product_version"),
            (&self.board_vendor, "board_vendor"),
            (&self.board_name, "board_name"),
            (&self.bios_vendor, "bios_vendor"),
            (&self.bios_version, "bios_version"),
        ];
        
        for (field, name) in &fields {
            if let Some(ref s) = field {
                validate_string_length(s, name, MAX_STRING_LENGTH)?;
                validate_printable_string(s, name)?;
            }
        }
        
        Ok(())
    }
}

/// Combined firmware anchor
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirmwareAnchor {
    pub sensor_label: Option<SensorLabelAnchor>,
    pub dmi: Option<DmiAnchor>,
}

impl FirmwareAnchor {
    pub fn has_any(&self) -> bool {
        self.sensor_label.is_some() || self.dmi.is_some()
    }

    pub fn strength(&self) -> f32 {
        let mut max_strength = 0.0f32;
        if self.sensor_label.is_some() {
            max_strength = max_strength.max(0.90);
        }
        if self.dmi.is_some() {
            max_strength = max_strength.max(0.60);
        }
        max_strength
    }
    
    /// Validate firmware anchor
    pub fn validate(&self) -> Result<(), AnchorValidationError> {
        if let Some(ref label) = self.sensor_label {
            label.validate().map_err(|e| AnchorValidationError::NestedError("SensorLabel".to_string(), Box::new(e)))?;
        }
        if let Some(ref dmi) = self.dmi {
            dmi.validate().map_err(|e| AnchorValidationError::NestedError("DMI".to_string(), Box::new(e)))?;
        }
        Ok(())
    }
}

// ============================================================================
// Tier 3: Driver Anchors (Semi-stable)
// ============================================================================

/// Driver identification anchor
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DriverAnchor {
    /// Driver/module name (e.g., "coretemp", "nct6775")
    pub driver_name: String,
    /// Canonical device symlink path
    pub device_path_canonical: Option<String>,
    /// Modalias string
    pub modalias: Option<String>,
    /// Driver version (if available)
    pub driver_version: Option<String>,
}

impl DriverAnchor {
    pub fn strength(&self) -> f32 {
        let mut score: f32 = 0.5; // Base score for driver name
        if self.device_path_canonical.is_some() {
            score = score.max(0.75);
        }
        if self.modalias.is_some() {
            score = score.max(0.65);
        }
        if self.driver_version.is_some() {
            score = score.max(0.55);
        }
        score.min(1.0)
    }
    
    /// Validate driver anchor
    pub fn validate(&self) -> Result<(), AnchorValidationError> {
        validate_string_length(&self.driver_name, "driver_name", MAX_STRING_LENGTH)?;
        validate_printable_string(&self.driver_name, "driver_name")?;
        
        // Driver name should be alphanumeric with underscores/hyphens
        if !self.driver_name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
            return Err(AnchorValidationError::InvalidFormat(
                "Driver name contains invalid characters".to_string()
            ));
        }
        
        if let Some(ref s) = self.device_path_canonical {
            validate_string_length(s, "device_path_canonical", MAX_PATH_LENGTH)?;
            validate_path_string(s, "device_path_canonical")?;
        }
        if let Some(ref s) = self.modalias {
            validate_string_length(s, "modalias", MAX_STRING_LENGTH)?;
            validate_printable_string(s, "modalias")?;
        }
        if let Some(ref s) = self.driver_version {
            validate_string_length(s, "driver_version", MAX_STRING_LENGTH)?;
            validate_printable_string(s, "driver_version")?;
        }
        
        Ok(())
    }
}

// ============================================================================
// Tier 4: Attribute Anchors (Validation)
// ============================================================================

/// Sensor attribute fingerprint for validation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttributeAnchor {
    /// Set of attribute files present (e.g., "_input", "_label", "_enable")
    pub attribute_files: HashSet<String>,
    /// Sensor capabilities
    pub capabilities: SensorCapabilities,
    /// Expected value range (for validation)
    pub expected_range: Option<(i64, i64)>,
}

/// Sensor capability flags
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SensorCapabilities {
    /// Has input file
    pub has_input: bool,
    /// Has label file
    pub has_label: bool,
    /// Has enable file (for PWM)
    pub has_enable: bool,
    /// Is writable (for PWM)
    pub is_writable: bool,
    /// Has min/max/crit files
    pub has_limits: bool,
    /// Has alarm file
    pub has_alarm: bool,
}

impl AttributeAnchor {
    pub fn strength(&self) -> f32 {
        let mut score = 0.0;
        let attr_count = self.attribute_files.len();
        
        // More attributes = stronger fingerprint
        score += (attr_count as f32 * 0.05).min(0.4);
        
        // Key capabilities add strength
        if self.capabilities.has_label {
            score += 0.3;
        }
        if self.capabilities.has_enable {
            score += 0.15;
        }
        if self.expected_range.is_some() {
            score += 0.15;
        }
        
        score.min(1.0)
    }
    
    /// Validate attribute anchor
    pub fn validate(&self) -> Result<(), AnchorValidationError> {
        validate_collection_size(
            self.attribute_files.len(),
            "attribute_files",
            MAX_ATTRIBUTE_FILES
        )?;
        
        // Validate each attribute name
        for attr in &self.attribute_files {
            validate_string_length(attr, "attribute_file", MAX_STRING_LENGTH)?;
            // Attribute names should start with underscore and be alphanumeric
            if !attr.starts_with('_') {
                return Err(AnchorValidationError::InvalidFormat(
                    "Attribute file name must start with underscore".to_string()
                ));
            }
            if !attr.chars().all(|c| c.is_alphanumeric() || c == '_') {
                return Err(AnchorValidationError::InvalidFormat(
                    "Attribute file name contains invalid characters".to_string()
                ));
            }
        }
        
        // Validate expected range
        if let Some((min, max)) = self.expected_range {
            if min > max {
                return Err(AnchorValidationError::InvalidFormat(
                    "Expected range min > max".to_string()
                ));
            }
        }
        
        Ok(())
    }
}

// ============================================================================
// Tier 5: Runtime Anchors (Behavioral)
// ============================================================================

/// Runtime behavioral anchor - PWM-to-fan response signature
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAnchor {
    /// PWM-to-RPM response curve (PWM value -> RPM reading)
    pub response_curve: Vec<(u8, u32)>,
    /// Response time in milliseconds
    pub response_time_ms: u32,
    /// RPM variance at steady state
    pub rpm_variance: f32,
    /// Minimum controllable PWM value
    pub min_pwm: Option<u8>,
    /// Maximum observed RPM
    pub max_rpm: Option<u32>,
    /// Response signature hash (for quick comparison)
    pub signature_hash: u64,
}

impl RuntimeAnchor {
    pub fn strength(&self) -> f32 {
        let mut score = 0.0;
        
        if !self.response_curve.is_empty() {
            score += 0.5;
        }
        if self.min_pwm.is_some() {
            score += 0.2;
        }
        if self.max_rpm.is_some() {
            score += 0.2;
        }
        if self.rpm_variance > 0.0 && self.rpm_variance.is_finite() {
            score += 0.1;
        }
        
        score
    }
    
    /// Validate runtime anchor
    pub fn validate(&self) -> Result<(), AnchorValidationError> {
        validate_collection_size(
            self.response_curve.len(),
            "response_curve",
            MAX_RESPONSE_CURVE_POINTS
        )?;
        
        // Validate response curve points
        for (_pwm, rpm) in &self.response_curve {
            // PWM values are 0-255
            // RPM values should be reasonable (0-20000 typical)
            if *rpm > 50000 {
                return Err(AnchorValidationError::OutOfRange(
                    "RPM value suspiciously high (>50000)".to_string()
                ));
            }
        }
        
        // Validate response time is reasonable (0-60 seconds)
        if self.response_time_ms > 60000 {
            return Err(AnchorValidationError::OutOfRange(
                "Response time too large (>60s)".to_string()
            ));
        }
        
        // Validate variance is finite and non-negative
        if !self.rpm_variance.is_finite() || self.rpm_variance < 0.0 {
            return Err(AnchorValidationError::InvalidFormat(
                "RPM variance must be finite and non-negative".to_string()
            ));
        }
        
        // Validate max RPM is reasonable
        if let Some(rpm) = self.max_rpm {
            if rpm > 50000 {
                return Err(AnchorValidationError::OutOfRange(
                    "Max RPM suspiciously high (>50000)".to_string()
                ));
            }
        }
        
        Ok(())
    }
}

// ============================================================================
// Complete Fingerprints
// ============================================================================

/// Complete chip fingerprint with all anchor tiers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChipFingerprint {
    /// Unique fingerprint ID (hash of all anchors)
    pub id: String,
    
    /// Tier 1: Hardware anchors (immutable)
    pub hardware: HardwareAnchor,
    
    /// Tier 2: Firmware anchors (stable)
    pub firmware: FirmwareAnchor,
    
    /// Tier 3: Driver anchors (semi-stable)
    pub driver: DriverAnchor,
    
    /// Chip classification
    pub chip_class: ChipClass,
    
    /// Original hwmon path at discovery (for reference only, NOT used for matching)
    pub original_hwmon_path: PathBuf,
    
    /// Creation timestamp
    pub created_at: u64,
    
    /// Last validation timestamp
    pub last_validated_at: Option<u64>,
}

impl ChipFingerprint {
    /// Validate chip fingerprint
    pub fn validate(&self) -> Result<(), AnchorValidationError> {
        validate_string_length(&self.id, "id", MAX_STRING_LENGTH)?;
        
        // ID should be hexadecimal
        if !self.id.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(AnchorValidationError::InvalidFormat(
                "Fingerprint ID must be hexadecimal".to_string()
            ));
        }
        
        // Validate all anchor tiers
        self.hardware.validate().map_err(|e| 
            AnchorValidationError::NestedError("Hardware".to_string(), Box::new(e))
        )?;
        self.firmware.validate().map_err(|e| 
            AnchorValidationError::NestedError("Firmware".to_string(), Box::new(e))
        )?;
        self.driver.validate().map_err(|e| 
            AnchorValidationError::NestedError("Driver".to_string(), Box::new(e))
        )?;
        
        // Validate path
        validate_pathbuf(&self.original_hwmon_path, "original_hwmon_path")?;
        
        // Validate timestamps
        validate_timestamp(self.created_at, "created_at")?;
        if let Some(ts) = self.last_validated_at {
            validate_timestamp(ts, "last_validated_at")?;
            
            // Last validated should not be before creation
            if ts < self.created_at {
                return Err(AnchorValidationError::InvalidFormat(
                    "last_validated_at is before created_at".to_string()
                ));
            }
        }
        
        Ok(())
    }
}

/// Complete channel (sensor) fingerprint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelFingerprint {
    /// Unique fingerprint ID
    pub id: String,
    
    /// Parent chip fingerprint ID
    pub chip_id: String,
    
    /// Channel type
    pub channel_type: ChannelType,
    
    /// Tier 2: Firmware anchor (sensor label)
    pub firmware: FirmwareAnchor,
    
    /// Tier 4: Attribute anchor
    pub attributes: AttributeAnchor,
    
    /// Semantic classification
    pub semantic_role: SemanticRole,
    
    /// Original sensor name (e.g., "temp1", "fan2") - NOT used for matching
    pub original_name: String,
    
    /// Original path - NOT used for matching
    pub original_path: PathBuf,
    
    /// Creation timestamp
    pub created_at: u64,
}

impl ChannelFingerprint {
    /// Validate channel fingerprint
    pub fn validate(&self) -> Result<(), AnchorValidationError> {
        validate_string_length(&self.id, "id", MAX_STRING_LENGTH)?;
        validate_string_length(&self.chip_id, "chip_id", MAX_STRING_LENGTH)?;
        
        // IDs should be hexadecimal
        if !self.id.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(AnchorValidationError::InvalidFormat(
                "Channel ID must be hexadecimal".to_string()
            ));
        }
        if !self.chip_id.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(AnchorValidationError::InvalidFormat(
                "Chip ID must be hexadecimal".to_string()
            ));
        }
        
        // Validate anchors
        self.firmware.validate().map_err(|e| 
            AnchorValidationError::NestedError("Firmware".to_string(), Box::new(e))
        )?;
        self.attributes.validate().map_err(|e| 
            AnchorValidationError::NestedError("Attributes".to_string(), Box::new(e))
        )?;
        
        // Validate original name (should be alphanumeric)
        validate_string_length(&self.original_name, "original_name", MAX_STRING_LENGTH)?;
        if !self.original_name.chars().all(|c| c.is_alphanumeric()) {
            return Err(AnchorValidationError::InvalidFormat(
                "Original name must be alphanumeric".to_string()
            ));
        }
        
        // Validate path
        validate_pathbuf(&self.original_path, "original_path")?;
        
        // Validate timestamp
        validate_timestamp(self.created_at, "created_at")?;
        
        Ok(())
    }
}

/// Complete PWM channel fingerprint with fan pairing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PwmChannelFingerprint {
    /// Base channel fingerprint
    pub channel: ChannelFingerprint,
    
    /// Paired fan channel fingerprint ID
    pub paired_fan_id: Option<String>,
    
    /// Tier 5: Runtime behavioral anchor
    pub runtime: Option<RuntimeAnchor>,
    
    /// PWM-specific capabilities
    pub pwm_capabilities: PwmCapabilities,
    
    /// Safe fallback policy if validation fails
    pub safe_fallback: SafeFallbackPolicy,
}

impl PwmChannelFingerprint {
    /// Validate PWM channel fingerprint
    pub fn validate(&self) -> Result<(), AnchorValidationError> {
        // Validate base channel
        self.channel.validate().map_err(|e| 
            AnchorValidationError::NestedError("Channel".to_string(), Box::new(e))
        )?;
        
        // Channel type must be PWM
        if self.channel.channel_type != ChannelType::Pwm {
            return Err(AnchorValidationError::InvalidFormat(
                "PWM channel fingerprint must have PWM channel type".to_string()
            ));
        }
        
        // Validate paired fan ID if present
        if let Some(ref fan_id) = self.paired_fan_id {
            validate_string_length(fan_id, "paired_fan_id", MAX_STRING_LENGTH)?;
            if !fan_id.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(AnchorValidationError::InvalidFormat(
                    "Paired fan ID must be hexadecimal".to_string()
                ));
            }
        }
        
        // Validate runtime anchor if present
        if let Some(ref runtime) = self.runtime {
            runtime.validate().map_err(|e| 
                AnchorValidationError::NestedError("Runtime".to_string(), Box::new(e))
            )?;
        }
        
        Ok(())
    }
}

/// PWM-specific capabilities
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PwmCapabilities {
    /// Has pwmX_enable file
    pub has_enable: bool,
    /// Is writable
    pub is_writable: bool,
    /// Has corresponding fanX_input
    pub has_rpm_feedback: bool,
    /// Detected control authority (BIOS vs manual)
    pub control_authority: ControlAuthority,
}

/// Control authority state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlAuthority {
    /// Manual control available (pwm_enable = 1)
    Manual,
    /// BIOS/EC controlled (pwm_enable = 0 or 2+)
    Automatic,
    /// No enable file - BIOS controlled
    BiosOnly,
    /// Unknown state
    Unknown,
}

/// Safe fallback policy when validation fails
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SafeFallbackPolicy {
    /// Set to 100% speed (safest)
    FullSpeed,
    /// Set to 75% speed
    HighSpeed,
    /// Set to 50% speed
    MediumSpeed,
    /// Restore automatic control
    RestoreAutomatic,
    /// Do nothing (dangerous - only if user explicitly configured)
    DoNothing,
}

impl Default for SafeFallbackPolicy {
    fn default() -> Self {
        Self::FullSpeed
    }
}

// ============================================================================
// Enums
// ============================================================================

/// Hardware chip classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChipClass {
    /// CPU temperature sensor (coretemp, k10temp)
    CpuTemp,
    /// Discrete GPU (amdgpu, nvidia, nouveau)
    DiscreteGpu,
    /// Integrated GPU (i915, xe)
    IntegratedGpu,
    /// Laptop embedded controller (thinkpad_hwmon, dell-smm)
    EmbeddedController,
    /// SuperIO chip (nct6775, it87, w83627)
    SuperIo,
    /// ACPI thermal zone
    AcpiThermal,
    /// NVMe drive
    NvmeDrive,
    /// SATA/SAS drive
    SataDrive,
    /// Chipset sensor
    Chipset,
    /// Unknown classification
    Unknown,
}

/// Sensor channel type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChannelType {
    Temperature,
    Fan,
    Pwm,
    Voltage,
    Power,
    Current,
    Energy,
    Humidity,
}

/// Semantic role of a sensor
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SemanticRole {
    CpuPackage,
    CpuCore,
    CpuDie,
    GpuCore,
    GpuHotspot,
    GpuMemory,
    GpuVrm,
    Chipset,
    Motherboard,
    VrmCpu,
    VrmGpu,
    NvmeDrive,
    SataDrive,
    CaseFan,
    CpuFan,
    GpuFan,
    ChipsetFan,
    AmbientTemp,
    Unknown,
}

impl Default for SemanticRole {
    fn default() -> Self {
        Self::Unknown
    }
}
