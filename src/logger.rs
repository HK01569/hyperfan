/*
 * This file is part of Hyperfan.
 *
 * Copyright (C) 2025 Hyperfan contributors
 *
 * Hyperfan is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * Hyperfan is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with Hyperfan. If not, see <https://www.gnu.org/licenses/>.
 */

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use lazy_static::lazy_static;
use serde_json::{json, Value};

const DEFAULT_LOG_PATH: &str = "/etc/hyperfan/logs.json";

lazy_static! {
    static ref LOG_FILE: Mutex<Option<File>> = Mutex::new(None);
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

pub fn init_logging() {
    // Ensure directory exists
    if let Some(parent) = Path::new(DEFAULT_LOG_PATH).parent() {
        let _ = fs::create_dir_all(parent);
    }
    // Open file append
    match OpenOptions::new().create(true).append(true).open(DEFAULT_LOG_PATH) {
        Ok(f) => {
            if let Ok(mut guard) = LOG_FILE.lock() {
                *guard = Some(f);
            }
        }
        Err(_e) => {
            // Last resort: fall back to /tmp if /etc is unavailable (silent)
            let fallback = "/tmp/hyperfan_logs.json";
            if let Some(parent) = Path::new(fallback).parent() { let _ = fs::create_dir_all(parent); }
            if let Ok(f) = OpenOptions::new().create(true).append(true).open(fallback) {
                if let Ok(mut guard) = LOG_FILE.lock() { *guard = Some(f); }
            }
        }
    }
}

pub fn log_event(event: &str, data: Value) {
    let line = json!({
        "ts_ms": now_millis(),
        "event": event,
        "data": data,
    })
    .to_string();

    if let Ok(mut guard) = LOG_FILE.lock() {
        if let Some(f) = guard.as_mut() {
            let _ = writeln!(f, "{}", line);
            return;
        }
    }
    // If logger not initialized, write to /tmp silently
    let fallback = "/tmp/hyperfan_logs.json";
    if let Some(parent) = Path::new(fallback).parent() { let _ = fs::create_dir_all(parent); }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(fallback) {
        let _ = writeln!(f, "{}", line);
    }
}

