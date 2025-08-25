/*
 * Test utilities and mock helpers for Hyperfan
 *
 * This module provides common test utilities, mock objects, and helper functions
 * that can be used across different test modules.
 */

#[cfg(test)]
pub mod test_utils {
    use crate::hwmon::{ChipReadings, FanPwmPairing, SensorInventory};
    use crate::config::{SavedConfig, SavedMapping, Metric, ControllerGroup};
    use crate::curves::{CurvesConfig, CurveGroup, CurveSpec, CurvePoint};
    use crate::app::{App, Mapping};
    use std::collections::HashMap;
    use tempfile::{TempDir, NamedTempFile};
    use std::fs;
    use std::io::Write;

    /// Creates a mock ChipReadings for testing
    pub fn create_mock_chip_readings(name: &str, hwmon_suffix: &str) -> ChipReadings {
        ChipReadings {
            name: format!("{}@{}", name, hwmon_suffix),
            temps: vec![
                ("temp1".to_string(), 45.5),
                ("temp2".to_string(), 38.2),
                ("CPU Temp".to_string(), 52.0),
            ],
            fans: vec![
                ("fan1".to_string(), 1200),
                ("fan2".to_string(), 800),
                ("CPU Fan".to_string(), 1500),
            ],
            pwms: vec![
                ("pwm1".to_string(), 128),
                ("pwm2".to_string(), 200),
                ("CPU PWM".to_string(), 180),
            ],
        }
    }

    /// Creates a mock SensorInventory for testing
    pub fn create_mock_sensor_inventory() -> SensorInventory {
        SensorInventory {
            fans: vec![
                ("chip1@hwmon0".to_string(), 1, "fan1".to_string()),
                ("chip1@hwmon0".to_string(), 2, "CPU Fan".to_string()),
                ("chip2@hwmon1".to_string(), 1, "case_fan".to_string()),
            ],
            pwms: vec![
                ("chip1@hwmon0".to_string(), 1, "pwm1".to_string()),
                ("chip1@hwmon0".to_string(), 2, "CPU PWM".to_string()),
                ("chip2@hwmon1".to_string(), 1, "case_pwm".to_string()),
            ],
            temps: vec![
                ("chip1@hwmon0".to_string(), 1, "temp1".to_string()),
                ("chip1@hwmon0".to_string(), 2, "CPU Temp".to_string()),
                ("chip2@hwmon1".to_string(), 1, "case_temp".to_string()),
            ],
        }
    }

    /// Creates a mock FanPwmPairing for testing
    pub fn create_mock_fan_pwm_pairing(confidence: f64) -> FanPwmPairing {
        FanPwmPairing {
            fan_chip: "chip1@hwmon0".to_string(),
            fan_idx: 1,
            fan_label: "CPU Fan".to_string(),
            pwm_chip: "chip1@hwmon0".to_string(),
            pwm_idx: 1,
            pwm_label: "CPU PWM".to_string(),
            confidence,
        }
    }

    /// Creates a mock CurveSpec for testing
    pub fn create_mock_curve_spec() -> CurveSpec {
        CurveSpec {
            points: vec![
                CurvePoint { temp_c: 30.0, pwm_pct: 20 },
                CurvePoint { temp_c: 50.0, pwm_pct: 50 },
                CurvePoint { temp_c: 70.0, pwm_pct: 80 },
                CurvePoint { temp_c: 85.0, pwm_pct: 100 },
            ],
            min_pwm_pct: 10,
            max_pwm_pct: 100,
            floor_pwm_pct: 15,
            hysteresis_pct: 5,
            write_min_delta: 3,
            apply_delay_ms: 1000,
        }
    }

    /// Creates a mock CurveGroup for testing
    pub fn create_mock_curve_group(name: &str) -> CurveGroup {
        CurveGroup {
            name: name.to_string(),
            members: vec![
                "chip1@hwmon0:pwm1".to_string(),
                "chip1@hwmon0:CPU PWM".to_string(),
            ],
            temp_source: "chip1@hwmon0:CPU Temp".to_string(),
            curve: create_mock_curve_spec(),
        }
    }

    /// Creates a mock CurvesConfig for testing
    pub fn create_mock_curves_config() -> CurvesConfig {
        CurvesConfig {
            version: 1,
            groups: vec![
                create_mock_curve_group("cpu_fans"),
                create_mock_curve_group("case_fans"),
            ],
        }
    }

    /// Creates a mock SavedConfig for testing
    pub fn create_mock_saved_config() -> SavedConfig {
        let mut fan_aliases = HashMap::new();
        fan_aliases.insert("chip1@hwmon0:fan1".to_string(), "CPU Fan".to_string());
        fan_aliases.insert("chip2@hwmon1:fan1".to_string(), "Case Fan".to_string());

        let mut pwm_aliases = HashMap::new();
        pwm_aliases.insert("chip1@hwmon0:pwm1".to_string(), "CPU PWM".to_string());
        pwm_aliases.insert("chip2@hwmon1:pwm1".to_string(), "Case PWM".to_string());

        let mut temp_aliases = HashMap::new();
        temp_aliases.insert("chip1@hwmon0:temp1".to_string(), "CPU Temp".to_string());
        temp_aliases.insert("chip2@hwmon1:temp1".to_string(), "Case Temp".to_string());

        let mut pwm_overrides = HashMap::new();
        pwm_overrides.insert("chip1@hwmon0:pwm1".to_string(), 75);

        SavedConfig {
            mappings: vec![
                SavedMapping {
                    fan: "chip1@hwmon0:fan1".to_string(),
                    pwm: "chip1@hwmon0:pwm1".to_string(),
                },
                SavedMapping {
                    fan: "chip2@hwmon1:fan1".to_string(),
                    pwm: "chip2@hwmon1:pwm1".to_string(),
                },
            ],
            metric: Metric::C,
            curves: Some(create_mock_curves_config()),
            fan_aliases,
            pwm_aliases,
            temp_aliases,
            controller_groups: vec![
                ControllerGroup {
                    name: "CPU Controllers".to_string(),
                    members: vec!["chip1@hwmon0:pwm1".to_string()],
                },
                ControllerGroup {
                    name: "Case Controllers".to_string(),
                    members: vec!["chip2@hwmon1:pwm1".to_string()],
                },
            ],
            pwm_overrides,
        }
    }

    /// Creates a mock App with test data
    pub fn create_mock_app() -> App {
        let mut app = App::new();
        
        // Add mock readings
        app.readings = vec![
            create_mock_chip_readings("chip1", "hwmon0"),
            create_mock_chip_readings("chip2", "hwmon1"),
        ];
        
        // Add mock mappings
        app.mappings = vec![
            Mapping {
                fan: "chip1@hwmon0:fan1".to_string(),
                pwm: "chip1@hwmon0:pwm1".to_string(),
            },
            Mapping {
                fan: "chip2@hwmon1:fan1".to_string(),
                pwm: "chip2@hwmon1:pwm1".to_string(),
            },
        ];
        
        // Set up aliases
        app.fan_aliases.insert("chip1@hwmon0:fan1".to_string(), "CPU Fan".to_string());
        app.pwm_aliases.insert("chip1@hwmon0:pwm1".to_string(), "CPU PWM".to_string());
        app.temp_aliases.insert("chip1@hwmon0:temp1".to_string(), "CPU Temp".to_string());
        
        app
    }

    /// Creates a temporary file with JSON content
    pub fn create_temp_json_file<T: serde::Serialize>(data: &T) -> NamedTempFile {
        let mut temp_file = NamedTempFile::new().unwrap();
        let json = serde_json::to_string_pretty(data).unwrap();
        temp_file.write_all(json.as_bytes()).unwrap();
        temp_file.flush().unwrap();
        temp_file
    }

    /// Creates a temporary directory with mock hwmon structure
    pub fn create_mock_hwmon_dir() -> TempDir {
        let temp_dir = TempDir::new().unwrap();
        let hwmon_root = temp_dir.path().join("sys/class/hwmon");
        
        // Create hwmon0 directory
        let hwmon0 = hwmon_root.join("hwmon0");
        fs::create_dir_all(&hwmon0).unwrap();
        fs::write(hwmon0.join("name"), "chip1").unwrap();
        fs::write(hwmon0.join("fan1_input"), "1200").unwrap();
        fs::write(hwmon0.join("fan1_label"), "CPU Fan").unwrap();
        fs::write(hwmon0.join("pwm1"), "128").unwrap();
        fs::write(hwmon0.join("pwm1_label"), "CPU PWM").unwrap();
        fs::write(hwmon0.join("temp1_input"), "45500").unwrap();
        fs::write(hwmon0.join("temp1_label"), "CPU Temp").unwrap();
        
        // Create hwmon1 directory
        let hwmon1 = hwmon_root.join("hwmon1");
        fs::create_dir_all(&hwmon1).unwrap();
        fs::write(hwmon1.join("name"), "chip2").unwrap();
        fs::write(hwmon1.join("fan1_input"), "800").unwrap();
        fs::write(hwmon1.join("fan1_label"), "Case Fan").unwrap();
        fs::write(hwmon1.join("pwm1"), "200").unwrap();
        fs::write(hwmon1.join("pwm1_label"), "Case PWM").unwrap();
        fs::write(hwmon1.join("temp1_input"), "38200").unwrap();
        fs::write(hwmon1.join("temp1_label"), "Case Temp").unwrap();
        
        temp_dir
    }

    /// Asserts that two floating point numbers are approximately equal
    pub fn assert_approx_eq(a: f64, b: f64, tolerance: f64) {
        assert!(
            (a - b).abs() < tolerance,
            "Values {} and {} are not approximately equal (tolerance: {})",
            a, b, tolerance
        );
    }

    /// Asserts that a slice contains a specific item
    pub fn assert_contains<T: PartialEq + std::fmt::Debug>(slice: &[T], item: &T) {
        assert!(
            slice.contains(item),
            "Slice {:?} does not contain item {:?}",
            slice, item
        );
    }

    /// Asserts that a HashMap contains a specific key-value pair
    pub fn assert_map_contains<K, V>(map: &HashMap<K, V>, key: &K, value: &V)
    where
        K: std::hash::Hash + Eq + std::fmt::Debug,
        V: PartialEq + std::fmt::Debug,
    {
        match map.get(key) {
            Some(v) if v == value => {},
            Some(v) => panic!("Map contains key {:?} but with value {:?}, expected {:?}", key, v, value),
            None => panic!("Map does not contain key {:?}", key),
        }
    }

    /// Creates a test temperature curve with realistic values
    pub fn create_realistic_temp_curve() -> Vec<CurvePoint> {
        vec![
            CurvePoint { temp_c: 20.0, pwm_pct: 0 },   // Idle
            CurvePoint { temp_c: 40.0, pwm_pct: 20 },  // Light load
            CurvePoint { temp_c: 60.0, pwm_pct: 50 },  // Medium load
            CurvePoint { temp_c: 75.0, pwm_pct: 80 },  // High load
            CurvePoint { temp_c: 85.0, pwm_pct: 100 }, // Maximum
        ]
    }

    /// Validates that a temperature curve is monotonic (non-decreasing)
    pub fn validate_curve_monotonic(points: &[CurvePoint]) -> bool {
        points.windows(2).all(|w| w[0].temp_c <= w[1].temp_c)
    }

    /// Validates that PWM percentages are within valid range
    pub fn validate_pwm_range(points: &[CurvePoint]) -> bool {
        points.iter().all(|p| p.pwm_pct <= 100)
    }
}

#[cfg(test)]
mod tests {
    use super::test_utils::*;
    use crate::curves::interp_pwm_percent;

    #[test]
    fn test_mock_chip_readings() {
        let readings = create_mock_chip_readings("test_chip", "hwmon0");
        assert_eq!(readings.name, "test_chip@hwmon0");
        assert_eq!(readings.temps.len(), 3);
        assert_eq!(readings.fans.len(), 3);
        assert_eq!(readings.pwms.len(), 3);
    }

    #[test]
    fn test_mock_sensor_inventory() {
        let inventory = create_mock_sensor_inventory();
        assert_eq!(inventory.fans.len(), 3);
        assert_eq!(inventory.pwms.len(), 3);
        assert_eq!(inventory.temps.len(), 3);
    }

    #[test]
    fn test_mock_saved_config() {
        let config = create_mock_saved_config();
        assert_eq!(config.mappings.len(), 2);
        assert!(config.curves.is_some());
        assert!(!config.fan_aliases.is_empty());
        assert!(!config.pwm_aliases.is_empty());
        assert!(!config.controller_groups.is_empty());
    }

    #[test]
    fn test_mock_app() {
        let app = create_mock_app();
        assert_eq!(app.readings.len(), 2);
        assert_eq!(app.mappings.len(), 2);
        assert!(!app.fan_aliases.is_empty());
    }

    #[test]
    fn test_realistic_temp_curve() {
        let curve = create_realistic_temp_curve();
        assert!(validate_curve_monotonic(&curve));
        assert!(validate_pwm_range(&curve));
        
        // Test realistic interpolation at 65°C
        let result = interp_pwm_percent(&curve, 65.0);
        assert_eq!(result, 60); // Should interpolate between 60°C->50% and 75°C->80%
        assert_eq!(interp_pwm_percent(&curve, 50.0), 35);  // Between 40°C->20% and 60°C->50%
        assert_eq!(interp_pwm_percent(&curve, 70.0), 70);  // Between 60°C->50% and 75°C->80%
    }

    #[test]
    fn test_assert_approx_eq() {
        assert_approx_eq(1.0, 1.001, 0.01);
        assert_approx_eq(25.5, 25.49, 0.1);
    }

    #[test]
    #[should_panic]
    fn test_assert_approx_eq_fails() {
        assert_approx_eq(1.0, 1.1, 0.01);
    }

    #[test]
    fn test_assert_contains() {
        let vec = vec![1, 2, 3, 4, 5];
        assert_contains(&vec, &3);
        assert_contains(&vec, &1);
        assert_contains(&vec, &5);
    }

    #[test]
    fn test_assert_map_contains() {
        let mut map = std::collections::HashMap::new();
        map.insert("key1", "value1");
        map.insert("key2", "value2");
        
        assert_map_contains(&map, &"key1", &"value1");
        assert_map_contains(&map, &"key2", &"value2");
    }
}
