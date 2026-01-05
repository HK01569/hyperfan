//! Hyperfan Core Library
//!
//! A world-class fan control platform for Linux systems.
//!
//! # Features
//!
//! - **Hardware Discovery**: Automatic enumeration of hwmon chips, fans, and PWM controllers
//! - **Smart Detection**: Active probing to accurately map PWM controllers to fans
//! - **Fan Curves**: Temperature-based fan control with hysteresis and smoothing
//! - **Daemon Mode**: Background service for continuous fan management
//! - **Configuration**: Persistent profiles with custom curves and sensor names
//!
//! # Module Structure
//!
//! - `hw/` - Hardware interaction (enumeration, detection, control)
//! - `data/` - Data types, configuration, validation
//! - `engine/` - Fan curve engine and daemon
//!
//! # Example
//!
//! ```no_run
//! use hf_core::{enumerate_hwmon_chips, FanCurve, CurvePreset};
//!
//! // Discover hardware
//! let chips = enumerate_hwmon_chips().unwrap();
//!
//! // Create a fan curve
//! let curve = CurvePreset::Balanced.to_curve();
//! ```

// Grouped modules
pub mod data;
pub mod engine;
pub mod fingerprinting;
pub mod hw;

// Standalone modules
pub mod constants;
pub mod daemon_client;
pub mod display;
pub mod error;
pub mod service;
pub mod settings;
pub mod system;

// Re-export primary types from data/
pub use data::{
    CurvePoint, FanMapping, FanSensor, HwmonChip, ProbeResult, PwmController,
    RawChipData, RawControllerSnapshot, RawFanReading, RawPwmReading, RawTempReading,
    SystemSummary, TempSource, TemperatureSensor,
    // GPU types
    GpuDevice, GpuFan, GpuSnapshot, GpuTemperature, GpuVendor,
};

// Re-export config functions from data/
pub use data::{
    create_default_curve,
};

// Re-export validation functions from data/
pub use data::{
    validate_curve_points, validate_fan_path, validate_file_size, validate_percentage,
    validate_pwm_path, validate_pwm_value, validate_sensor_name, validate_temp_path,
};

// Re-export persistence functions from data/
pub use data::{
    delete_curve, get_curves_path, load_curves, save_curve, save_curves,
    update_curve_points, CurveStore, PersistedCurve,
};

// Re-export error types
pub use error::{HyperfanError, Result};

// Re-export engine types
pub use engine::{CurvePreset, FanCurve};

// Re-export hardware functions from hw/
pub use hw::{
    autodetect_fan_pwm_mappings, autodetect_fan_pwm_mappings_advanced,
    autodetect_fan_pwm_mappings_heuristic, autodetect_with_fingerprints,
    FingerprintedDetectionResult,
    capture_chip_data, capture_raw_snapshot,
    check_pwm_permissions, enable_manual_pwm, enumerate_hwmon_chips, read_fan_rpm,
    read_pwm_value, read_temperature, set_pwm_percent, set_pwm_value, snapshot_to_json,
    snapshot_to_json_compact,
    // GPU functions
    capture_gpu_snapshot, enumerate_gpus, enumerate_gpu_pwm_controllers,
    reset_amd_fan_auto, reset_nvidia_fan_auto, set_amd_fan_speed, set_nvidia_fan_speed,
    set_gpu_fan_speed_by_id, GpuPwmController,
};

// Re-export fingerprint types and functions from hw/fingerprint
pub use hw::fingerprint::{
    // Core enums
    ValidationState, ChipClass, ChannelType, SemanticRole, SensorScope,
    SafeFallbackPolicy, ExpectedUnits, ValueBucket, VarianceProfile, DeltaProfile,
    // Identity structures
    PciIdentity, I2cIdentity, AcpiIdentity,
    // Runtime stats
    RuntimeStats, PwmProbeData,
    // Fingerprint structures
    ChipFingerprint, ChannelFingerprint, PwmChannelFingerprint, ValidatedPwmFanBinding,
    // Extraction functions
    extract_chip_fingerprint, extract_channel_fingerprint, extract_pwm_fingerprint,
    generate_chip_id, generate_channel_id,
    // Validation functions
    validate_chip_fingerprint, validate_channel_fingerprint,
    find_matching_hwmon, find_matching_channel,
    // Runtime stats
    classify_temperature_bucket, update_runtime_stats,
};

// Re-export binding management from hw/binding
pub use hw::binding::{
    BindingStore, ValidationReport, BindingValidationResult, FallbackAction,
    validate_all_bindings, discover_and_fingerprint_system,
    apply_safe_fallbacks, execute_fallback,
};

// Re-export system functions
pub use system::{get_os_name, get_system_summary, get_memory_available_mb, get_memory_total_mb, is_bsd, is_linux};

// Re-export settings functions
pub use settings::{
    AppSettings, DisplaySettings, FanCurvePair, GeneralSettings, PwmFanPairing,
    SensorFriendlyName, PwmHardwareId, FanHardwareId, AdvancedSettings,
    delete_pair, get_active_pairs, get_settings_path, load_settings,
    save_pair, save_settings, update_setting,
    // Cached settings (PERFORMANCE: use these in hot paths like draw functions)
    get_cached_settings, get_graph_style, get_graph_smoothing, get_frame_rate, invalidate_settings_cache,
    // PWM-fan mapping functions
    is_detection_completed, save_pwm_fan_mappings, get_pwm_fan_mappings,
    clear_pwm_fan_mappings,
    // PWM-fan pairing CRUD (UUID-based)
    save_pwm_pairing, delete_pwm_pairing, delete_pwm_pairing_by_path,
    get_pwm_pairing, get_pwm_pairing_by_path, get_all_pwm_pairings,
    update_pwm_pairing_name,
    // Binding store functions
    load_binding_store, save_binding_store, get_binding_store_path, binding_store_exists,
    // Sensor friendly name functions
    get_sensor_friendly_name, set_sensor_friendly_name, get_all_sensor_friendly_names,
    // Hardware identification extraction (CRITICAL for safe pairings)
    extract_pwm_hardware_id, extract_fan_hardware_id,
    // Fingerprinted pairing creation and validation (ZERO DRIFT)
    create_fingerprinted_pairing, validate_pairing, PairingValidation,
    // GUID generation for entity identification
    generate_guid,
    // Window manager detection
    WindowManager, detect_desktop_environment, get_effective_window_manager,
    // Temperature graph persistence
    PersistedGraph, get_temp_graphs_path, load_temp_graphs, save_temp_graphs,
    add_temp_graph, remove_temp_graph,
};

// Re-export service management functions
pub use service::{
    InitSystem, detect_init_system, get_socket_path,
    is_service_installed, is_service_running, is_socket_available,
    install_service, uninstall_service, reinstall_service,
    start_service, stop_service, restart_service,
    get_service_status, find_daemon_binary,
};

// Re-export daemon client types and functions
pub use daemon_client::{
    DaemonClient, DaemonRequest, DaemonResponse, DaemonResponseData,
    DaemonHardwareInfo, DaemonHwmonChip, DaemonTempSensor, DaemonFanSensor,
    DaemonPwmControl, DaemonGpuInfo, DaemonFanMapping,
    DaemonManualPwmFanPairing, DaemonEcChipInfo, DaemonEcRegisterValue,
    is_daemon_available, ping_daemon, get_daemon_version,
    daemon_read_temperature, daemon_read_fan_rpm, daemon_read_pwm,
    daemon_set_pwm, daemon_enable_manual_pwm, daemon_disable_manual_pwm,
    daemon_set_pwm_override, daemon_clear_pwm_override,
    daemon_list_hardware, daemon_list_gpus, daemon_set_gpu_fan, daemon_set_gpu_fan_for_fan,
    daemon_detect_fan_mappings, daemon_reload_config,
    daemon_reset_gpu_fan_auto,
    daemon_get_manual_pairings, daemon_set_manual_pairing, daemon_delete_manual_pairing,
    daemon_list_ec_chips, daemon_read_ec_register, daemon_write_ec_register, daemon_read_ec_register_range,
};

// Re-export display formatting functions
pub use display::{
    format_temp, format_temp_with_unit, format_temp_precise, format_temp_precise_with_unit,
    temp_unit_suffix, celsius_to_fahrenheit, fahrenheit_to_celsius,
    format_fan_speed, format_fan_speed_with_metric, format_fan_speed_f32,
    format_fan_speed_f32_with_metric, percent_to_pwm, pwm_to_percent,
    pwm_to_percent_f32, percent_to_pwm_u8, fan_metric_suffix,
    is_pwm_metric, is_fahrenheit, format_pwm_subtitle,
    format_rpm, format_rpm_optional, format_power, format_memory_mb, format_utilization,
};

// Re-export zero-drift fingerprinting system (GUARANTEES NO HWMON DRIFT)
pub use fingerprinting::{
    // Core types
    HardwareAnchor, FirmwareAnchor, DriverAnchor, AttributeAnchor, RuntimeAnchor,
    ChipFingerprint as FpChipFingerprint, ChannelFingerprint as FpChannelFingerprint,
    PwmChannelFingerprint as FpPwmChannelFingerprint,
    // Anchor types
    PciAnchor, I2cAnchor, AcpiAnchor, UsbAnchor, PlatformAnchor,
    SensorLabelAnchor, DmiAnchor, SensorCapabilities,
    // Enums
    ChipClass as FpChipClass, ChannelType as FpChannelType, SemanticRole as FpSemanticRole,
    ControlAuthority, SafeFallbackPolicy as FpSafeFallbackPolicy,
    // Matching
    MatchResult, MatchConfidence, MatchReason, AnchorTier,
    find_chip_by_fingerprint, find_channel_by_fingerprint,
    // Storage
    FingerprintStore, StoredBinding, ValidationState as FpValidationState,
    // Validation
    ValidationResult as FpValidationResult, validate_binding,
    // Runtime monitoring
    RuntimeValidator, DriftDetector, RuntimeStats as FpRuntimeStats,
    DriftStatus, PwmResponseValidator, ResponseValidation,
    // Validation report
    ValidationReport as FpValidationReport,
    // Constants
    MIN_CONFIDENCE_FOR_CONTROL, CONFIDENCE_WARNING_THRESHOLD, CONFIDENCE_DEGRADED_THRESHOLD,
};
