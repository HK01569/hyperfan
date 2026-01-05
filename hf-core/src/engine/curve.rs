//! Fan curve engine for temperature-based fan control
//!
//! Provides interpolation and hysteresis for smooth, intelligent fan control.
//!
//! # How It Works
//!
//! 1. **Interpolation**: Given a temperature, finds the fan speed by interpolating
//!    between defined curve points (linear interpolation).
//!
//! 2. **Hysteresis**: Prevents rapid fan speed oscillation by requiring temperature
//!    to change by a minimum amount before adjusting speed (default: 2°C).
//!
//! 3. **Smoothing**: Gradual speed changes to avoid jarring fan noise. The smoothing
//!    factor (0.0-0.99) controls how quickly the fan responds to temperature changes.

use std::time::Instant;

use crate::constants::{curve as curve_const, timing};
use crate::data::CurvePoint;

/// Fan curve controller with hysteresis, delay, and asymmetric ramp speeds
#[derive(Debug, Clone)]
pub struct FanCurve {
    points: Vec<CurvePoint>,
    hysteresis: f32,
    smoothing: f32,
    last_output: f32,
    rising: bool,
    min_speed: f32,
    last_update: Option<Instant>,
    /// Delay in milliseconds before responding to temperature changes
    delay_ms: u32,
    /// Ramp up speed in percent per second (0 = instant)
    ramp_up_speed: f32,
    /// Ramp down speed in percent per second (0 = instant)
    ramp_down_speed: f32,
    /// Pending target during delay period
    pending_target: Option<(f32, Instant)>,
    /// Stepped mode: jump instantly to next curve point instead of interpolating
    stepped: bool,
}

impl FanCurve {
    /// Create a new fan curve with the given points
    ///
    /// Uses default hysteresis and smoothing values from constants.
    pub fn new(points: Vec<CurvePoint>) -> Self {
        Self {
            points,
            hysteresis: curve_const::DEFAULT_HYSTERESIS_CELSIUS,
            smoothing: curve_const::DEFAULT_SMOOTHING_FACTOR,
            last_output: 0.0,
            rising: true,
            min_speed: 0.0,
            last_update: None,
            delay_ms: curve_const::DEFAULT_DELAY_MS,
            ramp_up_speed: curve_const::DEFAULT_RAMP_UP_SPEED,
            ramp_down_speed: curve_const::DEFAULT_RAMP_DOWN_SPEED,
            pending_target: None,
            stepped: false,
        }
    }

    /// Enable stepped mode (jump instantly between curve points instead of interpolating)
    pub fn with_stepped(mut self, stepped: bool) -> Self {
        self.stepped = stepped;
        self
    }

    /// Create a curve with custom hysteresis (minimum temperature change to trigger adjustment)
    ///
    /// # Arguments
    /// * `hysteresis` - Temperature delta in °C (e.g., 2.0 means temp must change by 2°C)
    pub fn with_hysteresis(mut self, hysteresis: f32) -> Self {
        self.hysteresis = hysteresis.max(0.0);
        self
    }

    /// Create a curve with custom smoothing factor
    ///
    /// # Arguments
    /// * `smoothing` - Value from 0.0 (instant response) to 0.99 (very gradual)
    pub fn with_smoothing(mut self, smoothing: f32) -> Self {
        self.smoothing = smoothing.clamp(curve_const::MIN_SMOOTHING, curve_const::MAX_SMOOTHING);
        self
    }

    /// Set minimum fan speed (for fans that stall at low PWM)
    pub fn with_min_speed(mut self, min_speed: f32) -> Self {
        self.min_speed = min_speed.clamp(0.0, 100.0);
        self
    }
    
    /// Set delay before responding to temperature changes
    ///
    /// # Arguments
    /// * `delay_ms` - Delay in milliseconds (0 = no delay)
    pub fn with_delay(mut self, delay_ms: u32) -> Self {
        self.delay_ms = delay_ms.min(curve_const::MAX_DELAY_MS);
        self
    }
    
    /// Set asymmetric ramp speeds for fan speed changes
    ///
    /// # Arguments
    /// * `ramp_up` - Speed in percent per second when increasing (0 = instant)
    /// * `ramp_down` - Speed in percent per second when decreasing (0 = instant)
    pub fn with_ramp_speeds(mut self, ramp_up: f32, ramp_down: f32) -> Self {
        self.ramp_up_speed = ramp_up.clamp(curve_const::MIN_RAMP_SPEED, curve_const::MAX_RAMP_SPEED);
        self.ramp_down_speed = ramp_down.clamp(curve_const::MIN_RAMP_SPEED, curve_const::MAX_RAMP_SPEED);
        self
    }

    /// Calculate the target fan speed for a given temperature
    ///
    /// Returns fan speed as percentage (0.0 - 100.0).
    /// If the curve has no points, returns 100% for safety.
    pub fn calculate(&mut self, temp: f32) -> f32 {
        if self.points.is_empty() {
            return curve_const::FALLBACK_FAN_PERCENT;
        }

        let effective_temp = self.apply_hysteresis(temp);
        let raw_output = self.interpolate(effective_temp);
        
        // Apply delay if configured
        let delayed_output = self.apply_delay(raw_output);
        
        // Apply asymmetric ramping
        let ramped_output = self.apply_ramping(delayed_output);
        
        // Apply smoothing on top of ramping
        let smoothed = self.apply_smoothing(ramped_output);

        let final_output = if smoothed > 0.0 && smoothed < self.min_speed {
            self.min_speed
        } else {
            smoothed
        };

        self.last_output = final_output;
        self.last_update = Some(Instant::now());

        final_output
    }

    /// Get the raw interpolated value without hysteresis/smoothing (for preview)
    pub fn preview(&self, temp: f32) -> f32 {
        self.interpolate(temp)
    }

    fn apply_hysteresis(&mut self, temp: f32) -> f32 {
        let last_temp_estimate = self.estimate_temp_from_output();

        if temp > last_temp_estimate + self.hysteresis {
            self.rising = true;
            temp
        } else if temp < last_temp_estimate - self.hysteresis {
            self.rising = false;
            temp
        } else {
            // Within hysteresis band - use last effective temperature
            last_temp_estimate
        }
    }
    
    /// Apply delay before responding to temperature changes
    fn apply_delay(&mut self, target: f32) -> f32 {
        if self.delay_ms == 0 {
            self.pending_target = None;
            return target;
        }
        
        let now = Instant::now();
        
        match self.pending_target {
            Some((pending, start_time)) => {
                // Check if target has changed significantly
                if (target - pending).abs() > 1.0 {
                    // Target changed, reset delay timer
                    self.pending_target = Some((target, now));
                    return self.last_output;
                }
                
                // Check if delay has elapsed
                let elapsed_ms = start_time.elapsed().as_millis() as u32;
                if elapsed_ms >= self.delay_ms {
                    self.pending_target = None;
                    target
                } else {
                    // Still waiting
                    self.last_output
                }
            }
            None => {
                // No pending target - start delay if target differs from current
                if (target - self.last_output).abs() > 1.0 {
                    self.pending_target = Some((target, now));
                    self.last_output
                } else {
                    target
                }
            }
        }
    }
    
    /// Apply asymmetric ramping to fan speed changes
    fn apply_ramping(&self, target: f32) -> f32 {
        let last_update = match self.last_update {
            Some(instant) => instant,
            None => return target, // First call, go directly to target
        };
        
        let elapsed_secs = last_update.elapsed().as_secs_f32();
        let diff = target - self.last_output;
        
        if diff.abs() < 0.1 {
            return target; // Close enough
        }
        
        let ramp_speed = if diff > 0.0 {
            self.ramp_up_speed
        } else {
            self.ramp_down_speed
        };
        
        // If ramp speed is 0, instant change
        if ramp_speed <= 0.0 {
            return target;
        }
        
        // Calculate maximum change allowed in this time step
        let max_change = ramp_speed * elapsed_secs;
        
        if diff.abs() <= max_change {
            target
        } else if diff > 0.0 {
            self.last_output + max_change
        } else {
            self.last_output - max_change
        }
    }

    /// Estimate what temperature would produce the current fan output
    ///
    /// This is the inverse of interpolation - given fan speed, estimate temperature.
    /// Used for hysteresis calculations.
    fn estimate_temp_from_output(&self) -> f32 {
        for window in self.points.windows(2) {
            let lower_point = &window[0];
            let upper_point = &window[1];

            // Check if current output falls between these two points
            if self.last_output >= lower_point.fan_percent
                && self.last_output <= upper_point.fan_percent
            {
                let fan_range = upper_point.fan_percent - lower_point.fan_percent;

                // Avoid division by zero
                if fan_range.abs() < curve_const::FLOAT_EPSILON {
                    return lower_point.temperature;
                }

                // Reverse interpolation
                let ratio = (self.last_output - lower_point.fan_percent) / fan_range;
                let temp_range = upper_point.temperature - lower_point.temperature;
                return lower_point.temperature + (ratio * temp_range);
            }
        }

        // Output is outside curve range - return boundary temperature
        let min_fan_speed = self.points.first().map(|p| p.fan_percent).unwrap_or(0.0);
        if self.last_output <= min_fan_speed {
            self.points.first().map(|p| p.temperature).unwrap_or(0.0)
        } else {
            // BUG FIX: Should return temperature, not FALLBACK_FAN_PERCENT
            self.points
                .last()
                .map(|p| p.temperature)
                .unwrap_or(100.0)  // Return max temp, not fan percent
        }
    }

    /// Linearly interpolate fan speed between curve points
    ///
    /// - Below minimum temp: returns lowest defined fan speed
    /// - Above maximum temp: returns highest defined fan speed
    /// - Between points: linear interpolation
    fn interpolate(&self, current_temp: f32) -> f32 {
        if self.points.is_empty() {
            return curve_const::FALLBACK_FAN_PERCENT;
        }

        let first_point = &self.points[0];
        let last_point = match self.points.last() {
            Some(p) => p,
            None => return curve_const::FALLBACK_FAN_PERCENT, // Should never happen after is_empty check, but be safe
        };

        // Below the curve - use minimum defined speed
        if current_temp <= first_point.temperature {
            return first_point.fan_percent;
        }

        // Above the curve - use maximum defined speed
        if current_temp >= last_point.temperature {
            return last_point.fan_percent;
        }

        // Find the two points that bracket the current temperature
        for window in self.points.windows(2) {
            // Safe: windows(2) guarantees exactly 2 elements
            let lower_point = &window[0];
            let upper_point = &window[1];

            if current_temp >= lower_point.temperature && current_temp <= upper_point.temperature {
                // Stepped mode: use the lower point's fan speed until we reach the next point
                if self.stepped {
                    return lower_point.fan_percent;
                }

                let temp_range = upper_point.temperature - lower_point.temperature;

                // Avoid division by zero for overlapping points
                if temp_range.abs() < curve_const::FLOAT_EPSILON {
                    return lower_point.fan_percent;
                }

                // Linear interpolation: how far between the two points (0.0 to 1.0)
                let interpolation_ratio = (current_temp - lower_point.temperature) / temp_range;
                let fan_range = upper_point.fan_percent - lower_point.fan_percent;

                return lower_point.fan_percent + (interpolation_ratio * fan_range);
            }
        }

        // Should never reach here, but return safe value
        curve_const::FALLBACK_FAN_PERCENT
    }

    fn apply_smoothing(&self, target: f32) -> f32 {
        let last_update = match self.last_update {
            Some(instant) => instant,
            None => return target,
        };

        let elapsed = last_update.elapsed();
        let time_factor = (elapsed.as_secs_f32() / timing::CURVE_UPDATE_INTERVAL.as_secs_f32())
            .min(1.0);

        let effective_smoothing = self.smoothing * (1.0 - time_factor);

        self.last_output * effective_smoothing + target * (1.0 - effective_smoothing)
    }

    /// Get the current curve points
    pub fn points(&self) -> &[CurvePoint] {
        &self.points
    }

    /// Update the curve points
    pub fn set_points(&mut self, points: Vec<CurvePoint>) {
        self.points = points;
    }

    /// Reset the curve state (smoothing history, delay, ramping)
    pub fn reset(&mut self) {
        self.last_output = 0.0;
        self.rising = true;
        self.last_update = None;
        self.pending_target = None;
    }
}

impl Default for FanCurve {
    fn default() -> Self {
        Self::new(crate::constants::default_curve::balanced())
    }
}

/// Preset curve profiles
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurvePreset {
    Quiet,
    Balanced,
    Performance,
    FullSpeed,
    Custom,
}

impl CurvePreset {
    /// Get the curve points for this preset
    pub fn points(&self) -> Vec<CurvePoint> {
        use crate::constants::default_curve;
        match self {
            CurvePreset::Quiet => default_curve::quiet(),
            CurvePreset::Balanced => default_curve::balanced(),
            CurvePreset::Performance => default_curve::performance(),
            CurvePreset::FullSpeed => default_curve::full_speed(),
            CurvePreset::Custom => default_curve::balanced(),
        }
    }

    /// Create a FanCurve from this preset
    pub fn to_curve(&self) -> FanCurve {
        FanCurve::new(self.points())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_curve() -> FanCurve {
        FanCurve::new(vec![
            CurvePoint { temperature: 30.0, fan_percent: 20.0 },
            CurvePoint { temperature: 50.0, fan_percent: 50.0 },
            CurvePoint { temperature: 70.0, fan_percent: 80.0 },
            CurvePoint { temperature: 80.0, fan_percent: 100.0 },
        ])
    }

    #[test]
    fn test_interpolation_at_points() {
        let mut curve = test_curve();
        assert!((curve.calculate(30.0) - 20.0).abs() < 1.0);
        assert!((curve.calculate(50.0) - 50.0).abs() < 1.0);
    }

    #[test]
    fn test_interpolation_between_points() {
        let mut curve = test_curve();
        let result = curve.calculate(40.0);
        assert!(result > 20.0 && result < 50.0);
    }

    #[test]
    fn test_below_curve() {
        let mut curve = test_curve();
        assert!((curve.calculate(20.0) - 20.0).abs() < 1.0);
    }

    #[test]
    fn test_above_curve() {
        let mut curve = test_curve();
        assert!((curve.calculate(90.0) - 100.0).abs() < 1.0);
    }
}
