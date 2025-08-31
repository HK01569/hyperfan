#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use hyperfan_core::*;
use std::path::PathBuf;
use tauri::Manager;

#[tauri::command]
fn get_summary() -> Result<SystemSummary, String> {
    get_system_summary().map_err(|e| e.to_string())
}

#[tauri::command]
fn get_hwmon_chips() -> Result<Vec<HwmonChip>, String> {
    enumerate_hwmon_chips().map_err(|e| e.to_string())
}

#[tauri::command]
fn set_pwm_percentage(pwm_path: String, percent: f32) -> Result<(), String> {
    let path = PathBuf::from(pwm_path);
    set_pwm_percent(&path, percent).map_err(|e| e.to_string())
}

#[tauri::command]
fn load_profile() -> Result<Option<ProfileConfig>, String> {
    load_profile_config().map_err(|e| e.to_string())
}

#[tauri::command]
fn save_profile(config: ProfileConfig) -> Result<(), String> {
    save_profile_config(&config).map_err(|e| e.to_string())
}

#[tauri::command]
fn check_profile_exists() -> bool {
    validate_profile_exists()
}

#[tauri::command]
async fn autodetect_mappings() -> Result<Vec<FanMapping>, String> {
    // Run the blocking operation in a separate thread to avoid freezing the UI
    tokio::task::spawn_blocking(|| {
        autodetect_fan_pwm_mappings()
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_sensor_names_cmd() -> Result<std::collections::HashMap<String, String>, String> {
    get_sensor_names().map_err(|e| e.to_string())
}

#[tauri::command]
fn set_sensor_name_cmd(key_input_path: String, name: String) -> Result<(), String> {
    set_sensor_name(key_input_path, name).map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_summary,
            get_hwmon_chips,
            set_pwm_percentage,
            load_profile,
            save_profile,
            check_profile_exists,
            autodetect_mappings,
            get_sensor_names_cmd,
            set_sensor_name_cmd
        ])
        .setup(|app| {
            #[cfg(debug_assertions)]
            if let Some(window) = app.get_window("main") {
                window.open_devtools();
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
