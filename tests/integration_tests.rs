/*
 * Integration tests for Hyperfan
 *
 * These tests verify the interaction between different modules
 * and test the application's behavior as a whole.
 */

use hyperfan::config::{SavedConfig, SavedMapping, Metric, validate_saved_config};
use hyperfan::curves::{CurvesConfig, CurveGroup, CurveSpec, CurvePoint, validate_curves, interp_pwm_percent};
use hyperfan::app::{App, Mapping, Focus};
use hyperfan::hwmon::{ChipReadings, HwmonError, extract_index};
use std::collections::HashMap;
use serial_test::serial;

// Test utilities
fn create_test_config_with_curves() -> SavedConfig {
    let curve_spec = CurveSpec {
        points: vec![
            CurvePoint { temp_c: 30.0, pwm_pct: 20 },
            CurvePoint { temp_c: 50.0, pwm_pct: 50 },
            CurvePoint { temp_c: 70.0, pwm_pct: 80 },
        ],
        min_pwm_pct: 10,
        max_pwm_pct: 90,
        floor_pwm_pct: 15,
        hysteresis_pct: 5,
        write_min_delta: 3,
        apply_delay_ms: 1000,
    };

    let curve_group = CurveGroup {
        name: "cpu_fans".to_string(),
        members: vec!["chip1:pwm1".to_string(), "chip1:pwm2".to_string()],
        temp_source: "chip1:temp1".to_string(),
        curve: curve_spec,
    };

    let curves_config = CurvesConfig {
        version: 1,
        groups: vec![curve_group],
    };

    let mut fan_aliases = HashMap::new();
    fan_aliases.insert("chip1:fan1".to_string(), "CPU Fan".to_string());

    let mut pwm_aliases = HashMap::new();
    pwm_aliases.insert("chip1:pwm1".to_string(), "CPU PWM".to_string());

    SavedConfig {
        mappings: vec![SavedMapping {
            fan: "chip1:fan1".to_string(),
            pwm: "chip1:pwm1".to_string(),
        }],
        metric: Metric::C,
        curves: Some(curves_config),
        fan_aliases,
        pwm_aliases,
        temp_aliases: HashMap::new(),
        controller_groups: Vec::new(),
        pwm_overrides: HashMap::new(),
    }
}

#[test]
#[serial]
fn test_config_curves_integration() {
    let config = create_test_config_with_curves();
    
    // Validate the entire config
    assert!(validate_saved_config(&config).is_ok());
    
    // Validate curves specifically
    let curves = config.curves.as_ref().unwrap();
    assert!(validate_curves(curves).is_ok());
    
    // Test curve interpolation
    let points = &curves.groups[0].curve.points;
    assert_eq!(interp_pwm_percent(points, 40.0), 35); // Linear interpolation
    assert_eq!(interp_pwm_percent(points, 60.0), 65); // Linear interpolation
}

#[test]
fn test_app_initialization_with_config() {
    // Test that App::new() handles configuration loading gracefully
    let app = App::new();
    
    // App should initialize even without config files
    assert_eq!(app.focus, Focus::Fans);
    assert_eq!(app.metric, Metric::C);
    assert!(app.readings.is_empty());
    assert!(app.fans.is_empty());
    assert!(app.pwms.is_empty());
    assert!(app.temps.is_empty());
}

#[test]
fn test_app_metric_conversion_integration() {
    let mut app = App::new();
    
    // Test all metric conversions
    let test_temps = vec![0.0, 25.0, 100.0, -10.0];
    
    for &temp in &test_temps {
        // Celsius
        app.metric = Metric::C;
        let (c_temp, c_unit) = app.convert_temp(temp);
        assert_eq!(c_temp, temp);
        assert_eq!(c_unit, "°C");
        
        // Fahrenheit
        app.metric = Metric::F;
        let (f_temp, f_unit) = app.convert_temp(temp);
        assert_eq!(f_temp, temp * 9.0 / 5.0 + 32.0);
        assert_eq!(f_unit, "°F");
        
        // Kelvin
        app.metric = Metric::K;
        let (k_temp, k_unit) = app.convert_temp(temp);
        assert_eq!(k_temp, temp + 273.15);
        assert_eq!(k_unit, "K");
    }
}

#[test]
fn test_hwmon_extract_index_integration() {
    // Test various real-world hwmon file patterns
    let test_cases = vec![
        ("fan1_input", "fan", "_input", Some(1)),
        ("fan12_input", "fan", "_input", Some(12)),
        ("pwm1", "pwm", "", Some(1)),
        ("pwm3", "pwm", "", Some(3)),
        ("temp1_label", "temp", "_label", Some(1)),
        ("temp2_crit", "temp", "_crit", Some(2)),
        ("in0_input", "in", "_input", Some(0)),
        ("curr1_input", "curr", "_input", Some(1)),
        // Invalid cases
        ("fan_input", "fan", "_input", None),
        ("fanX_input", "fan", "_input", None),
        ("temp1_input", "fan", "_input", None),
        ("", "fan", "_input", None),
    ];
    
    for (filename, prefix, suffix, expected) in test_cases {
        let result = extract_index(filename, prefix, suffix);
        assert_eq!(result, expected, 
            "Failed for filename: '{}', prefix: '{}', suffix: '{}'", 
            filename, prefix, suffix);
    }
}

#[test]
fn test_chip_readings_data_flow() {
    // Test the data flow from ChipReadings to App state
    let mut app = App::new();
    
    // Create mock chip readings
    let chip1 = ChipReadings {
        name: "chip1@hwmon0".to_string(),
        temps: vec![
            ("temp1".to_string(), 45.5),
            ("CPU Temp".to_string(), 52.0),
        ],
        fans: vec![
            ("fan1".to_string(), 1200),
            ("CPU Fan".to_string(), 1500),
        ],
        pwms: vec![
            ("pwm1".to_string(), 128),
            ("CPU PWM".to_string(), 200),
        ],
    };
    
    let chip2 = ChipReadings {
        name: "chip2@hwmon1".to_string(),
        temps: vec![("temp1".to_string(), 38.2)],
        fans: vec![("fan1".to_string(), 800)],
        pwms: vec![("pwm1".to_string(), 100)],
    };
    
    app.readings = vec![chip1, chip2];
    
    // Simulate the flattening process from App::refresh()
    app.fans.clear();
    app.pwms.clear();
    app.temps.clear();
    
    for chip in &app.readings {
        for (label, rpm) in &chip.fans {
            app.fans.push((format!("{}:{}", chip.name, label), *rpm));
        }
        for (label, val) in &chip.pwms {
            app.pwms.push((format!("{}:{}", chip.name, label), *val));
        }
        for (label, c) in &chip.temps {
            app.temps.push((format!("{}:{}", chip.name, label), *c));
        }
    }
    
    app.fans.sort_by(|a, b| a.0.cmp(&b.0));
    app.pwms.sort_by(|a, b| a.0.cmp(&b.0));
    app.temps.sort_by(|a, b| a.0.cmp(&b.0));
    
    // Verify the flattened data
    assert_eq!(app.fans.len(), 3);
    assert_eq!(app.pwms.len(), 3);
    assert_eq!(app.temps.len(), 3);
    
    // Check specific entries
    assert!(app.fans.iter().any(|(name, rpm)| name == "chip1@hwmon0:fan1" && *rpm == 1200));
    assert!(app.fans.iter().any(|(name, rpm)| name == "chip1@hwmon0:CPU Fan" && *rpm == 1500));
    assert!(app.fans.iter().any(|(name, rpm)| name == "chip2@hwmon1:fan1" && *rpm == 800));
    
    assert!(app.pwms.iter().any(|(name, val)| name == "chip1@hwmon0:pwm1" && *val == 128));
    assert!(app.pwms.iter().any(|(name, val)| name == "chip1@hwmon0:CPU PWM" && *val == 200));
    assert!(app.pwms.iter().any(|(name, val)| name == "chip2@hwmon1:pwm1" && *val == 100));
    
    assert!(app.temps.iter().any(|(name, temp)| name == "chip1@hwmon0:temp1" && *temp == 45.5));
    assert!(app.temps.iter().any(|(name, temp)| name == "chip1@hwmon0:CPU Temp" && *temp == 52.0));
    assert!(app.temps.iter().any(|(name, temp)| name == "chip2@hwmon1:temp1" && *temp == 38.2));
}

#[test]
fn test_mapping_validation_integration() {
    let mut app = App::new();
    
    // Add some test mappings
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
    
    // Test that mappings are properly stored
    assert_eq!(app.mappings.len(), 2);
    assert_eq!(app.mappings[0].fan, "chip1@hwmon0:fan1");
    assert_eq!(app.mappings[0].pwm, "chip1@hwmon0:pwm1");
    assert_eq!(app.mappings[1].fan, "chip2@hwmon1:fan1");
    assert_eq!(app.mappings[1].pwm, "chip2@hwmon1:pwm1");
    
    // Test conversion to SavedConfig format
    let saved_mappings: Vec<SavedMapping> = app.mappings
        .iter()
        .map(|m| SavedMapping { fan: m.fan.clone(), pwm: m.pwm.clone() })
        .collect();
    
    assert_eq!(saved_mappings.len(), 2);
    assert_eq!(saved_mappings[0].fan, "chip1@hwmon0:fan1");
    assert_eq!(saved_mappings[0].pwm, "chip1@hwmon0:pwm1");
}

#[test]
fn test_curve_temperature_response_integration() {
    // Test a complete temperature response curve scenario
    let points = vec![
        CurvePoint { temp_c: 20.0, pwm_pct: 10 },
        CurvePoint { temp_c: 40.0, pwm_pct: 30 },
        CurvePoint { temp_c: 60.0, pwm_pct: 60 },
        CurvePoint { temp_c: 80.0, pwm_pct: 90 },
    ];
    
    // Test various temperature scenarios
    let test_cases = vec![
        (15.0, 10),  // Below minimum
        (20.0, 10),  // At minimum
        (30.0, 20),  // Linear interpolation
        (50.0, 45),  // Linear interpolation
        (70.0, 75),  // Linear interpolation
        (80.0, 90),  // At maximum
        (90.0, 90),  // Above maximum
    ];
    
    for (temp, expected_pwm) in test_cases {
        let result = interp_pwm_percent(&points, temp);
        assert_eq!(result, expected_pwm, 
            "Temperature {}°C should give {}% PWM, got {}%", 
            temp, expected_pwm, result);
    }
}

#[test]
fn test_error_handling_integration() {
    // Test that different error types are properly handled
    
    // HwmonError variants
    let io_error = HwmonError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "test"));
    assert!(format!("{}", io_error).contains("IO error"));
    
    let parse_error = HwmonError::Parse("Invalid number".to_string());
    assert_eq!(format!("{}", parse_error), "Parse error: Invalid number");
    
    let invalid_data_error = HwmonError::InvalidData("Chip not found".to_string());
    assert_eq!(format!("{}", invalid_data_error), "Invalid data: Chip not found");
    
    let permission_error = HwmonError::PermissionDenied;
    assert_eq!(format!("{}", permission_error), "Permission denied - need root");
}

#[test]
fn test_app_state_consistency() {
    let mut app = App::new();
    
    // Test that cycling through focus states works correctly
    let focus_states = vec![Focus::Fans, Focus::Pwms, Focus::Temps, Focus::Control];
    
    for expected_focus in focus_states {
        app.focus = expected_focus;
        assert_eq!(app.focus, expected_focus);
    }
    
    // Test metric cycling
    assert_eq!(app.metric, Metric::C);
    app.cycle_metric();
    assert_eq!(app.metric, Metric::F);
    app.cycle_metric();
    assert_eq!(app.metric, Metric::K);
    app.cycle_metric();
    assert_eq!(app.metric, Metric::C);
}

#[test]
fn test_config_serialization_roundtrip() {
    let original_config = create_test_config_with_curves();
    
    // Serialize to JSON
    let json = serde_json::to_string_pretty(&original_config).unwrap();
    
    // Deserialize back
    let deserialized_config: SavedConfig = serde_json::from_str(&json).unwrap();
    
    // Verify key fields match
    assert_eq!(deserialized_config.mappings.len(), original_config.mappings.len());
    assert_eq!(deserialized_config.metric, original_config.metric);
    assert_eq!(deserialized_config.fan_aliases.len(), original_config.fan_aliases.len());
    
    // Verify curves are preserved
    assert!(deserialized_config.curves.is_some());
    let curves = deserialized_config.curves.as_ref().unwrap();
    assert_eq!(curves.version, 1);
    assert_eq!(curves.groups.len(), 1);
    assert_eq!(curves.groups[0].name, "cpu_fans");
    assert_eq!(curves.groups[0].curve.points.len(), 3);
    
    // Validate the deserialized config
    assert!(validate_saved_config(&deserialized_config).is_ok());
}
