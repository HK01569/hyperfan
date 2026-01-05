//! Zero-Drift Matching Engine
//!
//! This module implements the core matching algorithm that finds sensors in the
//! current system state based on stored fingerprints. It guarantees zero drift by
//! using multiple independent anchor layers and never relying on hwmon indices.

use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, trace};

use super::anchors::*;

// ============================================================================
// Match Results
// ============================================================================

/// Result of matching a fingerprint to current hardware
#[derive(Debug, Clone)]
pub struct MatchResult {
    /// Matched hwmon path (e.g., /sys/class/hwmon/hwmon3)
    pub hwmon_path: PathBuf,
    /// Matched sensor path (e.g., /sys/class/hwmon/hwmon3/temp1_input)
    pub sensor_path: Option<PathBuf>,
    /// Match confidence (0.0 - 1.0)
    pub confidence: MatchConfidence,
    /// Detailed reasons for confidence score
    pub reasons: Vec<MatchReason>,
    /// Which anchor tiers matched
    pub matched_tiers: Vec<AnchorTier>,
}

/// Confidence breakdown by anchor tier
#[derive(Debug, Clone)]
pub struct MatchConfidence {
    /// Overall confidence score (0.0 - 1.0)
    pub overall: f32,
    /// Tier 1 (Hardware) confidence
    pub hardware: f32,
    /// Tier 2 (Firmware) confidence
    pub firmware: f32,
    /// Tier 3 (Driver) confidence
    pub driver: f32,
    /// Tier 4 (Attributes) confidence
    pub attributes: f32,
    /// Tier 5 (Runtime) confidence
    pub runtime: f32,
}

impl MatchConfidence {
    /// Check if confidence is sufficient for safe fan control
    pub fn is_safe_for_control(&self) -> bool {
        self.overall >= super::MIN_CONFIDENCE_FOR_CONTROL
    }

    /// Check if confidence warrants a warning
    pub fn should_warn(&self) -> bool {
        self.overall < super::CONFIDENCE_WARNING_THRESHOLD
    }

    /// Check if in degraded state
    pub fn is_degraded(&self) -> bool {
        self.overall < super::CONFIDENCE_DEGRADED_THRESHOLD
    }
}

/// Reason for confidence score
#[derive(Debug, Clone)]
pub struct MatchReason {
    pub tier: AnchorTier,
    pub message: String,
    pub impact: f32,
}

/// Anchor tier that participated in matching
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorTier {
    Hardware,
    Firmware,
    Driver,
    Attributes,
    Runtime,
}

// ============================================================================
// Chip Matching
// ============================================================================

/// Find a chip in the current system by its fingerprint
///
/// This function scans /sys/class/hwmon and matches chips using the multi-tier
/// anchor system. It NEVER relies on hwmon index numbers.
pub fn find_chip_by_fingerprint(
    fingerprint: &ChipFingerprint,
) -> Result<MatchResult, MatchError> {
    let hwmon_base = Path::new("/sys/class/hwmon");
    
    if !hwmon_base.exists() {
        return Err(MatchError::HwmonNotAvailable);
    }

    let entries = fs::read_dir(hwmon_base)
        .map_err(|e| MatchError::IoError(format!("Failed to read hwmon: {}", e)))?;

    let mut best_match: Option<MatchResult> = None;

    for entry in entries.flatten() {
        let hwmon_path = entry.path();
        
        trace!(path = ?hwmon_path, "Checking hwmon device");

        let match_result = match_chip_at_path(fingerprint, &hwmon_path);
        
        if let Some(result) = match_result {
            // Keep the best match
            if let Some(ref current_best) = best_match {
                if result.confidence.overall > current_best.confidence.overall {
                    best_match = Some(result);
                }
            } else {
                best_match = Some(result);
            }
        }
    }

    best_match.ok_or(MatchError::NoMatch)
}

/// Match a chip fingerprint against a specific hwmon path
fn match_chip_at_path(
    fingerprint: &ChipFingerprint,
    hwmon_path: &Path,
) -> Option<MatchResult> {
    let mut confidence = MatchConfidence {
        overall: 0.0,
        hardware: 0.0,
        firmware: 0.0,
        driver: 0.0,
        attributes: 0.0,
        runtime: 0.0,
    };
    let mut reasons = Vec::new();
    let mut matched_tiers = Vec::new();
    let mut total_weight = 0.0;
    let mut weighted_score = 0.0;

    // Tier 1: Hardware anchors (highest weight)
    let hw_weight = 0.40;
    let hw_score = match_hardware_anchor(&fingerprint.hardware, hwmon_path, &mut reasons);
    if hw_score > 0.0 {
        confidence.hardware = hw_score;
        weighted_score += hw_score * hw_weight;
        total_weight += hw_weight;
        matched_tiers.push(AnchorTier::Hardware);
    }

    // Tier 2: Firmware anchors
    let fw_weight = 0.25;
    let fw_score = match_firmware_anchor(&fingerprint.firmware, hwmon_path, &mut reasons);
    if fw_score > 0.0 {
        confidence.firmware = fw_score;
        weighted_score += fw_score * fw_weight;
        total_weight += fw_weight;
        matched_tiers.push(AnchorTier::Firmware);
    }

    // Tier 3: Driver anchors
    let drv_weight = 0.20;
    let drv_score = match_driver_anchor(&fingerprint.driver, hwmon_path, &mut reasons);
    if drv_score > 0.0 {
        confidence.driver = drv_score;
        weighted_score += drv_score * drv_weight;
        total_weight += drv_weight;
        matched_tiers.push(AnchorTier::Driver);
    }

    // Calculate overall confidence
    confidence.overall = if total_weight > 0.0 {
        weighted_score / total_weight
    } else {
        0.0
    };

    // Require minimum confidence to return a match
    if confidence.overall < 0.5 {
        return None;
    }

    debug!(
        path = ?hwmon_path,
        confidence = format!("{:.2}", confidence.overall),
        tiers = ?matched_tiers,
        "Chip match found"
    );

    Some(MatchResult {
        hwmon_path: hwmon_path.to_path_buf(),
        sensor_path: None,
        confidence,
        reasons,
        matched_tiers,
    })
}

// ============================================================================
// Channel Matching
// ============================================================================

/// Find a channel (sensor) within a chip by its fingerprint
pub fn find_channel_by_fingerprint(
    channel_fp: &ChannelFingerprint,
    chip_path: &Path,
) -> Result<MatchResult, MatchError> {
    let entries = fs::read_dir(chip_path)
        .map_err(|e| MatchError::IoError(format!("Failed to read chip path: {}", e)))?;

    let mut best_match: Option<MatchResult> = None;

    // Build list of candidate sensors
    let mut candidates = Vec::new();
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();

        // Match by channel type
        let is_candidate = match channel_fp.channel_type {
            ChannelType::Temperature => name_str.starts_with("temp") && name_str.ends_with("_input"),
            ChannelType::Fan => name_str.starts_with("fan") && name_str.ends_with("_input"),
            ChannelType::Pwm => name_str.starts_with("pwm") && !name_str.contains('_'),
            ChannelType::Voltage => name_str.starts_with("in") && name_str.ends_with("_input"),
            ChannelType::Power => name_str.starts_with("power") && name_str.ends_with("_input"),
            ChannelType::Current => name_str.starts_with("curr") && name_str.ends_with("_input"),
            _ => false,
        };

        if is_candidate {
            let base_name = if channel_fp.channel_type == ChannelType::Pwm {
                name_str.to_string()
            } else {
                name_str.trim_end_matches("_input").to_string()
            };
            candidates.push((base_name, entry.path()));
        }
    }

    // Match each candidate
    for (base_name, sensor_path) in candidates {
        let match_result = match_channel_at_path(channel_fp, chip_path, &base_name, &sensor_path);
        
        if let Some(result) = match_result {
            if let Some(ref current_best) = best_match {
                if result.confidence.overall > current_best.confidence.overall {
                    best_match = Some(result);
                }
            } else {
                best_match = Some(result);
            }
        }
    }

    best_match.ok_or(MatchError::NoMatch)
}

/// Match a channel fingerprint against a specific sensor
fn match_channel_at_path(
    fingerprint: &ChannelFingerprint,
    chip_path: &Path,
    base_name: &str,
    sensor_path: &Path,
) -> Option<MatchResult> {
    let mut confidence = MatchConfidence {
        overall: 0.0,
        hardware: 0.0,
        firmware: 0.0,
        driver: 0.0,
        attributes: 0.0,
        runtime: 0.0,
    };
    let mut reasons = Vec::new();
    let mut matched_tiers = Vec::new();
    let mut total_weight = 0.0;
    let mut weighted_score = 0.0;

    // Tier 2: Firmware anchors (sensor label) - highest weight for channels
    let fw_weight = 0.50;
    let fw_score = match_channel_firmware_anchor(&fingerprint.firmware, chip_path, base_name, &mut reasons);
    if fw_score > 0.0 {
        confidence.firmware = fw_score;
        weighted_score += fw_score * fw_weight;
        total_weight += fw_weight;
        matched_tiers.push(AnchorTier::Firmware);
    }

    // Tier 4: Attribute anchors
    let attr_weight = 0.50;
    let attr_score = match_attribute_anchor(&fingerprint.attributes, chip_path, base_name, &mut reasons);
    if attr_score > 0.0 {
        confidence.attributes = attr_score;
        weighted_score += attr_score * attr_weight;
        total_weight += attr_weight;
        matched_tiers.push(AnchorTier::Attributes);
    }

    // Calculate overall confidence
    confidence.overall = if total_weight > 0.0 {
        weighted_score / total_weight
    } else {
        0.0
    };

    // Require minimum confidence
    if confidence.overall < 0.5 {
        return None;
    }

    trace!(
        sensor = base_name,
        confidence = format!("{:.2}", confidence.overall),
        "Channel match found"
    );

    Some(MatchResult {
        hwmon_path: chip_path.to_path_buf(),
        sensor_path: Some(sensor_path.to_path_buf()),
        confidence,
        reasons,
        matched_tiers,
    })
}

// ============================================================================
// Anchor Matching Functions
// ============================================================================

/// Match hardware anchor (PCI, I2C, ACPI, USB)
fn match_hardware_anchor(
    anchor: &HardwareAnchor,
    hwmon_path: &Path,
    reasons: &mut Vec<MatchReason>,
) -> f32 {
    if !anchor.has_any() {
        return 0.0;
    }

    let mut score = 0.0;
    let mut max_score = 0.0;

    // Try to extract current hardware identifiers
    let device_path = hwmon_path.join("device");
    if !device_path.exists() {
        return 0.0;
    }

    // Match PCI anchor
    if let Some(expected_pci) = &anchor.pci {
        max_score += 1.0;
        if let Some(current_pci) = extract_pci_identity(&device_path) {
            let pci_score = compare_pci_anchors(expected_pci, &current_pci);
            score += pci_score;
            
            if pci_score > 0.9 {
                reasons.push(MatchReason {
                    tier: AnchorTier::Hardware,
                    message: format!("PCI device matched: {}", expected_pci.address),
                    impact: pci_score,
                });
            }
        }
    }

    // Match I2C anchor
    if let Some(expected_i2c) = &anchor.i2c {
        max_score += 1.0;
        if let Some(current_i2c) = extract_i2c_identity(&device_path) {
            let i2c_score = compare_i2c_anchors(expected_i2c, &current_i2c);
            score += i2c_score;
            
            if i2c_score > 0.9 {
                reasons.push(MatchReason {
                    tier: AnchorTier::Hardware,
                    message: format!("I2C device matched: bus {} addr 0x{:02x}", 
                                   expected_i2c.bus_number, expected_i2c.device_address),
                    impact: i2c_score,
                });
            }
        }
    }

    // Match ACPI anchor
    if let Some(expected_acpi) = &anchor.acpi {
        max_score += 1.0;
        if let Some(current_acpi) = extract_acpi_identity(&device_path) {
            let acpi_score = compare_acpi_anchors(expected_acpi, &current_acpi);
            score += acpi_score;
            
            if acpi_score > 0.9 {
                reasons.push(MatchReason {
                    tier: AnchorTier::Hardware,
                    message: format!("ACPI device matched: {}", expected_acpi.path),
                    impact: acpi_score,
                });
            }
        }
    }

    if max_score > 0.0 {
        score / max_score
    } else {
        0.0
    }
}

/// Match firmware anchor (sensor labels)
fn match_firmware_anchor(
    anchor: &FirmwareAnchor,
    hwmon_path: &Path,
    reasons: &mut Vec<MatchReason>,
) -> f32 {
    if !anchor.has_any() {
        return 0.0;
    }

    // For chip-level matching, we just check if DMI matches (system identification)
    if let Some(expected_dmi) = &anchor.dmi {
        if let Some(current_dmi) = extract_dmi_identity() {
            let dmi_score = compare_dmi_anchors(expected_dmi, &current_dmi);
            if dmi_score > 0.8 {
                reasons.push(MatchReason {
                    tier: AnchorTier::Firmware,
                    message: "System DMI matched".to_string(),
                    impact: dmi_score,
                });
                return dmi_score;
            }
        }
    }

    0.0
}

/// Match firmware anchor for a specific channel (sensor label)
fn match_channel_firmware_anchor(
    anchor: &FirmwareAnchor,
    chip_path: &Path,
    base_name: &str,
    reasons: &mut Vec<MatchReason>,
) -> f32 {
    if let Some(expected_label) = &anchor.sensor_label {
        let label_path = chip_path.join(format!("{}_label", base_name));
        
        if let Ok(current_label_raw) = fs::read_to_string(&label_path) {
            let current_label = current_label_raw.trim();
            let current_normalized = normalize_label(current_label);
            
            // Exact match on normalized label
            if current_normalized == expected_label.normalized_label {
                reasons.push(MatchReason {
                    tier: AnchorTier::Firmware,
                    message: format!("Sensor label matched: '{}'", current_label),
                    impact: 1.0,
                });
                return 1.0;
            }
            
            // Partial match
            if current_normalized.contains(&expected_label.normalized_label) 
                || expected_label.normalized_label.contains(&current_normalized) {
                reasons.push(MatchReason {
                    tier: AnchorTier::Firmware,
                    message: format!("Sensor label partial match: '{}'", current_label),
                    impact: 0.7,
                });
                return 0.7;
            }
        }
    }

    0.0
}

/// Match driver anchor
fn match_driver_anchor(
    anchor: &DriverAnchor,
    hwmon_path: &Path,
    reasons: &mut Vec<MatchReason>,
) -> f32 {
    let mut score = 0.0;
    let mut max_score = 0.0;

    // Match driver name
    max_score += 1.0;
    let name_path = hwmon_path.join("name");
    if let Ok(current_name) = fs::read_to_string(&name_path) {
        if current_name.trim() == anchor.driver_name {
            score += 1.0;
            reasons.push(MatchReason {
                tier: AnchorTier::Driver,
                message: format!("Driver name matched: {}", anchor.driver_name),
                impact: 1.0,
            });
        }
    }

    // Match canonical device path
    if let Some(expected_path) = &anchor.device_path_canonical {
        max_score += 1.0;
        let device_path = hwmon_path.join("device");
        if let Ok(current_path) = fs::canonicalize(&device_path) {
            let current_str = current_path.to_string_lossy();
            if current_str == *expected_path {
                score += 1.0;
                reasons.push(MatchReason {
                    tier: AnchorTier::Driver,
                    message: "Device path matched".to_string(),
                    impact: 1.0,
                });
            }
        }
    }

    if max_score > 0.0 {
        score / max_score
    } else {
        0.0
    }
}

/// Match attribute anchor
fn match_attribute_anchor(
    anchor: &AttributeAnchor,
    chip_path: &Path,
    base_name: &str,
    reasons: &mut Vec<MatchReason>,
) -> f32 {
    let current_attrs = scan_sensor_attributes(chip_path, base_name);
    
    let expected_count = anchor.attribute_files.len();
    let matched_count = anchor.attribute_files.intersection(&current_attrs).count();
    
    if expected_count == 0 {
        return 0.0;
    }

    let score = matched_count as f32 / expected_count as f32;
    
    if score > 0.8 {
        reasons.push(MatchReason {
            tier: AnchorTier::Attributes,
            message: format!("Attributes matched: {}/{}", matched_count, expected_count),
            impact: score,
        });
    } else if score > 0.5 {
        let missing: Vec<_> = anchor.attribute_files.difference(&current_attrs).collect();
        reasons.push(MatchReason {
            tier: AnchorTier::Attributes,
            message: format!("Partial attribute match: {}/{}, missing: {:?}", 
                           matched_count, expected_count, missing),
            impact: score,
        });
    }

    score
}

// ============================================================================
// Hardware Identity Extraction
// ============================================================================

fn extract_pci_identity(device_path: &Path) -> Option<PciAnchor> {
    let real_path = fs::canonicalize(device_path).ok()?;
    let path_str = real_path.to_string_lossy();
    
    if !path_str.contains("/pci") {
        return None;
    }

    let address = extract_pci_address(&path_str)?;
    let vendor_id = read_sysfs_hex(device_path, "vendor")?;
    let device_id = read_sysfs_hex(device_path, "device")?;
    let subsystem_vendor_id = read_sysfs_hex(device_path, "subsystem_vendor");
    let subsystem_device_id = read_sysfs_hex(device_path, "subsystem_device");
    let class = read_sysfs_hex(device_path, "class");
    let revision = read_sysfs_hex(device_path, "revision");

    Some(PciAnchor {
        address,
        vendor_id,
        device_id,
        subsystem_vendor_id,
        subsystem_device_id,
        class,
        revision,
    })
}

fn extract_i2c_identity(device_path: &Path) -> Option<I2cAnchor> {
    let real_path = fs::canonicalize(device_path).ok()?;
    let path_str = real_path.to_string_lossy();
    
    if !path_str.contains("/i2c-") {
        return None;
    }

    let (bus_number, device_address) = extract_i2c_info(&path_str)?;
    let adapter_name = read_i2c_adapter_name(bus_number)?;

    Some(I2cAnchor {
        bus_number,
        device_address,
        adapter_name,
        adapter_algo: None,
    })
}

fn extract_acpi_identity(device_path: &Path) -> Option<AcpiAnchor> {
    let real_path = fs::canonicalize(device_path).ok()?;
    let path_str = real_path.to_string_lossy();
    
    if !path_str.contains("LNXSYSTM") && !path_str.contains("ACPI") {
        return None;
    }

    let hid = read_sysfs_attr(device_path, "hid");
    let uid = read_sysfs_attr(device_path, "uid");
    let cid = read_sysfs_attr(device_path, "cid");

    Some(AcpiAnchor {
        path: path_str.to_string(),
        hid,
        uid,
        cid,
    })
}

fn extract_dmi_identity() -> Option<DmiAnchor> {
    let dmi_base = Path::new("/sys/class/dmi/id");
    if !dmi_base.exists() {
        return None;
    }

    Some(DmiAnchor {
        sys_vendor: read_sysfs_attr(dmi_base, "sys_vendor"),
        product_name: read_sysfs_attr(dmi_base, "product_name"),
        product_version: read_sysfs_attr(dmi_base, "product_version"),
        board_vendor: read_sysfs_attr(dmi_base, "board_vendor"),
        board_name: read_sysfs_attr(dmi_base, "board_name"),
        bios_vendor: read_sysfs_attr(dmi_base, "bios_vendor"),
        bios_version: read_sysfs_attr(dmi_base, "bios_version"),
    })
}

// ============================================================================
// Comparison Functions
// ============================================================================

fn compare_pci_anchors(expected: &PciAnchor, current: &PciAnchor) -> f32 {
    let mut score = 0.0;
    let mut max_score = 0.0;

    // Address match (highest priority)
    max_score += 3.0;
    if expected.address == current.address {
        score += 3.0;
    }

    // Vendor ID match
    max_score += 2.0;
    if expected.vendor_id == current.vendor_id {
        score += 2.0;
    }

    // Device ID match
    max_score += 2.0;
    if expected.device_id == current.device_id {
        score += 2.0;
    }

    // Subsystem IDs (lower priority)
    if expected.subsystem_vendor_id.is_some() {
        max_score += 1.0;
        if expected.subsystem_vendor_id == current.subsystem_vendor_id {
            score += 1.0;
        }
    }

    if expected.subsystem_device_id.is_some() {
        max_score += 1.0;
        if expected.subsystem_device_id == current.subsystem_device_id {
            score += 1.0;
        }
    }

    score / max_score
}

fn compare_i2c_anchors(expected: &I2cAnchor, current: &I2cAnchor) -> f32 {
    let mut score = 0.0;
    let mut max_score = 0.0;

    // Bus number
    max_score += 1.0;
    if expected.bus_number == current.bus_number {
        score += 1.0;
    }

    // Device address
    max_score += 1.0;
    if expected.device_address == current.device_address {
        score += 1.0;
    }

    // Adapter name
    max_score += 1.0;
    if expected.adapter_name == current.adapter_name {
        score += 1.0;
    }

    score / max_score
}

fn compare_acpi_anchors(expected: &AcpiAnchor, current: &AcpiAnchor) -> f32 {
    let mut score = 0.0;
    let mut max_score = 0.0;

    // Path match
    max_score += 2.0;
    if expected.path == current.path {
        score += 2.0;
    }

    // HID match
    if expected.hid.is_some() {
        max_score += 1.0;
        if expected.hid == current.hid {
            score += 1.0;
        }
    }

    // UID match
    if expected.uid.is_some() {
        max_score += 1.0;
        if expected.uid == current.uid {
            score += 1.0;
        }
    }

    score / max_score
}

fn compare_dmi_anchors(expected: &DmiAnchor, current: &DmiAnchor) -> f32 {
    let mut matches = 0;
    let mut total = 0;

    let fields = [
        (&expected.sys_vendor, &current.sys_vendor),
        (&expected.product_name, &current.product_name),
        (&expected.board_vendor, &current.board_vendor),
        (&expected.board_name, &current.board_name),
    ];

    for (exp, cur) in &fields {
        if exp.is_some() {
            total += 1;
            if exp == cur {
                matches += 1;
            }
        }
    }

    if total > 0 {
        matches as f32 / total as f32
    } else {
        0.0
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

fn extract_pci_address(path: &str) -> Option<String> {
    use regex::Regex;
    let re = Regex::new(r"([0-9a-fA-F]{4}:[0-9a-fA-F]{2}:[0-9a-fA-F]{2}\.[0-9a-fA-F])").ok()?;
    re.captures(path).map(|c| c[1].to_string())
}

fn extract_i2c_info(path: &str) -> Option<(u32, u8)> {
    use regex::Regex;
    let re = Regex::new(r"i2c-(\d+)/(\d+)-([0-9a-fA-F]{4})").ok()?;
    let caps = re.captures(path)?;
    
    let bus: u32 = caps[1].parse().ok()?;
    let addr: u8 = u8::from_str_radix(&caps[3], 16).ok()?;
    
    Some((bus, addr))
}

fn read_i2c_adapter_name(bus: u32) -> Option<String> {
    let path = format!("/sys/bus/i2c/devices/i2c-{}/name", bus);
    fs::read_to_string(&path).ok().map(|s| s.trim().to_string())
}

fn read_sysfs_attr(path: &Path, attr: &str) -> Option<String> {
    let attr_path = path.join(attr);
    fs::read_to_string(&attr_path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn read_sysfs_hex(path: &Path, attr: &str) -> Option<String> {
    read_sysfs_attr(path, attr)
}

fn normalize_label(label: &str) -> String {
    label
        .to_lowercase()
        .replace(&['_', '-', ' ', '/', '.', ':'][..], "")
        .trim()
        .to_string()
}

fn scan_sensor_attributes(chip_path: &Path, base_name: &str) -> std::collections::HashSet<String> {
    use std::collections::HashSet;
    
    let mut attrs = HashSet::new();
    
    let suffixes = [
        "_input", "_label", "_enable", "_min", "_max", "_crit", "_alarm",
        "_type", "_auto_point1_pwm", "_auto_point2_pwm", "_pulses", "_target",
    ];
    
    for suffix in &suffixes {
        let attr_path = chip_path.join(format!("{}{}", base_name, suffix));
        if attr_path.exists() {
            attrs.insert(suffix.to_string());
        }
    }
    
    // For PWM, check base file
    if base_name.starts_with("pwm") {
        let pwm_path = chip_path.join(base_name);
        if pwm_path.exists() {
            attrs.insert("_pwm".to_string());
        }
    }
    
    attrs
}

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug, Clone)]
pub enum MatchError {
    HwmonNotAvailable,
    NoMatch,
    IoError(String),
}

impl std::fmt::Display for MatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HwmonNotAvailable => write!(f, "Hwmon sysfs not available"),
            Self::NoMatch => write!(f, "No matching sensor found"),
            Self::IoError(msg) => write!(f, "I/O error: {}", msg),
        }
    }
}

impl std::error::Error for MatchError {}
