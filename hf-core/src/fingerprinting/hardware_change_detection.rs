//! Hardware Change Detection (v3 Anti-Drift)
//!
//! This module detects significant hardware changes (motherboard replacement,
//! major hardware changes) and explicitly notifies the user that bindings are
//! no longer valid.

use std::path::Path;
use tracing::{info, warn, error};

use super::anchors::*;
use super::store::*;

// ============================================================================
// Hardware Change Detection
// ============================================================================

/// Result of hardware change detection
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HardwareChangeStatus {
    /// No hardware changes detected
    NoChange,
    
    /// Minor hardware changes (GPU added/removed, drives changed)
    MinorChange {
        changes: Vec<String>,
    },
    
    /// Major hardware changes (motherboard replaced, CPU changed)
    MajorChange {
        changes: Vec<String>,
        severity: ChangeSeverity,
    },
    
    /// System identity completely different (different machine)
    SystemReplaced {
        old_system: String,
        new_system: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeSeverity {
    /// Moderate - some bindings may still work
    Moderate,
    
    /// Severe - most bindings invalid
    Severe,
    
    /// Critical - all bindings invalid, complete system replacement
    Critical,
}

/// Detailed hardware change report
#[derive(Debug, Clone)]
pub struct HardwareChangeReport {
    pub status: HardwareChangeStatus,
    pub safe_bindings: Vec<String>,
    pub invalid_bindings: Vec<String>,
    pub user_message: String,
    pub requires_rebind: bool,
    pub allow_control: bool,
}

// ============================================================================
// Detection Functions
// ============================================================================

/// Detect hardware changes by comparing stored system fingerprint with current
pub fn detect_hardware_changes(store: &FingerprintStore) -> HardwareChangeReport {
    info!("Detecting hardware changes");
    
    // Extract current system DMI
    let current_dmi = extract_current_system_dmi();
    
    // Compare with stored system DMI
    let stored_dmi = &store.system_dmi;
    
    let status = if let Some(ref stored) = stored_dmi {
        if let Some(ref current) = current_dmi {
            compare_system_dmi(stored, current)
        } else {
            // DMI not available on current system (unusual)
            warn!("DMI not available on current system");
            HardwareChangeStatus::MinorChange {
                changes: vec!["DMI information not available".to_string()],
            }
        }
    } else {
        // No stored DMI - first run or old config
        info!("No stored system DMI - first run or migration");
        HardwareChangeStatus::NoChange
    };
    
    // Generate report based on status
    generate_change_report(status, store)
}

/// Compare stored and current system DMI to detect changes
fn compare_system_dmi(stored: &DmiAnchor, current: &DmiAnchor) -> HardwareChangeStatus {
    let mut changes = Vec::new();
    let mut severity_score = 0;
    
    // Check system vendor
    if stored.sys_vendor != current.sys_vendor {
        changes.push(format!(
            "System vendor changed: {:?} -> {:?}",
            stored.sys_vendor, current.sys_vendor
        ));
        severity_score += 3; // High severity
    }
    
    // Check product name
    if stored.product_name != current.product_name {
        changes.push(format!(
            "Product name changed: {:?} -> {:?}",
            stored.product_name, current.product_name
        ));
        severity_score += 3; // High severity
    }
    
    // Check board vendor
    if stored.board_vendor != current.board_vendor {
        changes.push(format!(
            "Board vendor changed: {:?} -> {:?}",
            stored.board_vendor, current.board_vendor
        ));
        severity_score += 2; // Medium severity
    }
    
    // Check board name (MOST CRITICAL - motherboard replacement)
    if stored.board_name != current.board_name {
        changes.push(format!(
            "Motherboard changed: {:?} -> {:?}",
            stored.board_name, current.board_name
        ));
        severity_score += 5; // Critical severity
    }
    
    // Check BIOS vendor
    if stored.bios_vendor != current.bios_vendor {
        changes.push(format!(
            "BIOS vendor changed: {:?} -> {:?}",
            stored.bios_vendor, current.bios_vendor
        ));
        severity_score += 1; // Low severity
    }
    
    // Check BIOS version (expected to change with updates)
    if stored.bios_version != current.bios_version {
        changes.push(format!(
            "BIOS version changed: {:?} -> {:?}",
            stored.bios_version, current.bios_version
        ));
        // Don't increase severity - BIOS updates are normal
    }
    
    // Determine status based on severity
    if changes.is_empty() {
        HardwareChangeStatus::NoChange
    } else if severity_score >= 8 {
        // Critical: Complete system replacement
        HardwareChangeStatus::SystemReplaced {
            old_system: format_system_name(stored),
            new_system: format_system_name(current),
        }
    } else if severity_score >= 5 {
        // Severe: Motherboard replacement
        HardwareChangeStatus::MajorChange {
            changes,
            severity: ChangeSeverity::Critical,
        }
    } else if severity_score >= 3 {
        // Moderate: Significant changes
        HardwareChangeStatus::MajorChange {
            changes,
            severity: ChangeSeverity::Severe,
        }
    } else {
        // Minor: BIOS update or minor changes
        HardwareChangeStatus::MinorChange { changes }
    }
}

/// Generate comprehensive change report
fn generate_change_report(
    status: HardwareChangeStatus,
    store: &FingerprintStore,
) -> HardwareChangeReport {
    match status {
        HardwareChangeStatus::NoChange => {
            HardwareChangeReport {
                status,
                safe_bindings: store.bindings.keys().cloned().collect(),
                invalid_bindings: Vec::new(),
                user_message: "No hardware changes detected. All bindings valid.".to_string(),
                requires_rebind: false,
                allow_control: true,
            }
        }
        
        HardwareChangeStatus::MinorChange { ref changes } => {
            info!("Minor hardware changes detected: {:?}", changes);
            let changes_msg = changes.join("\n");
            HardwareChangeReport {
                status,
                safe_bindings: store.bindings.keys().cloned().collect(),
                invalid_bindings: Vec::new(),
                user_message: format!(
                    "Minor hardware changes detected:\n{}\n\nBindings remain valid.",
                    changes_msg
                ),
                requires_rebind: false,
                allow_control: true,
            }
        }
        
        HardwareChangeStatus::MajorChange { ref changes, severity } => {
            warn!("Major hardware changes detected: {:?}", changes);
            
            let (message, allow_control) = match severity {
                ChangeSeverity::Critical => (
                    format!(
                        "⚠️ CRITICAL HARDWARE CHANGE DETECTED ⚠️\n\n{}\n\n\
                        Fan control pairings are NO LONGER VALID.\n\
                        All fan controls have been DISABLED for safety.\n\n\
                        Please re-run hardware detection to create new bindings.",
                        changes.join("\n")
                    ),
                    false, // DO NOT allow control
                ),
                ChangeSeverity::Severe => (
                    format!(
                        "⚠️ SIGNIFICANT HARDWARE CHANGE DETECTED ⚠️\n\n{}\n\n\
                        Fan control pairings may be INVALID.\n\
                        Fan controls have been disabled for safety.\n\n\
                        Please verify hardware and re-run detection.",
                        changes.join("\n")
                    ),
                    false, // DO NOT allow control
                ),
                ChangeSeverity::Moderate => (
                    format!(
                        "⚠️ Hardware changes detected:\n\n{}\n\n\
                        Some fan control pairings may be affected.\n\
                        Please verify fan controls are working correctly.",
                        changes.join("\n")
                    ),
                    true, // Allow control but warn
                ),
            };
            
            HardwareChangeReport {
                status,
                safe_bindings: Vec::new(),
                invalid_bindings: store.bindings.keys().cloned().collect(),
                user_message: message,
                requires_rebind: true,
                allow_control,
            }
        }
        
        HardwareChangeStatus::SystemReplaced { ref old_system, ref new_system } => {
            error!("Complete system replacement detected");
            let old_sys = old_system.clone();
            let new_sys = new_system.clone();
            HardwareChangeReport {
                status,
                safe_bindings: Vec::new(),
                invalid_bindings: store.bindings.keys().cloned().collect(),
                user_message: format!(
                    "⚠️ SYSTEM REPLACEMENT DETECTED ⚠️\n\n\
                    Old system: {}\n\
                    New system: {}\n\n\
                    This appears to be a DIFFERENT COMPUTER.\n\
                    All fan control bindings are INVALID.\n\
                    All controls have been DISABLED.\n\n\
                    Please delete the old configuration and run hardware detection.",
                    old_sys, new_sys
                ),
                requires_rebind: true,
                allow_control: false,
            }
        }
    }
}

/// Extract current system DMI information
fn extract_current_system_dmi() -> Option<DmiAnchor> {
    let dmi_base = Path::new("/sys/class/dmi/id");
    if !dmi_base.exists() {
        return None;
    }
    
    let anchor = DmiAnchor {
        sys_vendor: read_dmi_field(dmi_base, "sys_vendor"),
        product_name: read_dmi_field(dmi_base, "product_name"),
        product_version: read_dmi_field(dmi_base, "product_version"),
        board_vendor: read_dmi_field(dmi_base, "board_vendor"),
        board_name: read_dmi_field(dmi_base, "board_name"),
        bios_vendor: read_dmi_field(dmi_base, "bios_vendor"),
        bios_version: read_dmi_field(dmi_base, "bios_version"),
    };
    
    // Only return if we got at least some data
    if anchor.sys_vendor.is_some() || anchor.product_name.is_some() || anchor.board_name.is_some() {
        if anchor.validate().is_ok() {
            return Some(anchor);
        }
    }
    
    None
}

fn read_dmi_field(base: &Path, field: &str) -> Option<String> {
    std::fs::read_to_string(base.join(field))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn format_system_name(dmi: &DmiAnchor) -> String {
    let vendor = dmi.sys_vendor.as_deref().unwrap_or("Unknown");
    let product = dmi.product_name.as_deref().unwrap_or("Unknown");
    let board = dmi.board_name.as_deref().unwrap_or("Unknown");
    format!("{} {} ({})", vendor, product, board)
}

// ============================================================================
// Store Integration
// ============================================================================

impl FingerprintStore {
    /// Update system DMI fingerprint (should be called on first extraction)
    pub fn update_system_dmi(&mut self) {
        if let Some(dmi) = extract_current_system_dmi() {
            info!(
                vendor = ?dmi.sys_vendor,
                product = ?dmi.product_name,
                board = ?dmi.board_name,
                "Updating system DMI fingerprint"
            );
            self.system_dmi = Some(dmi);
            self.last_modified_at = current_timestamp_ms();
        } else {
            warn!("Could not extract system DMI information");
        }
    }
    
    /// Check if hardware changes invalidate bindings
    pub fn check_hardware_changes(&self) -> HardwareChangeReport {
        detect_hardware_changes(self)
    }
}

fn current_timestamp_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_change() {
        let dmi = DmiAnchor {
            sys_vendor: Some("ASUS".to_string()),
            product_name: Some("ROG STRIX".to_string()),
            product_version: None,
            board_vendor: Some("ASUSTeK".to_string()),
            board_name: Some("ROG STRIX X570-E".to_string()),
            bios_vendor: Some("American Megatrends".to_string()),
            bios_version: Some("1.0".to_string()),
        };
        
        let status = compare_system_dmi(&dmi, &dmi);
        assert_eq!(status, HardwareChangeStatus::NoChange);
    }

    #[test]
    fn test_bios_update() {
        let old_dmi = DmiAnchor {
            sys_vendor: Some("ASUS".to_string()),
            product_name: Some("ROG STRIX".to_string()),
            product_version: None,
            board_vendor: Some("ASUSTeK".to_string()),
            board_name: Some("ROG STRIX X570-E".to_string()),
            bios_vendor: Some("American Megatrends".to_string()),
            bios_version: Some("1.0".to_string()),
        };
        
        let new_dmi = DmiAnchor {
            bios_version: Some("2.0".to_string()),
            ..old_dmi.clone()
        };
        
        let status = compare_system_dmi(&old_dmi, &new_dmi);
        assert!(matches!(status, HardwareChangeStatus::MinorChange { .. }));
    }

    #[test]
    fn test_motherboard_replacement() {
        let old_dmi = DmiAnchor {
            sys_vendor: Some("ASUS".to_string()),
            product_name: Some("ROG STRIX".to_string()),
            product_version: None,
            board_vendor: Some("ASUSTeK".to_string()),
            board_name: Some("ROG STRIX X570-E".to_string()),
            bios_vendor: Some("American Megatrends".to_string()),
            bios_version: Some("1.0".to_string()),
        };
        
        let new_dmi = DmiAnchor {
            board_name: Some("ROG STRIX B550-F".to_string()),
            ..old_dmi.clone()
        };
        
        let status = compare_system_dmi(&old_dmi, &new_dmi);
        assert!(matches!(
            status,
            HardwareChangeStatus::MajorChange {
                severity: ChangeSeverity::Critical,
                ..
            }
        ));
    }

    #[test]
    fn test_system_replacement() {
        let old_dmi = DmiAnchor {
            sys_vendor: Some("ASUS".to_string()),
            product_name: Some("ROG STRIX".to_string()),
            product_version: None,
            board_vendor: Some("ASUSTeK".to_string()),
            board_name: Some("ROG STRIX X570-E".to_string()),
            bios_vendor: Some("American Megatrends".to_string()),
            bios_version: Some("1.0".to_string()),
        };
        
        let new_dmi = DmiAnchor {
            sys_vendor: Some("Dell Inc.".to_string()),
            product_name: Some("XPS 15".to_string()),
            product_version: None,
            board_vendor: Some("Dell Inc.".to_string()),
            board_name: Some("0CF6RR".to_string()),
            bios_vendor: Some("Dell Inc.".to_string()),
            bios_version: Some("1.0".to_string()),
        };
        
        let status = compare_system_dmi(&old_dmi, &new_dmi);
        assert!(matches!(status, HardwareChangeStatus::SystemReplaced { .. }));
    }
}
