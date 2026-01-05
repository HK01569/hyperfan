//! Configuration management
//!
//! Handles loading and saving configuration data.

use crate::data::types::CurvePoint;

/// Create a default balanced fan curve
///
/// Returns the same curve as `constants::default_curve::balanced()`.
/// Prefer using the constant directly where possible.
pub fn create_default_curve() -> Vec<CurvePoint> {
    crate::constants::default_curve::balanced()
}
