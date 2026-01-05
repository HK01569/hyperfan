//! Unified error handling for Hyperfan
//!
//! This crate provides a single error type used across all Hyperfan components.
//! It uses thiserror for ergonomic error definitions with proper Display and Error trait impls.

use std::io;
use std::path::PathBuf;

/// Result type alias using HyperfanError
pub type Result<T> = std::result::Result<T, HyperfanError>;

/// Unified error type for all Hyperfan operations
#[derive(thiserror::Error, Debug)]
pub enum HyperfanError {
    // ============================================================================
    // I/O and File System Errors
    // ============================================================================
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("Failed to read file {path}: {source}")]
    FileRead {
        path: PathBuf,
        source: io::Error,
    },

    #[error("Failed to write file {path}: {source}")]
    FileWrite {
        path: PathBuf,
        source: io::Error,
    },

    #[error("File not found: {0}")]
    FileNotFound(PathBuf),

    #[error("File too large: {path} ({size} bytes, max {max_size} bytes)")]
    FileTooLarge {
        path: PathBuf,
        size: u64,
        max_size: u64,
    },

    // ============================================================================
    // Path Validation Errors
    // ============================================================================
    #[error("Invalid path {path}: {reason}")]
    InvalidPath {
        path: PathBuf,
        reason: String,
    },

    #[error("Path traversal attempt detected: {0}")]
    PathTraversal(PathBuf),

    #[error("Path not in allowed directory: {0}")]
    PathNotAllowed(PathBuf),

    // ============================================================================
    // Hardware Access Errors
    // ============================================================================
    #[error("Failed to read temperature from {path}: {reason}")]
    TemperatureRead {
        path: PathBuf,
        reason: String,
    },

    #[error("Failed to read fan RPM from {path}: {reason}")]
    FanRead {
        path: PathBuf,
        reason: String,
    },

    #[error("Failed to read PWM from {path}: {reason}")]
    PwmRead {
        path: PathBuf,
        reason: String,
    },

    #[error("Failed to write PWM to {path}: {reason}")]
    PwmWrite {
        path: PathBuf,
        reason: String,
    },

    #[error("Hardware not found: {0}")]
    HardwareNotFound(String),

    #[error("GPU error: {0}")]
    GpuError(String),

    // ============================================================================
    // Configuration and Settings Errors
    // ============================================================================
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Failed to parse JSON: {0}")]
    JsonParse(#[from] serde_json::Error),

    #[error("Invalid configuration value for {field}: {reason}")]
    InvalidConfig {
        field: String,
        reason: String,
    },

    #[error("Missing required configuration: {0}")]
    MissingConfig(String),

    // ============================================================================
    // Validation Errors
    // ============================================================================
    #[error("Invalid PWM value: {value} (must be 0-255)")]
    InvalidPwmValue {
        value: u16,
    },

    #[error("Invalid percentage: {value} (must be 0.0-100.0)")]
    InvalidPercentage {
        value: f32,
    },

    #[error("Invalid temperature: {value}°C")]
    InvalidTemperature {
        value: f32,
    },

    #[error("Invalid sensor name: {0}")]
    InvalidSensorName(String),

    #[error("Curve validation failed: {0}")]
    InvalidCurve(String),

    // ============================================================================
    // Daemon and IPC Errors
    // ============================================================================
    #[error("Daemon not available")]
    DaemonNotAvailable,

    #[error("Daemon connection failed: {0}")]
    DaemonConnection(String),

    #[error("Daemon request failed: {0}")]
    DaemonRequest(String),

    #[error("Daemon response error: {0}")]
    DaemonResponse(String),

    #[error("IPC protocol error: {0}")]
    IpcProtocol(String),

    #[error("Message too large: {size} bytes (max {max_size} bytes)")]
    MessageTooLarge {
        size: usize,
        max_size: usize,
    },

    // ============================================================================
    // Service Management Errors
    // ============================================================================
    #[error("Service error: {0}")]
    Service(String),

    #[error("Service not installed")]
    ServiceNotInstalled,

    #[error("Service not running")]
    ServiceNotRunning,

    #[error("Failed to execute privileged command: {0}")]
    PrivilegeEscalation(String),

    #[error("Init system not supported: {0}")]
    UnsupportedInitSystem(String),

    // ============================================================================
    // Generic Errors
    // ============================================================================
    #[error("{0}")]
    Generic(String),

    #[error("Operation not supported: {0}")]
    NotSupported(String),

    #[error("Operation timed out: {0}")]
    Timeout(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),
}

impl HyperfanError {
    /// Create a generic error from a string
    pub fn generic(msg: impl Into<String>) -> Self {
        Self::Generic(msg.into())
    }

    /// Create a config error from a string
    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }

    /// Create an invalid path error
    pub fn invalid_path(path: impl Into<PathBuf>, reason: impl Into<String>) -> Self {
        Self::InvalidPath {
            path: path.into(),
            reason: reason.into(),
        }
    }

    /// Create a daemon error from a string
    pub fn daemon(msg: impl Into<String>) -> Self {
        Self::DaemonRequest(msg.into())
    }

    /// Create a service error from a string
    pub fn service(msg: impl Into<String>) -> Self {
        Self::Service(msg.into())
    }
}

// Allow converting from String to HyperfanError
impl From<String> for HyperfanError {
    fn from(s: String) -> Self {
        Self::Generic(s)
    }
}

// Allow converting from &str to HyperfanError
impl From<&str> for HyperfanError {
    fn from(s: &str) -> Self {
        Self::Generic(s.to_string())
    }
}
