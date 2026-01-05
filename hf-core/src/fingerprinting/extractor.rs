//! Maximum Hardware Information Extractor
//!
//! This module extracts EVERY possible hardware identifier to create the most
//! comprehensive fingerprints possible. The goal is to capture so much information
//! that hwmon reindexing can NEVER cause misidentification.

use std::fs;
use std::path::Path;
use std::hash::Hasher;
use tracing::{debug, warn};

use super::anchors::*;

#[allow(dead_code)]
const MAX_SYSFS_PATH_LENGTH: usize = 4096;

// ============================================================================
// Comprehensive Hardware Extraction
// ============================================================================

/// Extract maximum possible information from a chip
pub fn extract_comprehensive_chip_fingerprint(hwmon_path: &Path) -> Result<ChipFingerprint, ExtractionError> {
    validate_hwmon_path(hwmon_path)?;
    let now = current_timestamp_ms();
    
    // Extract driver name (REQUIRED)
    let driver_name = extract_driver_name(hwmon_path)?;
    
    // Extract ALL hardware anchors
    let hardware = extract_all_hardware_anchors(hwmon_path);
    
    // Extract ALL firmware anchors
    let firmware = extract_all_firmware_anchors(hwmon_path);
    
    // Build driver anchor with maximum info
    let driver = extract_comprehensive_driver_anchor(hwmon_path, &driver_name);
    
    // Classify chip
    let chip_class = classify_chip_comprehensive(&driver_name, &hardware, &firmware);
    
    // Generate unique ID from ALL anchors
    let id = generate_comprehensive_chip_id(&hardware, &firmware, &driver);
    
    // Check hardware anchors before moving
    let has_pci = hardware.pci.is_some();
    let has_i2c = hardware.i2c.is_some();
    let has_acpi = hardware.acpi.is_some();
    
    let fingerprint = ChipFingerprint {
        id,
        hardware,
        firmware,
        driver,
        chip_class,
        original_hwmon_path: hwmon_path.to_path_buf(),
        created_at: now,
        last_validated_at: None,
    };
    
    // Validate before returning
    fingerprint.validate()
        .map_err(|e| ExtractionError::ValidationFailed(format!("Chip validation failed: {}", e)))?;
    
    debug!(
        driver = %driver_name,
        has_pci = has_pci,
        has_i2c = has_i2c,
        has_acpi = has_acpi,
        "Extracted comprehensive chip fingerprint"
    );
    
    Ok(fingerprint)
}

/// Extract ALL possible hardware anchors
fn extract_all_hardware_anchors(hwmon_path: &Path) -> HardwareAnchor {
    let device_path = hwmon_path.join("device");
    
    HardwareAnchor {
        pci: extract_comprehensive_pci_anchor(&device_path),
        i2c: extract_comprehensive_i2c_anchor(&device_path),
        acpi: extract_comprehensive_acpi_anchor(&device_path),
        usb: extract_comprehensive_usb_anchor(&device_path),
        platform: extract_comprehensive_platform_anchor(&device_path),
    }
}

/// Extract comprehensive PCI anchor with ALL available fields
fn extract_comprehensive_pci_anchor(device_path: &Path) -> Option<PciAnchor> {
    if !device_path.exists() {
        return None;
    }
    if is_symlink(device_path) {
        warn!("Device path is symlink, skipping");
        return None;
    }
    let real_path = fs::canonicalize(device_path).ok()?;
    let path_str = real_path.to_string_lossy();
    
    if !path_str.contains("/pci") {
        return None;
    }
    
    // Extract PCI address
    let address = extract_pci_address(&path_str)?;
    
    // Read ALL PCI attributes
    let vendor_id = read_sysfs_hex(device_path, "vendor")?;
    let device_id = read_sysfs_hex(device_path, "device")?;
    let subsystem_vendor_id = read_sysfs_hex(device_path, "subsystem_vendor");
    let subsystem_device_id = read_sysfs_hex(device_path, "subsystem_device");
    let class = read_sysfs_hex(device_path, "class");
    let revision = read_sysfs_hex(device_path, "revision");
    
    // Also try alternative paths
    let subsystem_vendor_alt = read_sysfs_hex(device_path, "subsystem/vendor");
    let subsystem_device_alt = read_sysfs_hex(device_path, "subsystem/device");
    
    let anchor = PciAnchor {
        address,
        vendor_id,
        device_id,
        subsystem_vendor_id: subsystem_vendor_id.or(subsystem_vendor_alt),
        subsystem_device_id: subsystem_device_id.or(subsystem_device_alt),
        class,
        revision,
    };
    
    // Validate before returning
    if anchor.validate().is_ok() {
        Some(anchor)
    } else {
        warn!("PCI anchor validation failed, discarding");
        None
    }
}

/// Extract comprehensive I2C anchor with ALL available fields
fn extract_comprehensive_i2c_anchor(device_path: &Path) -> Option<I2cAnchor> {
    if !device_path.exists() {
        return None;
    }
    
    let real_path = fs::canonicalize(device_path).ok()?;
    let path_str = real_path.to_string_lossy();
    
    if !path_str.contains("/i2c-") {
        return None;
    }
    
    let (bus_number, device_address) = extract_i2c_info(&path_str)?;
    let adapter_name = read_i2c_adapter_name(bus_number)?;
    
    // Try to get adapter algorithm
    let adapter_algo = read_i2c_adapter_algo(bus_number);
    
    let anchor = I2cAnchor {
        bus_number,
        device_address,
        adapter_name,
        adapter_algo,
    };
    
    if anchor.validate().is_ok() {
        Some(anchor)
    } else {
        warn!("I2C anchor validation failed, discarding");
        None
    }
}

/// Extract comprehensive ACPI anchor
fn extract_comprehensive_acpi_anchor(device_path: &Path) -> Option<AcpiAnchor> {
    if !device_path.exists() {
        return None;
    }
    
    let real_path = fs::canonicalize(device_path).ok()?;
    let path_str = real_path.to_string_lossy();
    
    if !path_str.contains("LNXSYSTM") && !path_str.contains("ACPI") && !path_str.contains("PNP") {
        return None;
    }
    
    let hid = read_sysfs_attr(device_path, "hid")
        .or_else(|| read_sysfs_attr(device_path, "firmware_node/hid"));
    let uid = read_sysfs_attr(device_path, "uid")
        .or_else(|| read_sysfs_attr(device_path, "firmware_node/uid"));
    let cid = read_sysfs_attr(device_path, "cid")
        .or_else(|| read_sysfs_attr(device_path, "firmware_node/cid"));
    
    let anchor = AcpiAnchor {
        path: path_str.to_string(),
        hid,
        uid,
        cid,
    };
    
    if anchor.validate().is_ok() {
        Some(anchor)
    } else {
        None
    }
}

/// Extract comprehensive USB anchor
fn extract_comprehensive_usb_anchor(device_path: &Path) -> Option<UsbAnchor> {
    if !device_path.exists() {
        return None;
    }
    
    let real_path = fs::canonicalize(device_path).ok()?;
    let path_str = real_path.to_string_lossy();
    
    if !path_str.contains("/usb") {
        return None;
    }
    
    let vendor_id = read_sysfs_hex(device_path, "idVendor")?;
    let product_id = read_sysfs_hex(device_path, "idProduct")?;
    
    // Extract bus and device numbers
    let busnum_str = read_sysfs_attr(device_path, "busnum")?;
    let devnum_str = read_sysfs_attr(device_path, "devnum")?;
    let bus_number: u16 = busnum_str.trim().parse().ok()?;
    let device_address: u16 = devnum_str.trim().parse().ok()?;
    if bus_number > 255 || device_address > 127 {
        return None;
    }
    
    let serial_number = read_sysfs_attr(device_path, "serial");
    let port_path = extract_usb_port_path(&path_str);
    
    let anchor = UsbAnchor {
        bus_number,
        device_address,
        vendor_id,
        product_id,
        serial_number,
        port_path,
    };
    
    if anchor.validate().is_ok() {
        Some(anchor)
    } else {
        None
    }
}

/// Extract comprehensive platform anchor
fn extract_comprehensive_platform_anchor(device_path: &Path) -> Option<PlatformAnchor> {
    if !device_path.exists() {
        return None;
    }
    
    let real_path = fs::canonicalize(device_path).ok()?;
    let path_str = real_path.to_string_lossy();
    
    if !path_str.contains("/platform/") {
        return None;
    }
    
    let device_name = device_path.file_name()?.to_string_lossy().to_string();
    let of_node_path = read_sysfs_attr(device_path, "of_node/name")
        .or_else(|| read_sysfs_attr(device_path, "of_node"));
    
    // Try to extract device ID
    let device_id = read_sysfs_attr(device_path, "id")
        .and_then(|s| s.parse::<i32>().ok());
    
    let anchor = PlatformAnchor {
        device_name,
        of_node_path,
        device_id,
    };
    
    if anchor.validate().is_ok() {
        Some(anchor)
    } else {
        None
    }
}

/// Extract ALL firmware anchors
fn extract_all_firmware_anchors(_hwmon_path: &Path) -> FirmwareAnchor {
    FirmwareAnchor {
        sensor_label: None, // Will be set per-channel
        dmi: extract_comprehensive_dmi_anchor(),
    }
}

/// Extract comprehensive DMI anchor with ALL available fields
fn extract_comprehensive_dmi_anchor() -> Option<DmiAnchor> {
    let dmi_base = Path::new("/sys/class/dmi/id");
    if !dmi_base.exists() {
        return None;
    }
    
    let anchor = DmiAnchor {
        sys_vendor: read_sysfs_attr(dmi_base, "sys_vendor"),
        product_name: read_sysfs_attr(dmi_base, "product_name"),
        product_version: read_sysfs_attr(dmi_base, "product_version"),
        board_vendor: read_sysfs_attr(dmi_base, "board_vendor"),
        board_name: read_sysfs_attr(dmi_base, "board_name"),
        bios_vendor: read_sysfs_attr(dmi_base, "bios_vendor"),
        bios_version: read_sysfs_attr(dmi_base, "bios_version"),
    };
    
    // Only return if we got at least some data
    if anchor.sys_vendor.is_some() || anchor.product_name.is_some() || anchor.board_name.is_some() {
        if anchor.validate().is_ok() {
            return Some(anchor);
        }
    }
    
    None
}

/// Extract comprehensive driver anchor
fn extract_comprehensive_driver_anchor(hwmon_path: &Path, driver_name: &str) -> DriverAnchor {
    let device_path = hwmon_path.join("device");
    
    let device_path_canonical = if device_path.exists() {
        fs::canonicalize(&device_path)
            .ok()
            .map(|p| p.to_string_lossy().to_string())
    } else {
        None
    };
    
    let modalias = read_sysfs_attr(&device_path, "modalias");
    
    // Try to get driver version from multiple sources
    let driver_version = read_sysfs_attr(&device_path, "driver/version")
        .or_else(|| read_sysfs_attr(&device_path, "driver/module/version"))
        .or_else(|| read_sysfs_attr(&device_path, "module/version"));
    
    DriverAnchor {
        driver_name: driver_name.to_string(),
        device_path_canonical,
        modalias,
        driver_version,
    }
}

/// Extract comprehensive channel fingerprint
pub fn extract_comprehensive_channel_fingerprint(
    chip_fp: &ChipFingerprint,
    channel_type: ChannelType,
    sensor_name: &str,
    sensor_path: &Path,
) -> Result<ChannelFingerprint, ExtractionError> {
    let chip_path = &chip_fp.original_hwmon_path;
    let base_name = sensor_name.trim_end_matches("_input");
    
    // Extract label with multiple fallback attempts
    let label_anchor = extract_comprehensive_label_anchor(chip_path, base_name);
    
    // Build comprehensive attribute anchor
    let attributes = extract_comprehensive_attribute_anchor(chip_path, base_name, channel_type);
    
    // Infer semantic role from ALL available information
    let semantic_role = infer_semantic_role_comprehensive(
        &chip_fp.chip_class,
        &label_anchor,
        base_name,
        &chip_fp.driver.driver_name,
    );
    
    let firmware = FirmwareAnchor {
        sensor_label: label_anchor,
        dmi: chip_fp.firmware.dmi.clone(),
    };
    
    let id = generate_comprehensive_channel_id(&chip_fp.id, base_name, &firmware);
    
    let fingerprint = ChannelFingerprint {
        id,
        chip_id: chip_fp.id.clone(),
        channel_type,
        firmware,
        attributes,
        semantic_role,
        original_name: base_name.to_string(),
        original_path: sensor_path.to_path_buf(),
        created_at: current_timestamp_ms(),
    };
    
    fingerprint.validate()
        .map_err(|e| ExtractionError::ValidationFailed(format!("Channel validation failed: {}", e)))?;
    
    Ok(fingerprint)
}

/// Extract comprehensive label anchor with multiple attempts
fn extract_comprehensive_label_anchor(chip_path: &Path, base_name: &str) -> Option<SensorLabelAnchor> {
    // Try standard label file
    let label_path = chip_path.join(format!("{}_label", base_name));
    let raw_label = if let Ok(label) = fs::read_to_string(&label_path) {
        label.trim().to_string()
    } else {
        // Try alternative label locations
        let alt_label = read_sysfs_attr(chip_path, &format!("{}_name", base_name))
            .or_else(|| read_sysfs_attr(chip_path, &format!("{}_type", base_name)));
        
        alt_label?
    };
    
    if raw_label.is_empty() {
        return None;
    }
    
    let normalized_label = normalize_label(&raw_label);
    let label_hash = calculate_label_hash(&normalized_label);
    
    let anchor = SensorLabelAnchor {
        raw_label,
        normalized_label,
        label_hash,
    };
    
    if anchor.validate().is_ok() {
        Some(anchor)
    } else {
        None
    }
}

/// Extract comprehensive attribute anchor
fn extract_comprehensive_attribute_anchor(
    chip_path: &Path,
    base_name: &str,
    channel_type: ChannelType,
) -> AttributeAnchor {
    let mut attribute_files = std::collections::HashSet::new();
    
    // Comprehensive list of possible attributes
    let all_suffixes = [
        "_input", "_label", "_enable", "_min", "_max", "_crit", "_alarm",
        "_type", "_auto_point1_pwm", "_auto_point2_pwm", "_auto_point3_pwm",
        "_auto_point4_pwm", "_auto_point5_pwm",
        "_auto_point1_temp", "_auto_point2_temp", "_auto_point3_temp",
        "_auto_point4_temp", "_auto_point5_temp",
        "_pulses", "_target", "_div", "_beep", "_fault", "_emergency",
        "_lowest", "_highest", "_average", "_reset", "_offset",
        "_lcrit", "_crit_hyst", "_max_hyst", "_min_hyst",
    ];
    
    for suffix in &all_suffixes {
        let attr_path = chip_path.join(format!("{}{}", base_name, suffix));
        if attr_path.exists() {
            attribute_files.insert(suffix.to_string());
        }
    }
    
    // For PWM, check base file
    if channel_type == ChannelType::Pwm {
        let pwm_path = chip_path.join(base_name);
        if pwm_path.exists() {
            attribute_files.insert("_pwm".to_string());
        }
    }
    
    // Build capabilities
    let capabilities = SensorCapabilities {
        has_input: attribute_files.contains("_input"),
        has_label: attribute_files.contains("_label"),
        has_enable: attribute_files.contains("_enable"),
        is_writable: check_writable(chip_path, base_name, channel_type),
        has_limits: attribute_files.contains("_min") || attribute_files.contains("_max"),
        has_alarm: attribute_files.contains("_alarm"),
    };
    
    // Try to determine expected range from current value
    let expected_range = extract_value_range(chip_path, base_name, channel_type);
    
    AttributeAnchor {
        attribute_files,
        capabilities,
        expected_range,
    }
}

/// Extract comprehensive PWM fingerprint
pub fn extract_comprehensive_pwm_fingerprint(
    channel_fp: ChannelFingerprint,
    chip_path: &Path,
    pwm_name: &str,
) -> Result<PwmChannelFingerprint, ExtractionError> {
    let pwm_path = chip_path.join(pwm_name);
    let enable_path = chip_path.join(format!("{}_enable", pwm_name));
    
    // Check for corresponding fan
    let fan_name = pwm_name.replace("pwm", "fan");
    let fan_input_path = chip_path.join(format!("{}_input", fan_name));
    let has_rpm_feedback = fan_input_path.exists();
    
    let pwm_capabilities = PwmCapabilities {
        has_enable: enable_path.exists(),
        is_writable: check_pwm_writable(&pwm_path),
        has_rpm_feedback,
        control_authority: detect_control_authority(&enable_path),
    };
    
    let fingerprint = PwmChannelFingerprint {
        channel: channel_fp,
        paired_fan_id: None, // Will be set during binding
        runtime: None, // Will be populated during probing
        pwm_capabilities,
        safe_fallback: SafeFallbackPolicy::FullSpeed,
    };
    
    fingerprint.validate()
        .map_err(|e| ExtractionError::ValidationFailed(format!("PWM validation failed: {}", e)))?;
    
    Ok(fingerprint)
}

// ============================================================================
// Helper Functions
// ============================================================================

fn extract_driver_name(hwmon_path: &Path) -> Result<String, ExtractionError> {
    let name_path = hwmon_path.join("name");
    fs::read_to_string(&name_path)
        .map(|s| s.trim().to_string())
        .map_err(|e| ExtractionError::MissingDriverName(format!("Failed to read driver name: {}", e)))
}

fn extract_pci_address(path: &str) -> Option<String> {
    // Manual parsing - no regex dependency needed
    // PCI address format: DDDD:BB:DD.F (domain:bus:device.function)
    for part in path.split('/') {
        if part.len() >= 12 && part.contains(':') && part.contains('.') {
            let bytes = part.as_bytes();
            if bytes.len() >= 12 &&
               bytes[4] == b':' && bytes[7] == b':' && bytes[10] == b'.' &&
               bytes[0..4].iter().all(|&b| b.is_ascii_hexdigit()) &&
               bytes[5..7].iter().all(|&b| b.is_ascii_hexdigit()) &&
               bytes[8..10].iter().all(|&b| b.is_ascii_hexdigit()) &&
               bytes.get(11).map(|&b| b.is_ascii_hexdigit()).unwrap_or(false) {
                return Some(part[..12].to_string());
            }
        }
    }
    None
}

fn extract_i2c_info(path: &str) -> Option<(u32, u8)> {
    // Manual parsing: look for "i2c-N/N-XXXX" pattern
    for (i, part) in path.split('/').enumerate() {
        if part.starts_with("i2c-") {
            let bus_str = &part[4..];
            let bus: u32 = bus_str.parse().ok()?;
            if let Some(next_part) = path.split('/').nth(i + 1) {
                if let Some(dash_pos) = next_part.find('-') {
                    let addr_str = &next_part[dash_pos + 1..];
                    if addr_str.len() == 4 && addr_str.chars().all(|c| c.is_ascii_hexdigit()) {
                        let addr: u8 = u8::from_str_radix(addr_str, 16).ok()?;
                        return Some((bus, addr));
                    }
                }
            }
        }
    }
    None
}

fn extract_usb_port_path(path: &str) -> Option<String> {
    // Manual parsing: look for "usbN/X-Y.Z/" pattern
    for (i, part) in path.split('/').enumerate() {
        if part.starts_with("usb") && part[3..].chars().all(|c| c.is_ascii_digit()) {
            if let Some(port) = path.split('/').nth(i + 1) {
                if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-') {
                    return Some(port.to_string());
                }
            }
        }
    }
    None
}

fn read_sysfs_attr(path: &Path, attr: &str) -> Option<String> {
    if attr.contains("..") || attr.contains('/') || attr.contains('\0') {
        return None;
    }
    let attr_path = path.join(attr);
    fs::read_to_string(&attr_path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn read_sysfs_hex(path: &Path, attr: &str) -> Option<String> {
    read_sysfs_attr(path, attr)
}

fn read_i2c_adapter_name(bus: u32) -> Option<String> {
    let path = format!("/sys/bus/i2c/devices/i2c-{}/name", bus);
    fs::read_to_string(&path).ok().map(|s| s.trim().to_string())
}

fn read_i2c_adapter_algo(bus: u32) -> Option<String> {
    let path = format!("/sys/bus/i2c/devices/i2c-{}/adapter/name", bus);
    fs::read_to_string(&path).ok().map(|s| s.trim().to_string())
}

fn normalize_label(label: &str) -> String {
    label
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

fn calculate_label_hash(label: &str) -> u64 {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(label.as_bytes());
    let result = hasher.finalize();
    // SAFETY: SHA256 always returns 32 bytes, so taking first 8 bytes is always safe
    u64::from_le_bytes(result[0..8].try_into().expect("SHA256 result is always 32 bytes"))
}

fn check_writable(chip_path: &Path, base_name: &str, channel_type: ChannelType) -> bool {
    use std::fs::OpenOptions;
    
    let path = if channel_type == ChannelType::Pwm {
        chip_path.join(base_name)
    } else {
        chip_path.join(format!("{}_input", base_name))
    };
    
    OpenOptions::new().write(true).open(&path).is_ok()
}

fn check_pwm_writable(pwm_path: &Path) -> bool {
    use std::fs::OpenOptions;
    OpenOptions::new().write(true).open(pwm_path).is_ok()
}

fn detect_control_authority(enable_path: &Path) -> ControlAuthority {
    if !enable_path.exists() {
        return ControlAuthority::BiosOnly;
    }
    
    if let Ok(content) = fs::read_to_string(enable_path) {
        match content.trim() {
            "1" => ControlAuthority::Manual,
            "0" | "2" | "3" | "4" | "5" => ControlAuthority::Automatic,
            _ => ControlAuthority::Unknown,
        }
    } else {
        ControlAuthority::Unknown
    }
}

fn extract_value_range(chip_path: &Path, base_name: &str, channel_type: ChannelType) -> Option<(i64, i64)> {
    // Try to read min/max if available
    let min_val = read_sysfs_attr(chip_path, &format!("{}_min", base_name))
        .and_then(|s| s.parse::<i64>().ok());
    let max_val = read_sysfs_attr(chip_path, &format!("{}_max", base_name))
        .and_then(|s| s.parse::<i64>().ok());
    
    if let (Some(min), Some(max)) = (min_val, max_val) {
        return Some((min, max));
    }
    
    // Fallback to reasonable defaults based on type
    match channel_type {
        ChannelType::Temperature => Some((0, 150000)), // 0-150°C in millidegrees
        ChannelType::Fan => Some((0, 50000)), // 0-50000 RPM
        ChannelType::Pwm => Some((0, 255)), // 0-255 PWM
        ChannelType::Voltage => Some((0, 20000)), // 0-20V in millivolts
        _ => None,
    }
}

fn classify_chip_comprehensive(
    driver_name: &str,
    hardware: &HardwareAnchor,
    _firmware: &FirmwareAnchor,
) -> ChipClass {
    let name_lower = driver_name.to_lowercase();
    
    // CPU temperature drivers
    if name_lower.contains("coretemp") || name_lower.contains("k10temp") 
        || name_lower.contains("k8temp") || name_lower.contains("zenpower") {
        return ChipClass::CpuTemp;
    }
    
    // GPU drivers
    if name_lower.contains("amdgpu") || name_lower.contains("radeon") 
        || name_lower.contains("nouveau") || name_lower.contains("nvidia") {
        return ChipClass::DiscreteGpu;
    }
    
    if name_lower.contains("i915") || name_lower.contains("xe") {
        return ChipClass::IntegratedGpu;
    }
    
    // Check PCI class for GPU
    if let Some(ref pci) = hardware.pci {
        if let Some(ref class) = pci.class {
            if class.starts_with("0x03") {
                return ChipClass::DiscreteGpu;
            }
        }
    }
    
    // Embedded Controller
    if name_lower.contains("thinkpad") || name_lower.contains("dell") 
        || name_lower.contains("hp") || name_lower.contains("asus") 
        || name_lower.contains("applesmc") {
        return ChipClass::EmbeddedController;
    }
    
    // SuperIO
    if name_lower.contains("nct") || name_lower.contains("it87") 
        || name_lower.contains("w83") || name_lower.contains("f71") {
        return ChipClass::SuperIo;
    }
    
    // ACPI Thermal
    if name_lower.contains("acpi") || name_lower.contains("thermal") {
        return ChipClass::AcpiThermal;
    }
    
    // NVMe
    if name_lower.contains("nvme") {
        return ChipClass::NvmeDrive;
    }
    
    // SATA
    if name_lower.contains("drivetemp") || name_lower.contains("sata") {
        return ChipClass::SataDrive;
    }
    
    ChipClass::Unknown
}

fn infer_semantic_role_comprehensive(
    chip_class: &ChipClass,
    label: &Option<SensorLabelAnchor>,
    _name: &str,
    _driver_name: &str,
) -> SemanticRole {
    // Check label first (most reliable)
    if let Some(ref label_anchor) = label {
        let l = label_anchor.normalized_label.to_lowercase();
        
        if l.contains("package") || l.contains("pkg") || l.contains("tctl") {
            return SemanticRole::CpuPackage;
        }
        if l.contains("core") && !l.contains("gpu") {
            return SemanticRole::CpuCore;
        }
        if l.contains("die") || l.contains("tdie") {
            return SemanticRole::CpuDie;
        }
        if l.contains("gpu") || l.contains("graphics") {
            if l.contains("hotspot") || l.contains("junction") || l.contains("mem") {
                return SemanticRole::GpuHotspot;
            }
            return SemanticRole::GpuCore;
        }
        if l.contains("vrm") || l.contains("vcore") {
            if l.contains("cpu") {
                return SemanticRole::VrmCpu;
            }
            if l.contains("gpu") {
                return SemanticRole::VrmGpu;
            }
        }
        if l.contains("chipset") || l.contains("pch") {
            return SemanticRole::Chipset;
        }
        if l.contains("nvme") || l.contains("ssd") {
            return SemanticRole::NvmeDrive;
        }
        if l.contains("ambient") || l.contains("motherboard") {
            return SemanticRole::Motherboard;
        }
    }
    
    // Infer from chip class and driver
    match chip_class {
        ChipClass::CpuTemp => SemanticRole::CpuPackage,
        ChipClass::DiscreteGpu | ChipClass::IntegratedGpu => SemanticRole::GpuCore,
        ChipClass::NvmeDrive => SemanticRole::NvmeDrive,
        ChipClass::SataDrive => SemanticRole::SataDrive,
        ChipClass::SuperIo => SemanticRole::Motherboard,
        _ => SemanticRole::Unknown,
    }
}

fn generate_comprehensive_chip_id(
    hardware: &HardwareAnchor,
    firmware: &FirmwareAnchor,
    driver: &DriverAnchor,
) -> String {
    use sha2::{Sha256, Digest};
    
    let mut hasher = Sha256::new();
    
    // Hash ALL available anchors in deterministic order
    if let Some(ref pci) = hardware.pci {
        hasher.update(b"pci:");
        hasher.update(pci.address.as_bytes());
        hasher.update(pci.vendor_id.as_bytes());
        hasher.update(pci.device_id.as_bytes());
        if let Some(ref s) = pci.subsystem_vendor_id {
            hasher.update(s.as_bytes());
        }
        if let Some(ref s) = pci.subsystem_device_id {
            hasher.update(s.as_bytes());
        }
    }
    
    if let Some(ref i2c) = hardware.i2c {
        hasher.update(b"i2c:");
        hasher.update(i2c.bus_number.to_le_bytes());
        hasher.update(&[i2c.device_address]);
        hasher.update(i2c.adapter_name.as_bytes());
    }
    
    if let Some(ref acpi) = hardware.acpi {
        hasher.update(b"acpi:");
        hasher.update(acpi.path.as_bytes());
        if let Some(ref s) = acpi.hid {
            hasher.update(s.as_bytes());
        }
        if let Some(ref s) = acpi.uid {
            hasher.update(s.as_bytes());
        }
    }
    
    hasher.update(b"driver:");
    hasher.update(driver.driver_name.as_bytes());
    if let Some(ref s) = driver.device_path_canonical {
        hasher.update(s.as_bytes());
    }
    if let Some(ref s) = driver.modalias {
        hasher.update(s.as_bytes());
    }
    
    if let Some(ref dmi) = firmware.dmi {
        hasher.update(b"dmi:");
        if let Some(ref s) = dmi.sys_vendor {
            hasher.update(s.as_bytes());
        }
        if let Some(ref s) = dmi.product_name {
            hasher.update(s.as_bytes());
        }
        if let Some(ref s) = dmi.board_name {
            hasher.update(s.as_bytes());
        }
    }
    
    let result = hasher.finalize();
    // SAFETY: SHA256 always returns 32 bytes, so taking first 8 bytes is always safe
    format!("{:016x}", u64::from_le_bytes(result[0..8].try_into().expect("SHA256 result is always 32 bytes")))
}

fn generate_comprehensive_channel_id(
    chip_id: &str,
    base_name: &str,
    firmware: &FirmwareAnchor,
) -> String {
    use sha2::{Sha256, Digest};
    
    let mut hasher = Sha256::new();
    
    hasher.update(b"channel:");
    hasher.update(chip_id.as_bytes());
    hasher.update(base_name.as_bytes());
    
    if let Some(ref label) = firmware.sensor_label {
        hasher.update(b"label:");
        hasher.update(label.normalized_label.as_bytes());
    }
    
    let result = hasher.finalize();
    // SAFETY: SHA256 always returns 32 bytes, so taking first 8 bytes is always safe
    format!("{:016x}", u64::from_le_bytes(result[0..8].try_into().expect("SHA256 result is always 32 bytes")))
}

fn current_timestamp_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ============================================================================
// Security Helper Functions
// ============================================================================

/// Validate hwmon path is under /sys/class/hwmon
fn validate_hwmon_path(path: &Path) -> Result<(), ExtractionError> {
    let path_str = path.to_string_lossy();
    
    // Must be absolute path
    if !path.is_absolute() {
        return Err(ExtractionError::ValidationFailed(
            "Hwmon path must be absolute".to_string()
        ));
    }
    
    // Must be under /sys/class/hwmon
    if !path_str.starts_with("/sys/class/hwmon/") {
        return Err(ExtractionError::ValidationFailed(
            "Hwmon path must be under /sys/class/hwmon".to_string()
        ));
    }
    
    // No path traversal
    if path_str.contains("..") {
        return Err(ExtractionError::ValidationFailed(
            "Path traversal detected in hwmon path".to_string()
        ));
    }
    
    // No null bytes
    if path_str.contains('\0') {
        return Err(ExtractionError::ValidationFailed(
            "Null byte in hwmon path".to_string()
        ));
    }
    
    Ok(())
}

/// Check if path is a symlink
fn is_symlink(path: &Path) -> bool {
    path.symlink_metadata()
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
}

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug, Clone)]
pub enum ExtractionError {
    MissingDriverName(String),
    ValidationFailed(String),
    IoError(String),
}

impl std::fmt::Display for ExtractionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingDriverName(msg) => write!(f, "Missing driver name: {}", msg),
            Self::ValidationFailed(msg) => write!(f, "Validation failed: {}", msg),
            Self::IoError(msg) => write!(f, "I/O error: {}", msg),
        }
    }
}

impl std::error::Error for ExtractionError {}
