//! Runtime Validation and Drift Detection
//!
//! This module provides continuous runtime monitoring to detect hardware changes,
//! sensor drift, or mispairing that could occur during system operation.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use super::anchors::*;

// ============================================================================
// Runtime Validator
// ============================================================================

/// Runtime validator for continuous monitoring
pub struct RuntimeValidator {
    /// Last validation time for each binding
    validation_times: HashMap<String, Instant>,
    
    /// Validation interval
    validation_interval: Duration,
    
    /// Drift detector
    drift_detector: DriftDetector,
}

impl RuntimeValidator {
    /// Create a new runtime validator
    pub fn new(validation_interval: Duration) -> Self {
        Self {
            validation_times: HashMap::new(),
            validation_interval,
            drift_detector: DriftDetector::new(),
        }
    }

    /// Check if a binding needs validation
    pub fn needs_validation(&self, pwm_id: &str) -> bool {
        match self.validation_times.get(pwm_id) {
            Some(last_time) => last_time.elapsed() >= self.validation_interval,
            None => true,
        }
    }

    /// Mark a binding as validated
    pub fn mark_validated(&mut self, pwm_id: String) {
        self.validation_times.insert(pwm_id, Instant::now());
    }

    /// Get drift detector
    pub fn drift_detector(&mut self) -> &mut DriftDetector {
        &mut self.drift_detector
    }
}

// ============================================================================
// Drift Detector
// ============================================================================

/// Detects sensor drift and hardware changes during runtime
pub struct DriftDetector {
    /// Runtime statistics for each sensor
    sensor_stats: HashMap<String, RuntimeStats>,
    
    /// Drift detection threshold
    drift_threshold: f32,
}

impl DriftDetector {
    /// Create a new drift detector
    pub fn new() -> Self {
        Self {
            sensor_stats: HashMap::new(),
            drift_threshold: 0.15, // 15% deviation triggers warning
        }
    }

    /// Record a sensor reading
    pub fn record_reading(&mut self, sensor_id: String, value: f32, timestamp_ms: u64) {
        let stats = self.sensor_stats.entry(sensor_id.clone()).or_insert_with(RuntimeStats::new);
        stats.add_sample(value, timestamp_ms);
    }

    /// Check if a sensor has drifted
    pub fn check_drift(&self, sensor_id: &str, current_value: f32) -> DriftStatus {
        if let Some(stats) = self.sensor_stats.get(sensor_id) {
            if let Some((min, max)) = stats.value_range {
                let range = max - min;
                if range > 0.0 {
                    let deviation = if current_value < min {
                        (min - current_value) / range
                    } else if current_value > max {
                        (current_value - max) / range
                    } else {
                        0.0
                    };

                    if deviation > self.drift_threshold * 2.0 {
                        return DriftStatus::Critical(deviation);
                    } else if deviation > self.drift_threshold {
                        return DriftStatus::Warning(deviation);
                    }
                }
            }
        }

        DriftStatus::Normal
    }

    /// Get statistics for a sensor
    pub fn get_stats(&self, sensor_id: &str) -> Option<&RuntimeStats> {
        self.sensor_stats.get(sensor_id)
    }

    /// Clear statistics for a sensor
    pub fn clear_stats(&mut self, sensor_id: &str) {
        self.sensor_stats.remove(sensor_id);
    }

    /// Clear all statistics
    pub fn clear_all(&mut self) {
        self.sensor_stats.clear();
    }
}

impl Default for DriftDetector {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Runtime Statistics
// ============================================================================

/// Runtime statistics for a sensor
#[derive(Debug, Clone)]
pub struct RuntimeStats {
    /// Number of samples collected
    pub sample_count: u64,
    
    /// Value range (min, max)
    pub value_range: Option<(f32, f32)>,
    
    /// Running average
    pub average: f32,
    
    /// Running variance
    pub variance: f32,
    
    /// First sample timestamp
    pub first_sample_at: Option<u64>,
    
    /// Last sample timestamp
    pub last_sample_at: Option<u64>,
    
    /// Sample history (limited size)
    history: Vec<(f32, u64)>,
    
    /// Maximum history size
    max_history: usize,
}

impl RuntimeStats {
    /// Create new runtime statistics
    pub fn new() -> Self {
        Self {
            sample_count: 0,
            value_range: None,
            average: 0.0,
            variance: 0.0,
            first_sample_at: None,
            last_sample_at: None,
            history: Vec::new(),
            max_history: 100,
        }
    }

    /// Add a sample
    pub fn add_sample(&mut self, value: f32, timestamp_ms: u64) {
        // Update sample count
        self.sample_count += 1;

        // Update timestamps
        if self.first_sample_at.is_none() {
            self.first_sample_at = Some(timestamp_ms);
        }
        self.last_sample_at = Some(timestamp_ms);

        // Update range
        if let Some((min, max)) = &mut self.value_range {
            *min = min.min(value);
            *max = max.max(value);
        } else {
            self.value_range = Some((value, value));
        }

        // Update running average (exponential moving average)
        let alpha = 0.1; // Smoothing factor
        if self.sample_count == 1 {
            self.average = value;
        } else {
            self.average = alpha * value + (1.0 - alpha) * self.average;
        }

        // Update variance (simplified)
        let diff = value - self.average;
        self.variance = alpha * (diff * diff) + (1.0 - alpha) * self.variance;

        // Add to history
        self.history.push((value, timestamp_ms));
        if self.history.len() > self.max_history {
            self.history.remove(0);
        }
    }

    /// Get recent samples
    pub fn recent_samples(&self, count: usize) -> &[(f32, u64)] {
        let start = self.history.len().saturating_sub(count);
        &self.history[start..]
    }

    /// Check if sensor appears frozen (no variance)
    pub fn is_frozen(&self) -> bool {
        self.sample_count > 10 && self.variance < 0.01
    }

    /// Check if sensor is noisy (high variance)
    pub fn is_noisy(&self) -> bool {
        if let Some((min, max)) = self.value_range {
            let range = max - min;
            if range > 0.0 {
                let cv = self.variance.sqrt() / self.average.abs().max(1.0);
                return cv > 0.5; // Coefficient of variation > 50%
            }
        }
        false
    }

    /// Get standard deviation
    pub fn std_dev(&self) -> f32 {
        self.variance.sqrt()
    }
}

impl Default for RuntimeStats {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Drift Status
// ============================================================================

/// Status of drift detection
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DriftStatus {
    /// No drift detected
    Normal,
    /// Warning level drift detected
    Warning(f32),
    /// Critical drift detected
    Critical(f32),
}

impl DriftStatus {
    /// Check if drift is problematic
    pub fn is_problematic(&self) -> bool {
        !matches!(self, Self::Normal)
    }

    /// Get drift severity (0.0 - 1.0)
    pub fn severity(&self) -> f32 {
        match self {
            Self::Normal => 0.0,
            Self::Warning(d) => d.min(1.0),
            Self::Critical(d) => d.min(1.0),
        }
    }
}

// ============================================================================
// PWM Response Validator
// ============================================================================

/// Validates PWM-to-fan response to detect mispairing
pub struct PwmResponseValidator {
    /// Expected response signature
    expected_signature: Option<RuntimeAnchor>,
    
    /// Tolerance for response matching
    tolerance: f32,
}

impl PwmResponseValidator {
    /// Create a new response validator
    pub fn new(expected_signature: Option<RuntimeAnchor>) -> Self {
        Self {
            expected_signature,
            tolerance: 0.20, // 20% tolerance
        }
    }

    /// Validate a PWM response against expected signature
    pub fn validate_response(
        &self,
        pwm_value: u8,
        measured_rpm: u32,
    ) -> ResponseValidation {
        if let Some(ref signature) = self.expected_signature {
            // Find closest PWM value in response curve
            let closest = signature.response_curve.iter()
                .min_by_key(|(pwm, _)| (*pwm as i16 - pwm_value as i16).abs())
                .map(|(_, rpm)| *rpm);

            if let Some(expected_rpm) = closest {
                if expected_rpm > 0 {
                    let deviation = ((measured_rpm as f32 - expected_rpm as f32) / expected_rpm as f32).abs();
                    
                    if deviation > self.tolerance * 2.0 {
                        return ResponseValidation::Mismatch(deviation);
                    } else if deviation > self.tolerance {
                        return ResponseValidation::Degraded(deviation);
                    } else {
                        return ResponseValidation::Match;
                    }
                }
            }
        }

        ResponseValidation::Unknown
    }

    /// Update expected signature
    pub fn update_signature(&mut self, signature: RuntimeAnchor) {
        self.expected_signature = Some(signature);
    }
}

/// Result of response validation
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResponseValidation {
    /// Response matches expected signature
    Match,
    /// Response is degraded but acceptable
    Degraded(f32),
    /// Response does not match - possible mispairing
    Mismatch(f32),
    /// No signature to compare against
    Unknown,
}

impl ResponseValidation {
    /// Check if validation failed
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Mismatch(_))
    }

    /// Get deviation amount
    pub fn deviation(&self) -> f32 {
        match self {
            Self::Match => 0.0,
            Self::Degraded(d) | Self::Mismatch(d) => *d,
            Self::Unknown => 0.0,
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_stats() {
        let mut stats = RuntimeStats::new();
        
        stats.add_sample(50.0, 1000);
        stats.add_sample(55.0, 2000);
        stats.add_sample(52.0, 3000);
        
        assert_eq!(stats.sample_count, 3);
        assert!(stats.value_range.is_some());
        assert!(stats.average > 0.0);
    }

    #[test]
    fn test_drift_detection() {
        let mut detector = DriftDetector::new();
        
        detector.record_reading("temp1".to_string(), 50.0, 1000);
        detector.record_reading("temp1".to_string(), 52.0, 2000);
        detector.record_reading("temp1".to_string(), 51.0, 3000);
        
        // Normal reading
        let status = detector.check_drift("temp1", 51.5);
        assert_eq!(status, DriftStatus::Normal);
        
        // Drifted reading
        let status = detector.check_drift("temp1", 100.0);
        assert!(status.is_problematic());
    }

    #[test]
    fn test_frozen_sensor() {
        let mut stats = RuntimeStats::new();
        
        for i in 0..20 {
            stats.add_sample(50.0, i * 1000);
        }
        
        assert!(stats.is_frozen());
    }

    #[test]
    fn test_response_validation() {
        let signature = RuntimeAnchor {
            response_curve: vec![(0, 500), (128, 1500), (255, 3000)],
            response_time_ms: 1000,
            rpm_variance: 50.0,
            min_pwm: Some(0),
            max_rpm: Some(3000),
            signature_hash: 0,
        };
        
        let validator = PwmResponseValidator::new(Some(signature));
        
        // Matching response
        let result = validator.validate_response(128, 1500);
        assert_eq!(result, ResponseValidation::Match);
        
        // Slightly off response
        let result = validator.validate_response(128, 1700);
        assert!(matches!(result, ResponseValidation::Degraded(_)));
        
        // Mismatched response
        let result = validator.validate_response(128, 500);
        assert!(matches!(result, ResponseValidation::Mismatch(_)));
    }
}
