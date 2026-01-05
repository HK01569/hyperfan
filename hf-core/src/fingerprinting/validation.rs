//! Validation Helper Functions
//!
//! Comprehensive input validation to prevent injection attacks, DoS, and data corruption.

use std::path::Path;

// Security limits to prevent DoS attacks
const MAX_PATH_LENGTH: usize = 4096;
#[allow(dead_code)]
const MAX_TIMESTAMP_MS: u64 = u64::MAX / 2;

// ============================================================================
// Validation Error Types
// ============================================================================

/// Validation error for anchor data
#[derive(Debug, Clone)]
pub enum AnchorValidationError {
    /// String exceeds maximum length
    TooLong { field: String, max: usize, actual: usize },
    /// Value out of acceptable range
    OutOfRange(String),
    /// Invalid format or characters
    InvalidFormat(String),
    /// Path traversal attempt detected
    PathTraversal(String),
    /// Contains non-printable or control characters
    InvalidCharacters(String),
    /// Nested validation error
    NestedError(String, Box<AnchorValidationError>),
}

impl std::fmt::Display for AnchorValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLong { field, max, actual } => {
                write!(f, "Field '{}' too long: {} bytes (max {})", field, actual, max)
            }
            Self::OutOfRange(msg) => write!(f, "Out of range: {}", msg),
            Self::InvalidFormat(msg) => write!(f, "Invalid format: {}", msg),
            Self::PathTraversal(msg) => write!(f, "Path traversal detected: {}", msg),
            Self::InvalidCharacters(msg) => write!(f, "Invalid characters: {}", msg),
            Self::NestedError(context, err) => write!(f, "{}: {}", context, err),
        }
    }
}

impl std::error::Error for AnchorValidationError {}

// ============================================================================
// String Validation
// ============================================================================

/// Validate string length
pub fn validate_string_length(
    s: &str,
    field_name: &str,
    max_length: usize,
) -> Result<(), AnchorValidationError> {
    let len = s.len();
    if len > max_length {
        return Err(AnchorValidationError::TooLong {
            field: field_name.to_string(),
            max: max_length,
            actual: len,
        });
    }
    Ok(())
}

/// Validate string contains only printable ASCII characters
pub fn validate_printable_string(
    s: &str,
    field_name: &str,
) -> Result<(), AnchorValidationError> {
    if !s.chars().all(|c| c.is_ascii_graphic() || c.is_ascii_whitespace()) {
        return Err(AnchorValidationError::InvalidCharacters(format!(
            "Field '{}' contains non-printable characters",
            field_name
        )));
    }
    
    // Check for control characters (except whitespace)
    if s.chars().any(|c| c.is_control() && !c.is_whitespace()) {
        return Err(AnchorValidationError::InvalidCharacters(format!(
            "Field '{}' contains control characters",
            field_name
        )));
    }
    
    Ok(())
}

/// Validate hexadecimal string (with optional 0x prefix)
pub fn validate_hex_string(s: &str, field_name: &str) -> Result<(), AnchorValidationError> {
    let hex_part = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    
    if hex_part.is_empty() {
        return Err(AnchorValidationError::InvalidFormat(format!(
            "Field '{}' is empty",
            field_name
        )));
    }
    
    if !hex_part.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(AnchorValidationError::InvalidFormat(format!(
            "Field '{}' contains non-hexadecimal characters",
            field_name
        )));
    }
    
    Ok(())
}

/// Validate path string (no path traversal, no null bytes)
pub fn validate_path_string(s: &str, field_name: &str) -> Result<(), AnchorValidationError> {
    // Check for null bytes
    if s.contains('\0') {
        return Err(AnchorValidationError::InvalidCharacters(format!(
            "Field '{}' contains null bytes",
            field_name
        )));
    }
    
    // Check for path traversal attempts
    if s.contains("..") {
        return Err(AnchorValidationError::PathTraversal(format!(
            "Field '{}' contains '..' (path traversal)",
            field_name
        )));
    }
    
    // Check for absolute paths trying to escape (multiple leading slashes)
    if s.starts_with("//") {
        return Err(AnchorValidationError::PathTraversal(format!(
            "Field '{}' starts with '//' (network path)",
            field_name
        )));
    }
    
    Ok(())
}

/// Validate PathBuf (comprehensive path security check)
pub fn validate_pathbuf(path: &Path, field_name: &str) -> Result<(), AnchorValidationError> {
    let path_str = path.to_string_lossy();
    
    validate_string_length(&path_str, field_name, MAX_PATH_LENGTH)?;
    validate_path_string(&path_str, field_name)?;
    
    // Additional checks for PathBuf
    for component in path.components() {
        let comp_str = component.as_os_str().to_string_lossy();
        
        // Check each component doesn't contain null bytes or control chars
        if comp_str.contains('\0') {
            return Err(AnchorValidationError::InvalidCharacters(format!(
                "Path '{}' component contains null bytes",
                field_name
            )));
        }
    }
    
    Ok(())
}

// ============================================================================
// Numeric Validation
// ============================================================================

/// Validate timestamp is within reasonable bounds
pub fn validate_timestamp(timestamp_ms: u64, field_name: &str) -> Result<(), AnchorValidationError> {
    if timestamp_ms > MAX_TIMESTAMP_MS {
        return Err(AnchorValidationError::OutOfRange(format!(
            "Timestamp '{}' is too large (possible overflow)",
            field_name
        )));
    }
    
    // Sanity check: timestamp should be after year 2000
    const YEAR_2000_MS: u64 = 946_684_800_000;
    if timestamp_ms > 0 && timestamp_ms < YEAR_2000_MS {
        return Err(AnchorValidationError::OutOfRange(format!(
            "Timestamp '{}' is before year 2000 (suspicious)",
            field_name
        )));
    }
    
    Ok(())
}

/// Validate collection size
pub fn validate_collection_size(
    size: usize,
    field_name: &str,
    max_size: usize,
) -> Result<(), AnchorValidationError> {
    if size > max_size {
        return Err(AnchorValidationError::OutOfRange(format!(
            "Collection '{}' too large: {} items (max {})",
            field_name, size, max_size
        )));
    }
    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_length_validation() {
        assert!(validate_string_length("short", "test", 10).is_ok());
        assert!(validate_string_length("very long string", "test", 5).is_err());
    }

    #[test]
    fn test_printable_string_validation() {
        assert!(validate_printable_string("Hello World", "test").is_ok());
        assert!(validate_printable_string("Hello\x00World", "test").is_err());
        assert!(validate_printable_string("Hello\x1bWorld", "test").is_err());
    }

    #[test]
    fn test_hex_string_validation() {
        assert!(validate_hex_string("0x1234", "test").is_ok());
        assert!(validate_hex_string("ABCDEF", "test").is_ok());
        assert!(validate_hex_string("0xGHIJ", "test").is_err());
        assert!(validate_hex_string("", "test").is_err());
    }

    #[test]
    fn test_path_traversal_detection() {
        assert!(validate_path_string("/valid/path", "test").is_ok());
        assert!(validate_path_string("/path/../traversal", "test").is_err());
        assert!(validate_path_string("//network/path", "test").is_err());
        assert!(validate_path_string("/path/with\0null", "test").is_err());
    }

    #[test]
    fn test_timestamp_validation() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        
        assert!(validate_timestamp(now, "test").is_ok());
        assert!(validate_timestamp(super::super::MAX_TIMESTAMP_MS + 1, "test").is_err());
        assert!(validate_timestamp(100_000, "test").is_err()); // Before year 2000
    }

    #[test]
    fn test_collection_size_validation() {
        assert!(validate_collection_size(10, "test", 100).is_ok());
        assert!(validate_collection_size(101, "test", 100).is_err());
    }
}
