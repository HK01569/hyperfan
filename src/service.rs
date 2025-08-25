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

use std::thread;
use std::time::{Duration, Instant};
use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};

use crate::config::{load_saved_config, try_load_system_config};
use crate::hwmon;
use crate::curves::{self, CurvesConfig};

#[derive(Clone, Debug)]
struct CurvePoint {
    temp_c: f64,
    pwm_pct: u8,
}

fn default_curve() -> Vec<CurvePoint> {
    // Keep in sync with `App::new()` defaults
    vec![
        CurvePoint { temp_c: 30.0, pwm_pct: 20 },
        CurvePoint { temp_c: 40.0, pwm_pct: 30 },
        CurvePoint { temp_c: 50.0, pwm_pct: 50 },
        CurvePoint { temp_c: 60.0, pwm_pct: 70 },
        CurvePoint { temp_c: 70.0, pwm_pct: 100 },
    ]
}

fn interp_pwm_percent(curve: &[CurvePoint], temp_c: f64) -> u8 {
    if curve.is_empty() {
        return 0;
    }
    // Below first point
    if temp_c <= curve[0].temp_c {
        return curve[0].pwm_pct;
    }
    // Above last point
    if temp_c >= curve[curve.len() - 1].temp_c {
        return curve[curve.len() - 1].pwm_pct;
    }
    // Find surrounding points
    for w in curve.windows(2) {
        let a = &w[0];
        let b = &w[1];
        if temp_c >= a.temp_c && temp_c <= b.temp_c {
            let t = (temp_c - a.temp_c) / (b.temp_c - a.temp_c);
            let v = (a.pwm_pct as f64) + t * ((b.pwm_pct as f64) - (a.pwm_pct as f64));
            return v.round().clamp(0.0, 100.0) as u8;
        }
    }
    // Fallback
    curve[curve.len() - 1].pwm_pct
}

fn parse_chip_and_label(s: &str) -> Option<(String, String)> {
    s.split_once(':').map(|(a, b)| (a.to_string(), b.to_string()))
}

pub fn run_service() -> Result<()> {
    eprintln!("hyperfan: starting service mode");

    // Try to load system config (profile.json). If it contains curves, use them.
    let system_cfg = try_load_system_config().ok();
    let curves_from_profile: Option<CurvesConfig> = system_cfg.as_ref().and_then(|c| c.curves.clone());

    // Prefer curves.json as the source of truth; fall back to curves from profile.json
    let curves_cfg: Option<CurvesConfig> = match curves::load_curves() {
        Some(c) => Some(c),
        None => curves_from_profile,
    };

    // Fallback legacy mapping-based targets and default curve (used when no curves available)
    // Load mappings from system config first, fallback to user config
    let legacy_cfg = if curves_cfg.is_none() {
        match system_cfg {
            Some(c) => Some(c),
            None => load_saved_config(),
        }
    } else { None };

    let mut legacy_targets: Vec<(String, usize)> = Vec::new(); // (pwm_chip, pwm_idx)
    if let Some(cfg) = &legacy_cfg {
        if cfg.mappings.is_empty() {
            return Err(anyhow!("no mappings defined"));
        }
        for m in &cfg.mappings {
            let (pwm_chip, pwm_label) = match parse_chip_and_label(&m.pwm) { Some(x) => x, None => continue };
            if let Some(idx) = hwmon::find_pwm_index_by_label(&pwm_chip, &pwm_label) {
                legacy_targets.push((pwm_chip, idx));
            }
        }
        if legacy_targets.is_empty() { return Err(anyhow!("no usable PWM targets resolved from config")); }
    }

    let legacy_curve = default_curve();

    // If not in curves mode, apply any persistent pwm_overrides once at startup
    if curves_cfg.is_none() {
        if let Some(cfg) = &legacy_cfg {
            if !cfg.pwm_overrides.is_empty() {
                eprintln!("hyperfan: applying {} pwm override(s)", cfg.pwm_overrides.len());
                for (key, pct) in &cfg.pwm_overrides {
                    if let Some((chip, label)) = parse_chip_and_label(key) {
                        if let Some(idx) = hwmon::find_pwm_index_by_label(&chip, &label) {
                            let value = ((*pct as u16) * 255 / 100) as u8;
                            let _ = hwmon::write_pwm(&chip, idx, value);
                        }
                    }
                }
            }
        }
    }

    let interval = Duration::from_millis(1000);
    let mut last = Instant::now() - interval;

    // Cache last written PWM percent per (chip, idx) to apply hysteresis/min delta
    let mut last_written_pct: HashMap<(String, usize), u8> = HashMap::new();

    loop {
        let now = Instant::now();
        if now.duration_since(last) < interval {
            thread::sleep(Duration::from_millis(50));
            continue;
        }
        last = now;

        // Read all sensors once per tick
        let chips = hwmon::read_all().context("read_all")?;

        if let Some(cfg) = &curves_cfg {
            // Curves/groups mode
            for g in &cfg.groups {
                // Find temp
                let (temp_chip, temp_label) = match parse_chip_and_label(&g.temp_source) { Some(x) => x, None => continue };
                let mut temp_opt: Option<f64> = None;
                for ch in &chips {
                    if ch.name == temp_chip {
                        for (lbl, c) in &ch.temps {
                            if lbl == &temp_label { temp_opt = Some(*c); break; }
                        }
                        break;
                    }
                }
                let Some(temp_c) = temp_opt else { continue };

                let pct_raw = curves::interp_pwm_percent(&g.curve.points, temp_c);
                let mut pct = pct_raw.clamp(g.curve.min_pwm_pct, g.curve.max_pwm_pct);
                if pct < g.curve.floor_pwm_pct { pct = g.curve.floor_pwm_pct; }

                for member in &g.members {
                    let (pwm_chip, pwm_label) = match parse_chip_and_label(member) { Some(x) => x, None => continue };
                    if let Some(pwm_idx) = hwmon::find_pwm_index_by_label(&pwm_chip, &pwm_label) {
                        let key = (pwm_chip.clone(), pwm_idx);
                        let do_write = match last_written_pct.get(&key) {
                            Some(&last_pct) => {
                                let diff_pct = pct.abs_diff(last_pct);
                                diff_pct >= g.curve.hysteresis_pct
                            }
                            None => true,
                        };
                        if do_write {
                            let value = ((pct as u16) * 255 / 100) as u8;
                            if let Some(&last_pct) = last_written_pct.get(&key) {
                                let last_val = ((last_pct as u16) * 255 / 100) as u8;
                                if value.abs_diff(last_val) < g.curve.write_min_delta { continue; }
                            }
                            let _ = hwmon::write_pwm(&key.0, key.1, value);
                            last_written_pct.insert(key, pct);
                        }
                    }
                }
            }
        } else {
            // Legacy mappings mode
            for (pwm_chip, pwm_idx) in &legacy_targets {
                // Choose a temperature source dynamically:
                // 1) Prefer a temperature from the same chip as the PWM
                // 2) Fallback to the first available temperature overall
                let mut temp_opt: Option<f64> = None;
                // Prefer same chip
                for ch in &chips {
                    if ch.name == *pwm_chip {
                        if let Some((_, c)) = ch.temps.first() { temp_opt = Some(*c); }
                        break;
                    }
                }
                // Fallback
                if temp_opt.is_none() {
                    'outer: for ch in &chips {
                        for (_, c) in &ch.temps { temp_opt = Some(*c); break 'outer; }
                    }
                }
                if let Some(temp_c) = temp_opt {
                    let pct = interp_pwm_percent(&legacy_curve, temp_c);
                    let value = ((pct as u16) * 255 / 100) as u8;
                    let _ = hwmon::write_pwm(pwm_chip, *pwm_idx, value);
                }
            }
        }
    }
}
