//! Zero-Drift Sensor Fingerprinting System
//!
//! This module implements a comprehensive multi-layered fingerprinting system that
//! guarantees PWM/fan sensor pairings can NEVER drift due to hwmon reindexing,
//! kernel driver changes, or hardware enumeration order changes.
//!
//! # Architecture
//!
//! The system uses multiple independent anchor layers:
//!
//! 1. **Hardware Anchors** (Tier 1 - Immutable)
//!    - PCI bus topology and device IDs
//!    - I2C bus addresses and adapter names
//!    - ACPI device paths
//!    - USB topology (for external controllers)
//!
//! 2. **Firmware Anchors** (Tier 2 - Stable)
//!    - Sensor labels from firmware/BIOS
//!    - DMI/SMBIOS identifiers
//!    - Device tree paths (ARM/embedded)
//!
//! 3. **Driver Anchors** (Tier 3 - Semi-stable)
//!    - Driver name and version
//!    - Device symlink canonical paths
//!    - Modalias strings
//!
//! 4. **Attribute Anchors** (Tier 4 - Validation)
//!    - Sensor attribute file sets
//!    - Capability flags (writable, enable modes)
//!    - Value range characteristics
//!
//! 5. **Runtime Anchors** (Tier 5 - Behavioral)
//!    - PWM-to-fan response signatures
//!    - Temporal response characteristics
//!    - Value distribution patterns
//!
//! # Matching Strategy
//!
//! When validating a stored fingerprint against the current system:
//!
//! 1. Match using Tier 1 anchors (PCI/I2C/ACPI) - 100% confidence if all match
//! 2. If Tier 1 partial, combine with Tier 2 (labels) - 95%+ confidence
//! 3. If Tier 1-2 unavailable, use Tier 3 (driver) + Tier 4 (attributes) - 85%+ confidence
//! 4. Tier 5 used for runtime validation and drift detection
//!
//! # Zero-Drift Guarantee
//!
//! The system guarantees zero drift by:
//!
//! - Never relying on hwmon index numbers (hwmon0, hwmon1, etc.)
//! - Never relying on sensor index numbers alone (temp1, fan2, etc.)
//! - Always requiring multiple independent anchors to match
//! - Failing safe (refusing to apply control) if confidence drops below threshold
//! - Continuous runtime validation to detect hardware changes
//!
//! # Safety
//!
//! If a binding cannot be validated with high confidence:
//! - The system will NOT apply fan control
//! - Safe fallback policies are applied (typically 100% fan speed)
//! - User is notified to re-run detection/binding

pub mod validation;
pub mod anchors;
pub mod extractor;
pub mod drift_correction;
pub mod matcher;
pub mod store;
pub mod validator;
pub mod runtime;
pub mod startup;
pub mod hardware_change_detection;

// Re-export validation types for external use
pub use validation::{AnchorValidationError};

pub use anchors::{
    HardwareAnchor, FirmwareAnchor, DriverAnchor, AttributeAnchor, RuntimeAnchor,
    ChipFingerprint, ChannelFingerprint, PwmChannelFingerprint,
    PciAnchor, I2cAnchor, AcpiAnchor, UsbAnchor, PlatformAnchor,
    SensorLabelAnchor, DmiAnchor, SensorCapabilities, PwmCapabilities,
    ChipClass, ChannelType, SemanticRole, ControlAuthority, SafeFallbackPolicy,
};
pub use extractor::{
    extract_comprehensive_chip_fingerprint,
    extract_comprehensive_channel_fingerprint,
    extract_comprehensive_pwm_fingerprint,
    ExtractionError,
};
pub use drift_correction::{
    detect_and_correct_drift,
    DriftDetectionResult, BindingDriftInfo, DriftStatus,
    generate_drift_report,
};
pub use matcher::{
    MatchResult, MatchConfidence, MatchReason, AnchorTier, MatchError,
    find_chip_by_fingerprint, find_channel_by_fingerprint,
};
pub use store::{FingerprintStore, StoredBinding, ValidationState};
pub use validator::{ValidationResult, ValidationReport, validate_binding, validate_all_bindings};
pub use runtime::{RuntimeValidator, DriftDetector, RuntimeStats, PwmResponseValidator, ResponseValidation};
pub use startup::{
    initialize_fingerprinting_system,
    is_fingerprinting_initialized,
    get_safe_bindings,
    get_binding_info,
    StartupResult, BindingInfo, FanInfo,
};
pub use hardware_change_detection::{
    detect_hardware_changes,
    HardwareChangeStatus, HardwareChangeReport, ChangeSeverity,
};

/// Minimum confidence threshold for applying fan control
pub const MIN_CONFIDENCE_FOR_CONTROL: f32 = 0.90;

/// Confidence threshold for warning user
pub const CONFIDENCE_WARNING_THRESHOLD: f32 = 0.95;

/// Confidence threshold for degraded state
pub const CONFIDENCE_DEGRADED_THRESHOLD: f32 = 0.85;
