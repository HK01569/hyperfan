use crate::{
    app::{App, Mapping, Focus},
    config::{save_mappings, write_system_config, SavedConfig, ControllerGroup},
    curves::{self, CurveGroup, CurvePoint, CurveSpec, CurvesConfig},
    hwmon,
};
use std::collections::HashMap;
use std::{sync::{Arc, Mutex}, thread, io::Write};

fn parse_chip_and_label(s: &str) -> Option<(String, String)> {
    s.split_once(':').map(|(a, b)| (a.to_string(), b.to_string()))
}

// ===== Groups Manager Handlers =====
pub fn toggle_groups_manager(app: &mut App) {
    app.show_groups_manager = !app.show_groups_manager;
    if app.show_groups_manager {
        // Load latest from system profile if available
        if let Ok(cfg) = crate::config::try_load_system_config() {
            app.groups = cfg.controller_groups;
        }
        app.group_idx = 0;
        app.groups_pwm_idx = 0;
        app.groups_focus_right = false;
        app.status = "Groups: ←/→ focus | ↑/↓ navigate | Space toggle member | n new | r rename | X delete | s save | Esc exit".to_string();
    } else {
        app.status = "Exited groups".to_string();
    }
}

/// Apply a sane default reverse-arc curve to all editor groups.
/// The curve ramps concavely to 100% by 80°C and stays at 100% afterwards.
pub fn editor_apply_default_curves(app: &mut App) {
    if app.editor_groups.is_empty() { return; }
    for g in &mut app.editor_groups {
        // Generate points up to 80°C with a quadratic curve, then 100% to 100°C.
        let mut pts: Vec<CurvePoint> = Vec::new();
        let anchors = [0u32, 30, 40, 50, 60, 70, 80, 100];
        for &t in &anchors {
            let pwm: u8 = if t <= 80 {
                let x = (t as f64) / 80.0; // 0..1
                let y = x * x; // concave-up (reverse arc feel)
                (y * 100.0).round().clamp(0.0, 100.0) as u8
            } else { 100 };
            pts.push(CurvePoint { temp_c: t as f64, pwm_pct: pwm });
        }
        // Deduplicate in case of identical values and ensure sorted
        pts.dedup_by(|a, b| (a.temp_c as u32) == (b.temp_c as u32) && a.pwm_pct == b.pwm_pct);
        g.curve.points = pts;
    }
    app.editor_dirty = true;
    app.status = "Applied default curves to all control pairs (100% at 80°C)".to_string();
}

// ===== Groups: Map PWM -> FAN popup handlers =====
pub fn groups_start_map_pwm(app: &mut App) {
    if app.pwms.is_empty() || app.fans.is_empty() { return; }
    app.map_fan_idx = 0;
    app.show_map_pwm_popup = true;
}

pub fn groups_cancel_map_pwm(app: &mut App) {
    app.show_map_pwm_popup = false;
}

pub fn groups_apply_map_pwm(app: &mut App) {
    if !app.show_map_pwm_popup { return; }
    if app.pwms.is_empty() || app.fans.is_empty() { app.show_map_pwm_popup = false; return; }
    // Target PWM is the one currently selected in the right-hand list
    let Some((pwm_full, _)) = app.pwms.get(app.groups_pwm_idx).cloned() else { app.show_map_pwm_popup = false; return; };
    let Some((fan_full, _)) = app.fans.get(app.map_fan_idx).cloned() else { app.show_map_pwm_popup = false; return; };

    // If an entry for this PWM exists, update its fan; else if an entry for this fan exists, update its pwm; else push new mapping
    if let Some(m) = app.mappings.iter_mut().find(|m| m.pwm == pwm_full) {
        m.fan = fan_full.clone();
    } else if let Some(m) = app.mappings.iter_mut().find(|m| m.fan == fan_full) {
        m.pwm = pwm_full.clone();
    } else {
        app.mappings.push(Mapping { fan: fan_full.clone(), pwm: pwm_full.clone() });
    }
    // Move control selection to the updated/added mapping
    if let Some(idx) = app.mappings.iter().position(|m| m.pwm == pwm_full) { app.control_idx = idx; }
    let _ = save_mappings(&app.mappings);
    app.show_map_pwm_popup = false;
}

pub fn groups_prev(app: &mut App) { if app.group_idx > 0 { app.group_idx -= 1; } }
pub fn groups_next(app: &mut App) { if app.group_idx + 1 < app.groups.len() { app.group_idx += 1; } }

pub fn groups_delete_current(app: &mut App) {
    if app.groups.is_empty() { return; }
    let idx = app.group_idx.min(app.groups.len() - 1);
    app.groups.remove(idx);
    if app.group_idx >= app.groups.len() { app.group_idx = app.groups.len().saturating_sub(1); }
}

pub fn groups_toggle_member_selected(app: &mut App) {
    if app.groups.is_empty() || app.pwms.is_empty() { return; }
    let pwm = match app.pwms.get(app.groups_pwm_idx) { Some((s, _)) => s.clone(), None => return };
    let g = &mut app.groups[app.group_idx];
    if g.members.iter().any(|m| m == &pwm) {
        g.members.retain(|m| m != &pwm);
    } else {
        g.members.push(pwm);
    }
}

pub fn groups_add_group(app: &mut App) {
    // Open popup to enter a name for the new group
    app.group_name_input = format!("group-{}", app.groups.len() + 1);
    if app.group_name_input.len() > 20 { app.group_name_input.truncate(20); }
    app.group_rename_mode = false;
    app.show_group_name_popup = true;
}

pub fn groups_start_rename_current(app: &mut App) {
    if app.groups.is_empty() { return; }
    let g = &app.groups[app.group_idx];
    app.group_name_input = g.name.clone();
    if app.group_name_input.len() > 20 { app.group_name_input.truncate(20); }
    app.group_rename_mode = true;
    app.show_group_name_popup = true;
}

pub fn groups_apply_new_name(app: &mut App) {
    if !app.show_group_name_popup { return; }
    let name = app.group_name_input.trim();
    if !is_safe_group_label(name) {
        app.warning_message = "Invalid group name. Max 20 chars. Allowed: letters, numbers, and '-' (no consecutive dashes)".to_string();
        app.show_warning_popup = true;
        return;
    }
    if app.group_rename_mode {
        if app.groups.is_empty() { return; }
        let idx = app.group_idx.min(app.groups.len() - 1);
        app.groups[idx].name = name.to_string();
    } else {
        app.groups.push(ControllerGroup { name: name.to_string(), members: Vec::new() });
        app.group_idx = app.groups.len().saturating_sub(1);
    }
    app.show_group_name_popup = false;
    app.group_name_input.clear();
    app.group_rename_mode = false;
}

pub fn groups_cancel_new_name(app: &mut App) {
    app.show_group_name_popup = false;
    app.group_name_input.clear();
    app.group_rename_mode = false;
}

pub fn groups_add_member_current(app: &mut App) {
    if app.groups.is_empty() { return; }
    let Some((pwm, _)) = app.pwms.get(app.pwms_idx).cloned() else { return; };
    let g = &mut app.groups[app.group_idx];
    if !g.members.contains(&pwm) { g.members.push(pwm); }
}

pub fn groups_remove_member_current(app: &mut App) {
    if app.groups.is_empty() { return; }
    let Some((pwm, _)) = app.pwms.get(app.pwms_idx).cloned() else { return; };
    let g = &mut app.groups[app.group_idx];
    g.members.retain(|m| m != &pwm);
}

pub fn groups_save(app: &mut App) {
    // Preserve existing curves if present in the current system profile
    let existing_cfg = crate::config::try_load_system_config().ok();
    let existing_curves = existing_cfg.as_ref().and_then(|c| c.curves.clone());
    let existing_overrides = existing_cfg.map(|c| c.pwm_overrides).unwrap_or_default();
    let saved = SavedConfig {
        mappings: app
            .mappings
            .iter()
            .map(|m| crate::config::SavedMapping { fan: m.fan.clone(), pwm: m.pwm.clone() })
            .collect(),
        metric: app.metric,
        curves: existing_curves,
        fan_aliases: app.fan_aliases.clone(),
        pwm_aliases: app.pwm_aliases.clone(),
        temp_aliases: app.temp_aliases.clone(),
        controller_groups: app.groups.clone(),
        pwm_overrides: existing_overrides,
    };
    match write_system_config(&saved) {
        Ok(()) => {
            app.status = "Saved groups to /etc/hyperfan/profile.json".to_string();
            app.show_groups_manager = false; // return to main screen
        }
        Err(e) => app.status = format!("Failed to save groups: {}", e),
    }
}
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

use std::fs;


fn is_safe_user_label(s: &str) -> bool {
    // Allow a friendly set of characters similar to config::is_safe_label
    if s.is_empty() || s.len() > 128 { return false; }
    s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '_' | '-' | '.' | ' ' | '@'))
}

fn is_safe_group_label(s: &str) -> bool {
    if s.is_empty() || s.len() > 20 { return false; }
    // letters, numbers, and single dashes (no consecutive '-')
    let mut prev_dash = false;
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            prev_dash = false;
            continue;
        }
        if ch == '-' {
            if prev_dash { return false; }
            prev_dash = true;
            continue;
        }
        return false;
    }
    true
}

// ===== Rename Handlers =====
pub fn start_rename(app: &mut App) {
    if app.focus == Focus::Control { return; }
    // Determine target based on focus
    let target = match app.focus {
        Focus::Fans => app.fans.get(app.fans_idx).map(|(n, _)| (n.clone(), Focus::Fans)),
        Focus::Pwms => app.pwms.get(app.pwms_idx).map(|(n, _)| (n.clone(), Focus::Pwms)),
        Focus::Temps => app.temps.get(app.temps_idx).map(|(n, _)| (n.clone(), Focus::Temps)),
        Focus::Control => None,
    };
    let Some((name, kind)) = target else { return; };

    app.rename_target_kind = Some(kind);
    app.rename_target_name = name.clone();
    // Pre-fill input with existing alias or current name for quick editing
    app.rename_input = match kind {
        Focus::Fans => app.fan_aliases.get(&name).cloned().unwrap_or(name),
        Focus::Pwms => app.pwm_aliases.get(&name).cloned().unwrap_or(name),
        Focus::Temps => app.temp_aliases.get(&name).cloned().unwrap_or(name),
        _ => name,
    };
    app.show_rename_popup = true;
}

pub fn apply_rename(app: &mut App) {
    if !app.show_rename_popup { return; }
    let Some(kind) = app.rename_target_kind else { return; };
    let key = app.rename_target_name.clone();
    let new_label = app.rename_input.trim();
    if !is_safe_user_label(new_label) {
        app.warning_message = "Invalid label. Allowed: letters, numbers, space, : _ - . @ (max 128 chars)".to_string();
        app.show_warning_popup = true;
        return;
    }
    match kind {
        Focus::Fans => { app.fan_aliases.insert(key, new_label.to_string()); }
        Focus::Pwms => { app.pwm_aliases.insert(key, new_label.to_string()); }
        Focus::Temps => { app.temp_aliases.insert(key, new_label.to_string()); }
        _ => {}
    }
    app.show_rename_popup = false;
    app.rename_input.clear();
    // Persist to system profile immediately, preserving existing curves and overrides
    let existing_cfg = crate::config::try_load_system_config().ok();
    let existing_curves = existing_cfg.as_ref().and_then(|c| c.curves.clone());
    let existing_overrides = existing_cfg.map(|c| c.pwm_overrides).unwrap_or_default();
    let saved = SavedConfig {
        mappings: app
            .mappings
            .iter()
            .map(|m| crate::config::SavedMapping { fan: m.fan.clone(), pwm: m.pwm.clone() })
            .collect(),
        metric: app.metric,
        curves: existing_curves,
        fan_aliases: app.fan_aliases.clone(),
        pwm_aliases: app.pwm_aliases.clone(),
        temp_aliases: app.temp_aliases.clone(),
        controller_groups: app.groups.clone(),
        pwm_overrides: existing_overrides,
    };
    if let Err(e) = write_system_config(&saved) {
        app.status = format!("Failed to save aliases: {}", e);
    } else {
        app.status = "Saved aliases to /etc/hyperfan/profile.json".to_string();
    }
}

pub fn revert_rename_default(app: &mut App) {
    if !app.show_rename_popup { return; }
    let Some(kind) = app.rename_target_kind.clone() else { return; };
    let key = app.rename_target_name.clone();
    match kind {
        Focus::Fans => { app.fan_aliases.remove(&key); }
        Focus::Pwms => { app.pwm_aliases.remove(&key); }
        Focus::Temps => { app.temp_aliases.remove(&key); }
        _ => {}
    }
    app.show_rename_popup = false;
    app.rename_input.clear();
}

pub fn cancel_rename(app: &mut App) {
    app.show_rename_popup = false;
    app.rename_input.clear();
}

// ===== Curve Editor Handlers =====
pub fn toggle_curve_editor(app: &mut App) {
    app.show_curve_editor = !app.show_curve_editor;
    if app.show_curve_editor {
        app.editor_focus_right = false;
        // Get the set of PWMs that are actually mapped in CONTROL section
        let mapped_pwms: std::collections::HashSet<String> = app.mappings
            .iter()
            .map(|m| m.pwm.clone())
            .collect();

        if let Ok(saved) = crate::config::try_load_system_config() {
            if let Some(curves_cfg) = saved.curves {
                // Filter groups to only include those with members that are actually mapped
                app.editor_groups = curves_cfg.groups
                    .into_iter()
                    .filter(|group| {
                        // Keep group if any of its members are in the mapped PWMs
                        group.members.iter().any(|member| mapped_pwms.contains(member))
                    })
                    .collect();
                app.editor_group_idx = 0;
                app.editor_point_idx = 0;
            } else if let Some(cfg) = curves::load_curves() {
                // Filter groups to only include those with members that are actually mapped
                app.editor_groups = cfg.groups
                    .into_iter()
                    .filter(|group| {
                        group.members.iter().any(|member| mapped_pwms.contains(member))
                    })
                    .collect();
                app.editor_group_idx = 0;
                app.editor_point_idx = 0;
            } else {
                app.editor_groups.clear();
                app.editor_group_idx = 0;
                app.editor_point_idx = 0;
            }
        } else if let Some(cfg) = curves::load_curves() {
            // Filter groups to only include those with members that are actually mapped
            app.editor_groups = cfg.groups
                .into_iter()
                .filter(|group| {
                    group.members.iter().any(|member| mapped_pwms.contains(member))
                })
                .collect();
            app.editor_group_idx = 0;
            app.editor_point_idx = 0;
        } else {
            app.editor_groups.clear();
            app.editor_group_idx = 0;
            app.editor_point_idx = 0;
        }
        // Only create a default group if there are no existing groups and no mappings
        if app.mappings.is_empty() && app.editor_groups.is_empty() {
            editor_add_group(app);
        }
        // Ensure selection indices are in range
        if app.editor_group_idx >= app.editor_groups.len() {
            app.editor_group_idx = app.editor_groups.len().saturating_sub(1);
        }
        if app.editor_groups.is_empty() {
            app.editor_group_idx = 0;
            app.editor_point_idx = 0;
        }
        app.status = "Curve editor: n=new (auto), t=set temp, [/] select point, +/- adjust, g=graph, h=delay (ms), s=save, Esc=exit".to_string();
    } else {
        app.status = "Exited curve editor".to_string();
    }
}

pub fn editor_add_group(app: &mut App) {
    // Determine members (magic): prefer current controller group; else current mapping/control; else selected PWM
    let mut members: Vec<String> = Vec::new();
    let mut name: String = String::new();
    // Use controller group if available and non-empty
    if !app.groups.is_empty() {
        let gi = app.group_idx.min(app.groups.len() - 1);
        let g = &app.groups[gi];
        if !g.members.is_empty() {
            members = g.members.clone();
            name = g.name.clone();
        }
    }
    // Else use current control mapping (if exists)
    if members.is_empty() && !app.mappings.is_empty() {
        let mi = app.control_idx.min(app.mappings.len() - 1);
        let m = &app.mappings[mi];
        members.push(m.pwm.clone());
        // Derive name from fan label or pwm label
        name = m
            .fan
            .split(':')
            .last()
            .unwrap_or("fan")
            .to_string();
    }
    // Else fallback to the currently selected PWM
    if members.is_empty() {
        if let Some((pwm_full, _)) = app.pwms.get(app.pwms_idx) {
            members.push(pwm_full.clone());
            name = pwm_full
                .split(':')
                .last()
                .unwrap_or("PWM")
                .to_string();
        }
    }
    if name.is_empty() {
        name = format!("Group {}", app.editor_groups.len() + 1);
    }

    // Temperature source: use currently selected temp or first
    let temp_source = match app.temps.get(app.temps_idx) {
        Some((s, _)) => s.clone(),
        None => app.temps.first().map(|(s, _)| s.clone()).unwrap_or_default(),
    };

    let group = CurveGroup {
        name,
        members,
        temp_source,
        curve: CurveSpec {
            points: vec![
                CurvePoint { temp_c: 30.0, pwm_pct: 20 },
                CurvePoint { temp_c: 40.0, pwm_pct: 30 },
                CurvePoint { temp_c: 50.0, pwm_pct: 50 },
                CurvePoint { temp_c: 60.0, pwm_pct: 70 },
                CurvePoint { temp_c: 70.0, pwm_pct: 100 },
            ],
            min_pwm_pct: 0,
            max_pwm_pct: 100,
            floor_pwm_pct: 0,
            hysteresis_pct: 5,
            write_min_delta: 5,
            apply_delay_ms: 0,
        },
    };
    app.editor_groups.push(group);
    app.editor_group_idx = app.editor_groups.len().saturating_sub(1);
    app.editor_point_idx = 0;
}

// ===== Curve Editor: Hysteresis Apply-Delay Popup =====
pub fn editor_start_delay_popup(app: &mut App) {
    if app.editor_groups.is_empty() { return; }
    let g = &app.editor_groups[app.editor_group_idx];
    app.curve_delay_input = g.curve.apply_delay_ms.to_string();
    app.show_curve_delay_popup = true;
}

pub fn editor_apply_delay_popup(app: &mut App) {
    if !app.show_curve_delay_popup || app.editor_groups.is_empty() { return; }
    let s = app.curve_delay_input.trim();
    match s.parse::<u32>() {
        Ok(mut v) => {
            if v > 600_000 { v = 600_000; }
            let g = &mut app.editor_groups[app.editor_group_idx];
            g.curve.apply_delay_ms = v;
            app.status = format!("Curve delay set to {} ms", v);
            app.show_curve_delay_popup = false;
            app.editor_dirty = true;
        }
        Err(_) => {
            app.warning_message = "Invalid delay. Enter milliseconds (0..600000)".to_string();
            app.show_warning_popup = true;
        }
    }
}

pub fn editor_cancel_delay_popup(app: &mut App) {
    app.show_curve_delay_popup = false;
    // Return user to graph view
    if app.editor_groups.is_empty() { return; }
    if !app.editor_graph_mode {
        // Initialize graph bins from current group's curve
        let g = &app.editor_groups[app.editor_group_idx];
        let mut bins = [0u8; 101];
        for t in 0..=100usize { bins[t] = curves::interp_pwm_percent(&g.curve.points, t as f64); }
        app.editor_graph = bins;
        app.editor_graph_sel = 40.min(100);
        app.editor_graph_input.clear();
        app.editor_graph_typed = false;
    }
    app.editor_graph_mode = true;
}

// ===== Curve Editor: Hysteresis Percent Popup =====
pub fn editor_start_hyst_popup(app: &mut App) {
    if app.editor_groups.is_empty() { return; }
    app.show_curve_hyst_popup = true;
    let g = &app.editor_groups[app.editor_group_idx];
    app.curve_hyst_input = g.curve.hysteresis_pct.to_string();
}

pub fn editor_apply_hyst_popup(app: &mut App) {
    if app.editor_groups.is_empty() { return; }
    if let Ok(v) = app.curve_hyst_input.parse::<u8>() {
        let clamped = v.min(50);
        let g = &mut app.editor_groups[app.editor_group_idx];
        g.curve.hysteresis_pct = clamped;
        app.editor_dirty = true;
    }
}

pub fn editor_cancel_hyst_popup(app: &mut App) {
    app.show_curve_hyst_popup = false;
    app.curve_hyst_input.clear();
}

// ===== Curve Editor: Temperature Source Selection Popup =====

pub fn editor_start_temp_source_popup(app: &mut App) {
    if app.editor_groups.is_empty() { return; }
    app.show_temp_source_popup = true;
    app.temp_source_selection = 0;
    
    // Find current temp source index if it exists
    let current_temp = &app.editor_groups[app.editor_group_idx].temp_source;
    if let Some(idx) = app.temps.iter().position(|(name, _)| name == current_temp) {
        app.temp_source_selection = idx;
    }
}

pub fn editor_apply_temp_source_popup(app: &mut App) {
    if app.editor_groups.is_empty() || app.temps.is_empty() { return; }
    
    let selected_idx = app.temp_source_selection.min(app.temps.len().saturating_sub(1));
    if let Some((temp_name, _)) = app.temps.get(selected_idx) {
        let g = &mut app.editor_groups[app.editor_group_idx];
        g.temp_source = temp_name.clone();
        app.editor_dirty = true;
    }
    
    app.show_temp_source_popup = false;
    app.temp_source_selection = 0;
}

pub fn editor_cancel_temp_source_popup(app: &mut App) {
    app.show_temp_source_popup = false;
    app.temp_source_selection = 0;
}

pub fn editor_temp_source_move_selection(app: &mut App, delta: i32) {
    if !app.show_temp_source_popup || app.temps.is_empty() { return; }
    
    let current = app.temp_source_selection as i32;
    let mut new_idx = current + delta;
    
    if new_idx < 0 { new_idx = 0; }
    if new_idx >= app.temps.len() as i32 { new_idx = app.temps.len() as i32 - 1; }
    
    app.temp_source_selection = new_idx as usize;
}

// ===== Curve Editor: Graph Mode (0..100°C bins) =====
pub fn editor_graph_toggle_mode(app: &mut App) {
    if app.mappings.is_empty() {
        app.warning_message = "No CONTROL pairings yet. Map a FAN -> PWM on the main page first (press 'm').".to_string();
        app.show_warning_popup = true;
        return;
    }
    if app.editor_groups.is_empty() { return; }
    if !app.editor_graph_mode {
        let g = &app.editor_groups[app.editor_group_idx];
        let mut bins = [0u8; 101];
        for t in 0..=100usize {
            bins[t] = curves::interp_pwm_percent(&g.curve.points, t as f64);
        }
        app.editor_graph = bins;
        app.editor_graph_sel = 40.min(100);
        app.editor_graph_input.clear();
        app.editor_graph_typed = false;
    }
    app.editor_graph_mode = !app.editor_graph_mode;
    if !app.editor_graph_mode {
        app.editor_graph_input.clear();
        app.editor_graph_typed = false;
    }
}

pub fn editor_graph_move_sel(app: &mut App, delta: i32) {
    if !app.editor_graph_mode { return; }
    let cur = app.editor_graph_sel as i32;
    let mut v = cur + delta;
    if v < 0 { v = 0; }
    if v > 100 { v = 100; }
    app.editor_graph_sel = v as usize;
    // Reset any in-progress numeric typing when moving selection
    app.editor_graph_input.clear();
    app.editor_graph_typed = false;
}

// Enhanced navigation: jump to specific temperature points
pub fn editor_graph_jump_to_temp(app: &mut App, target_temp: usize) {
    if !app.editor_graph_mode { return; }
    app.editor_graph_sel = target_temp.min(100);
    app.editor_graph_input.clear();
    app.editor_graph_typed = false;
}

// Smart navigation: jump to next/prev significant point
pub fn editor_graph_jump_to_next_point(app: &mut App, forward: bool) {
    if !app.editor_graph_mode { return; }
    let current = app.editor_graph_sel;
    let mut target = current;
    
    // Find next significant change in PWM value (>5% difference)
    let current_pwm = app.editor_graph[current];
    
    if forward {
        for i in (current + 1)..=100 {
            if (app.editor_graph[i] as i16 - current_pwm as i16).abs() >= 5 {
                target = i;
                break;
            }
        }
        if target == current { target = 100; } // Jump to end if no significant change
    } else {
        for i in (0..current).rev() {
            if (app.editor_graph[i] as i16 - current_pwm as i16).abs() >= 5 {
                target = i;
                break;
            }
        }
        if target == current { target = 0; } // Jump to start if no significant change
    }
    
    app.editor_graph_sel = target;
    app.editor_graph_input.clear();
    app.editor_graph_typed = false;
}

pub fn editor_graph_adjust(app: &mut App, delta: i16) {
    if !app.editor_graph_mode { return; }
    let i = app.editor_graph_sel.min(100);
    let cur = app.editor_graph[i] as i16;
    let mut v = cur + delta;
    if v < 0 { v = 0; }
    if v > 100 { v = 100; }
    app.editor_graph[i] = v as u8;
    app.editor_dirty = true;
}

// Smooth curve adjustment: apply gradient to surrounding points
pub fn editor_graph_smooth_adjust(app: &mut App, delta: i16, range: usize) {
    if !app.editor_graph_mode { return; }
    let center = app.editor_graph_sel.min(100);
    let range = range.min(10); // Limit range to prevent excessive changes
    
    // Apply adjustment with falloff based on distance from center
    for i in 0..=100 {
        let distance = (i as i32 - center as i32).abs() as usize;
        if distance <= range {
            let falloff = 1.0 - (distance as f32 / range as f32);
            let adjusted_delta = (delta as f32 * falloff) as i16;
            
            let cur = app.editor_graph[i] as i16;
            let mut v = cur + adjusted_delta;
            if v < 0 { v = 0; }
            if v > 100 { v = 100; }
            app.editor_graph[i] = v as u8;
        }
    }
    app.editor_dirty = true;
}

// Apply smoothing filter to reduce jagged curves
pub fn editor_graph_apply_smoothing(app: &mut App) {
    if !app.editor_graph_mode { return; }
    
    let mut smoothed = app.editor_graph.clone();
    
    // Apply simple 3-point moving average
    for i in 1..100 {
        let avg = (app.editor_graph[i-1] as u16 + app.editor_graph[i] as u16 + app.editor_graph[i+1] as u16) / 3;
        smoothed[i] = avg as u8;
    }
    
    app.editor_graph = smoothed;
    app.editor_dirty = true;
}

pub fn editor_graph_commit_points(app: &mut App) {
    if app.editor_groups.is_empty() { return; }
    // Convert 0..100 bins into downsampled points (every 4°C) to satisfy max 32 points
    let mut pts: Vec<CurvePoint> = Vec::new();
    for t in (0..=100).step_by(4) {
        let pwm = app.editor_graph[t.min(100)];
        pts.push(CurvePoint { temp_c: t as f64, pwm_pct: pwm });
    }
    // Ensure at least two points
    if pts.len() < 2 { pts.push(CurvePoint { temp_c: 100.0, pwm_pct: app.editor_graph[100] }); }
    let g = &mut app.editor_groups[app.editor_group_idx];
    g.curve.points = pts;
    // Leave graph mode but keep selection
    app.editor_graph_mode = false;
    app.editor_point_idx = 0;
    app.editor_graph_input.clear();
    app.editor_graph_typed = false;
    app.editor_dirty = true;
}

pub fn editor_prev_group(app: &mut App) {
    if app.editor_groups.is_empty() { return; }
    if app.editor_group_idx > 0 { app.editor_group_idx -= 1; }
    app.editor_point_idx = 0;
}

pub fn editor_next_group(app: &mut App) {
    if app.editor_groups.is_empty() { return; }
    if app.editor_group_idx + 1 < app.editor_groups.len() { app.editor_group_idx += 1; }
    app.editor_point_idx = 0;
}

pub fn editor_set_temp_from_current(app: &mut App) {
    if app.editor_groups.is_empty() { return; }
    let src = match app.temps.get(app.temps_idx) { Some((s, _)) => s.clone(), None => return };
    app.editor_groups[app.editor_group_idx].temp_source = src;
}

pub fn editor_add_member_current(app: &mut App) {
    if app.editor_groups.is_empty() { return; }
    let pwm = match app.pwms.get(app.pwms_idx) { Some((s, _)) => s.clone(), None => return };
    let g = &mut app.editor_groups[app.editor_group_idx];
    if !g.members.contains(&pwm) { g.members.push(pwm); }
}

pub fn editor_remove_member_current(app: &mut App) {
    if app.editor_groups.is_empty() { return; }
    let pwm = match app.pwms.get(app.pwms_idx) { Some((s, _)) => s.clone(), None => return };
    let g = &mut app.editor_groups[app.editor_group_idx];
    g.members.retain(|m| m != &pwm);
}

pub fn editor_point_prev(app: &mut App) {
    if app.editor_groups.is_empty() { return; }
    if app.editor_point_idx > 0 { app.editor_point_idx -= 1; }
}

pub fn editor_point_next(app: &mut App) {
    if app.editor_groups.is_empty() { return; }
    let len = app.editor_groups[app.editor_group_idx].curve.points.len();
    if app.editor_point_idx + 1 < len { app.editor_point_idx += 1; }
}

pub fn editor_point_adjust_pwm(app: &mut App, delta: i16) {
    if app.editor_groups.is_empty() { return; }
    let g = &mut app.editor_groups[app.editor_group_idx];
    if g.curve.points.is_empty() { return; }
    let i = app.editor_point_idx.min(g.curve.points.len() - 1);
    let cur = g.curve.points[i].pwm_pct as i16;
    let mut v = cur + delta;
    if v < 0 { v = 0; }
    if v > 100 { v = 100; }
    g.curve.points[i].pwm_pct = v as u8;
}

pub fn editor_add_point(app: &mut App) {
    if app.editor_groups.is_empty() { return; }
    let g = &mut app.editor_groups[app.editor_group_idx];
    // Insert a point between current and next (or duplicate current + 5°C)
    if g.curve.points.is_empty() { return; }
    let i = app.editor_point_idx.min(g.curve.points.len() - 1);
    let (t, p) = if i + 1 < g.curve.points.len() {
        let a = &g.curve.points[i];
        let b = &g.curve.points[i + 1];
        ((a.temp_c + b.temp_c) / 2.0, ((a.pwm_pct as u16 + b.pwm_pct as u16) / 2) as u8)
    } else {
        let a = &g.curve.points[i];
        (a.temp_c + 5.0, a.pwm_pct)
    };
    g.curve.points.insert(i + 1, CurvePoint { temp_c: t, pwm_pct: p });
}

pub fn editor_remove_point(app: &mut App) {
    if app.editor_groups.is_empty() { return; }
    let g = &mut app.editor_groups[app.editor_group_idx];
    if g.curve.points.len() <= 2 { return; }
    let i = app.editor_point_idx.min(g.curve.points.len() - 1);
    g.curve.points.remove(i);
    if app.editor_point_idx >= g.curve.points.len() { app.editor_point_idx = g.curve.points.len() - 1; }
}

pub fn editor_save_curves(app: &mut App) {
    // Filter out empty groups
    fn sanitize_label(s: &str) -> String {
        // Mirror curves::is_safe_label: allow A-Za-z0-9 and ':', '_', '-', '.', ' '
        let mut out = String::new();
        for ch in s.chars() {
            if ch.is_ascii_alphanumeric() || matches!(ch, ':' | '_' | '-' | '.' | ' ') {
                out.push(ch);
            } else {
                out.push('_');
            }
        }
        out.trim().to_string()
    }

    let mut groups: Vec<CurveGroup> = app.editor_groups
        .iter()
        .cloned()
        .filter(|g| !g.members.is_empty() && !g.temp_source.is_empty() && g.curve.points.len() >= 2)
        .map(|mut g| {
            g.temp_source = sanitize_label(&g.temp_source);
            g.members = g.members.into_iter().map(|m| sanitize_label(&m)).collect();
            g.name = g.name.trim().to_string();
            g
        })
        .collect();

    // Ensure ALL mappings have curve groups - create default groups for any missing PWM controllers
    let mut covered_pwms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for group in &groups {
        for member in &group.members {
            covered_pwms.insert(member.clone());
        }
    }

    // Create consolidated curve groups for any unmapped PWM controllers
    let mut uncovered_pwms: Vec<String> = app.mappings
        .iter()
        .filter(|m| !covered_pwms.contains(&m.pwm))
        .map(|m| m.pwm.clone())
        .collect();

    if !uncovered_pwms.is_empty() {
        // Group PWMs by chip to reduce the number of groups
        let mut chip_groups: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
        
        for pwm in &uncovered_pwms {
            if let Some((chip, _)) = parse_chip_and_label(pwm) {
                chip_groups.entry(chip).or_default().push(pwm.clone());
            } else {
                // Fallback for PWMs without chip:label format
                chip_groups.entry("misc".to_string()).or_default().push(pwm.clone());
            }
        }

        let default_curve = crate::curves::CurveSpec {
            points: vec![
                crate::curves::CurvePoint { temp_c: 30.0, pwm_pct: 20 },
                crate::curves::CurvePoint { temp_c: 50.0, pwm_pct: 40 },
                crate::curves::CurvePoint { temp_c: 70.0, pwm_pct: 80 },
                crate::curves::CurvePoint { temp_c: 80.0, pwm_pct: 100 },
            ],
            min_pwm_pct: 0,
            max_pwm_pct: 100,
            floor_pwm_pct: 0,
            hysteresis_pct: 5,
            write_min_delta: 5,
            apply_delay_ms: 1000,
        };
        
        let temp_source = if !app.temps.is_empty() {
            app.temps[0].0.clone()
        } else {
            "temp0".to_string()
        };

        // Create one group per chip instead of per PWM
        for (chip, pwm_list) in chip_groups {
            if pwm_list.len() == 1 {
                // Single PWM - use its alias or name
                let pwm = &pwm_list[0];
                let group_name = app.pwm_aliases.get(pwm)
                    .cloned()
                    .unwrap_or_else(|| pwm.clone());
                
                groups.push(crate::curves::CurveGroup {
                    name: format!("Auto: {}", group_name),
                    members: pwm_list,
                    temp_source: sanitize_label(&temp_source),
                    curve: default_curve.clone(),
                });
            } else {
                // Multiple PWMs - group by chip
                groups.push(crate::curves::CurveGroup {
                    name: format!("Auto: {} ({} fans)", chip, pwm_list.len()),
                    members: pwm_list,
                    temp_source: sanitize_label(&temp_source),
                    curve: default_curve.clone(),
                });
            }
        }

        // Mark all as covered
        for pwm in uncovered_pwms {
            covered_pwms.insert(pwm);
        }
    }

    if groups.is_empty() {
        app.status = "Curve editor: no valid groups to save".to_string();
        return;
    }
    // Ensure every group's temp_source resolves to something non-empty after sanitization
    for g in &mut groups {
        if g.temp_source.is_empty() {
            // fallback to current temps selection or first
            g.temp_source = match app.temps.get(app.temps_idx) {
                Some((s, _)) => s.clone(),
                None => app.temps.first().map(|(s, _)| s.clone()).unwrap_or_else(|| "temp0".to_string()),
            };
        }
    }
    let cfg = CurvesConfig { version: 1, groups: groups.clone() };
    // Write to curves.json for compatibility
    match curves::write_curves(&cfg) {
        Ok(()) => app.status = "Saved curves to /etc/hyperfan/curves.json".to_string(),
        Err(e) => app.status = format!("Failed to save curves: {}", e),
    }
    // Also persist into system profile.json so the service can consume it directly
    let existing_overrides = crate::config::try_load_system_config().ok().map(|c| c.pwm_overrides).unwrap_or_default();
    let saved_profile = SavedConfig {
        mappings: app
            .mappings
            .iter()
            .map(|m| crate::config::SavedMapping { fan: m.fan.clone(), pwm: m.pwm.clone() })
            .collect(),
        metric: app.metric,
        curves: Some(CurvesConfig { version: 1, groups }),
        fan_aliases: app.fan_aliases.clone(),
        pwm_aliases: app.pwm_aliases.clone(),
        temp_aliases: app.temp_aliases.clone(),
        controller_groups: app.groups.clone(),
        pwm_overrides: existing_overrides,
    };
    if let Err(e) = write_system_config(&saved_profile) {
        app.status = format!("Saved curves.json; failed to save curves into system profile: {}", e);
    } else {
        app.status = "Saved curves.json".to_string();
    }
    // Saved successfully (at least curves.json). Clear dirty flag and return to main control pair view
    app.editor_dirty = false;
}

/// Apply curves to hardware by reading temperatures and setting PWM values accordingly
pub fn apply_curves_to_hardware(app: &App) {
    if app.editor_groups.is_empty() {
        return;
    }

    // Get current temperatures from all chips
    let chips = match hwmon::read_all() {
        Ok(chips) => chips,
        Err(_) => return, // Can't read temperatures, skip
    };

    // Build temperature map from all chips
    let mut temp_map: HashMap<String, f64> = HashMap::new();
    for chip in &chips {
        for (label, temp) in &chip.temps {
            temp_map.insert(label.clone(), *temp);
        }
    }

    for group in &app.editor_groups {
        // Get temperature for this group
        let current_temp = match temp_map.get(&group.temp_source) {
            Some(&temp) => temp,
            None => {
                // Fallback to first available temperature if source not found
                if let Some((&_, &temp)) = temp_map.iter().next() {
                    temp
                } else {
                    continue; // No temperatures available
                }
            }
        };

        // Calculate PWM percentage from curve
        let pwm_pct = curves::interp_pwm_percent(&group.curve.points, current_temp);
        let pwm_value = ((pwm_pct as f64 * 255.0) / 100.0) as u8;

        // Apply to all members of this group
        for member in &group.members {
            if let Some((chip, label)) = parse_chip_and_label(member) {
                if let Some(idx) = hwmon::find_pwm_index_by_label(&chip, &label) {
                    let _ = hwmon::write_pwm(&chip, idx, pwm_value);
                }
            }
        }
    }
}

pub fn save_system_config(app: &mut App) {
    // Preserve existing curves and overrides if present in the current system profile
    let existing_cfg = crate::config::try_load_system_config().ok();
    let existing_curves = existing_cfg.as_ref().and_then(|c| c.curves.clone());
    let existing_overrides = existing_cfg.map(|c| c.pwm_overrides).unwrap_or_default();
    let saved = SavedConfig {
        mappings: app
            .mappings
            .iter()
            .map(|m| crate::config::SavedMapping { fan: m.fan.clone(), pwm: m.pwm.clone() })
            .collect(),
        metric: app.metric,
        curves: existing_curves,
        fan_aliases: app.fan_aliases.clone(),
        pwm_aliases: app.pwm_aliases.clone(),
        temp_aliases: app.temp_aliases.clone(),
        controller_groups: app.groups.clone(),
        pwm_overrides: existing_overrides,
    };
    match write_system_config(&saved) {
        Ok(()) => app.status = "Saved config to /etc/hyperfan/profile.json".to_string(),
        Err(e) => app.status = format!("Failed to save config: {}", e),
    }
}

pub fn start_save_system_config(app: &mut App) {
    app.show_confirm_save_popup = true;
}

pub fn apply_save_system_config(app: &mut App) {
    // Preserve existing curves and overrides if present in the current system profile
    let existing_cfg = crate::config::try_load_system_config().ok();
    let existing_curves = existing_cfg.as_ref().and_then(|c| c.curves.clone());
    let existing_overrides = existing_cfg.map(|c| c.pwm_overrides).unwrap_or_default();
    let saved = SavedConfig {
        mappings: app
            .mappings
            .iter()
            .map(|m| crate::config::SavedMapping { fan: m.fan.clone(), pwm: m.pwm.clone() })
            .collect(),
        metric: app.metric,
        curves: existing_curves,
        fan_aliases: app.fan_aliases.clone(),
        pwm_aliases: app.pwm_aliases.clone(),
        temp_aliases: app.temp_aliases.clone(),
        controller_groups: app.groups.clone(),
        pwm_overrides: existing_overrides,
    };
    match write_system_config(&saved) {
        Ok(()) => app.status = "Saved config to /etc/hyperfan/profile.json".to_string(),
        Err(e) => app.status = format!("Failed to save config: {}", e),
    }
    app.show_confirm_save_popup = false;
}

pub fn cancel_save_system_config(app: &mut App) {
    app.show_confirm_save_popup = false;
    app.status = "Save canceled".to_string();
}

pub fn start_set_pwm(app: &mut App) {
    app.set_pwm_input.clear();
    app.set_pwm_feedback = None;
    app.set_pwm_typed = false;

    let target = match app.focus {
        Focus::Pwms => {
            let Some((full, _)) = app.pwms.get(app.pwms_idx).cloned() else { return; };
            if let Some((chip, label)) = parse_chip_and_label(&full) {
                if let Some(idx) = hwmon::find_pwm_index_by_label(&chip, &label) {
                    Some((chip, idx, label))
                } else {
                    None
                }
            } else {
                None
            }
        }
        Focus::Fans => {
            let Some((fan_full, _)) = app.fans.get(app.fans_idx).cloned() else { return; };
            if let Some(map) = app.mappings.iter().find(|m| m.fan == fan_full) {
                if let Some((chip, label)) = parse_chip_and_label(&map.pwm) {
                    if let Some(idx) = hwmon::find_pwm_index_by_label(&chip, &label) {
                        Some((chip, idx, label))
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                if let Some((chip, flabel)) = parse_chip_and_label(&fan_full) {
                    if let Some(n) = flabel.strip_prefix("fan").and_then(|s| s.parse::<usize>().ok()) {
                        let pwm_label = format!("pwm{}", n);
                        if let Some(idx) = hwmon::find_pwm_index_by_label(&chip, &pwm_label) {
                            Some((chip, idx, pwm_label))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        }
        Focus::Control => {
            if app.mappings.is_empty() {
                return;
            }
            let pwm_full = app.mappings[app.control_idx].pwm.clone();
            if let Some((chip, label)) = parse_chip_and_label(&pwm_full) {
                if let Some(idx) = hwmon::find_pwm_index_by_label(&chip, &label) {
                    Some((chip, idx, label))
                } else {
                    None
                }
            } else {
                None
            }
        }
        Focus::Temps => return,
    };

    // If we couldn't resolve a PWM target when starting from a Fan selection,
    // show a popup warning and do not open the Set PWM popup.
    if target.is_none() {
        if matches!(app.focus, Focus::Fans) {
            app.warning_message = "hyperfan can't seem to find a PWM controller for this fan.".to_string();
            app.show_warning_popup = true;
            app.show_set_pwm_popup = false;
            return;
        }
    }

    // Store target and prefill current percent if available
    app.set_pwm_target = target.clone();
    if let Some((chip, _idx, label)) = &target {
        let full = format!("{}:{}", chip, label);
        if let Some((_, raw)) = app.pwms.iter().find(|(name, _)| name == &full) {
            let pct = ((*raw as f64) * 100.0 / 255.0).round() as u16;
            app.set_pwm_input = pct.to_string();
        }
    }
    app.show_set_pwm_popup = true;
}

pub fn apply_set_pwm(app: &mut App) {
    // Interpret input as percentage (0-100)
    let pct: u8 = match app.set_pwm_input.parse::<u16>() {
        Ok(n) if n <= 100 => n as u8,
        _ => {
            app.set_pwm_feedback = Some((true, "Invalid PWM percent (0-100)".to_string()));
            return;
        }
    };
    // Convert percent to raw 0-255
    let val: u8 = ((pct as u16) * 255 / 100) as u8;
    if let Some((chip, idx, label)) = app.set_pwm_target.clone() {
        match hwmon::write_pwm(&chip, idx, val) {
            Ok(_) => {
                // Persist override into system profile
                let key = format!("{}:{}", chip, label);
                let cfg = crate::config::try_load_system_config().ok();
                let mut pwm_overrides = cfg.as_ref().map(|c| c.pwm_overrides.clone()).unwrap_or_default();
                pwm_overrides.insert(key, pct);
                let saved = SavedConfig {
                    mappings: app
                        .mappings
                        .iter()
                        .map(|m| crate::config::SavedMapping { fan: m.fan.clone(), pwm: m.pwm.clone() })
                        .collect(),
                    metric: app.metric,
                    curves: cfg.as_ref().and_then(|c| c.curves.clone()),
                    fan_aliases: app.fan_aliases.clone(),
                    pwm_aliases: app.pwm_aliases.clone(),
                    temp_aliases: app.temp_aliases.clone(),
                    controller_groups: app.groups.clone(),
                    pwm_overrides,
                };
                let _ = write_system_config(&saved);

                app.status = format!("Set {} pwm{} to {}% (raw {})", chip, idx, pct, val);
                app.show_set_pwm_popup = false;
                app.set_pwm_input.clear();
                app.set_pwm_target = None;
                app.set_pwm_feedback = None;
                app.refresh();
            }
            Err(e) => {
                app.set_pwm_feedback = Some((
                    true,
                    format!("Failed to set PWM {}:{} -> {}% (raw {}) ({})", chip, label, pct, val, e),
                ));
            }
        }
    } else {
        app.set_pwm_feedback = Some((true, "No PWM target resolved".to_string()));
    }
}

pub fn focus_next(app: &mut App) {
    app.focus = match app.focus {
        Focus::Fans => Focus::Pwms,
        Focus::Pwms => Focus::Temps,
        Focus::Temps => Focus::Control,
        Focus::Control => Focus::Fans,
    };
}

pub fn focus_prev(app: &mut App) {
    app.focus = match app.focus {
        Focus::Fans => Focus::Control,
        Focus::Pwms => Focus::Fans,
        Focus::Temps => Focus::Pwms,
        Focus::Control => Focus::Temps,
    };
}

pub fn move_up(app: &mut App) {
    match app.focus {
        Focus::Fans => {
            if app.fans_idx > 0 {
                app.fans_idx -= 1;
            }
        }
        Focus::Pwms => {
            if app.pwms_idx > 0 {
                app.pwms_idx -= 1;
            }
        }
        Focus::Temps => {
            if app.temps_idx > 0 {
                app.temps_idx -= 1;
            }
        }
        Focus::Control => {
            if app.control_idx > 0 {
                app.control_idx -= 1;
            }
        }
    }
}

pub fn move_down(app: &mut App) {
    match app.focus {
        Focus::Fans => {
            if app.fans_idx + 1 < app.fans.len() {
                app.fans_idx += 1;
            }
        }
        Focus::Pwms => {
            if app.pwms_idx + 1 < app.pwms.len() {
                app.pwms_idx += 1;
            }
        }
        Focus::Temps => {
            if app.temps_idx + 1 < app.temps.len() {
                app.temps_idx += 1;
            }
        }
        Focus::Control => {
            if app.control_idx + 1 < app.mappings.len() {
                app.control_idx += 1;
            }
        }
    }
}

pub fn map_current(app: &mut App) {
    if app.fans.is_empty() || app.pwms.is_empty() {
        return;
    }
    let fan = app
        .fans
        .get(app.fans_idx)
        .map(|(s, _)| s.clone())
        .unwrap_or_default();
    let pwm = app
        .pwms
        .get(app.pwms_idx)
        .map(|(s, _)| s.clone())
        .unwrap_or_default();
    if fan.is_empty() || pwm.is_empty() {
        return;
    }
    app.mappings.push(Mapping { fan, pwm });
    app.control_idx = app.mappings.len().saturating_sub(1);
    let _ = save_mappings(&app.mappings);
}

pub fn delete_mapping(app: &mut App) {
    if app.mappings.is_empty() {
        return;
    }
    let idx = app.control_idx.min(app.mappings.len() - 1);
    app.mappings.remove(idx);
    if app.control_idx >= app.mappings.len() {
        app.control_idx = app.mappings.len().saturating_sub(1);
    }
    let _ = save_mappings(&app.mappings);
}

pub fn toggle_curve_popup(app: &mut App) {
    app.show_curve_popup = !app.show_curve_popup;
    if app.show_curve_popup {
        app.status = "Curve manager opened (Enter to save, Esc to cancel)".to_string();
    } else {
        app.status = "Curve manager closed".to_string();
    }
}

pub fn save_curve(app: &mut App) {
    // Build a minimal curves.json using current curve points and available selections/mappings
    let points: Vec<CurvePoint> = app
        .curve_temp_points
        .iter()
        .map(|(t, p)| CurvePoint { temp_c: *t, pwm_pct: *p })
        .collect();

    // Determine group members and temp source
    let mut members: Vec<String> = Vec::new();
    let mut temp_source: Option<String> = None;

    if !app.mappings.is_empty() {
        // Use all mapped PWMs
        for m in &app.mappings {
            if !members.contains(&m.pwm) { members.push(m.pwm.clone()); }
        }
        // Choose temp source from current selection or first available temp
        if let Some((temp_full, _)) = app.temps.get(app.temps_idx) {
            temp_source = Some(temp_full.clone());
        } else if let Some((temp_full, _)) = app.temps.first() {
            temp_source = Some(temp_full.clone());
        }
    } else {
        // Fallback to current selections in lists
        if let Some((pwm_full, _)) = app.pwms.get(app.pwms_idx) { members.push(pwm_full.clone()); }
        if let Some((temp_full, _)) = app.temps.get(app.temps_idx) { temp_source = Some(temp_full.clone()); }
        else if let Some((temp_full, _)) = app.temps.first() { temp_source = Some(temp_full.clone()); }
    }

    let Some(temp_source) = temp_source else {
        app.status = "Cannot save curves: need at least one PWM and a temp source".to_string();
        return;
    };

    if members.is_empty() {
        app.status = "Cannot save curves: need at least one PWM".to_string();
        return;
    }

    let group = CurveGroup {
        name: "Default".to_string(),
        members,
        temp_source,
        curve: CurveSpec {
            points,
            min_pwm_pct: 0,
            max_pwm_pct: 100,
            floor_pwm_pct: 0,
            hysteresis_pct: 5,
            write_min_delta: 5,
            apply_delay_ms: 0,
        },
    };

    let cfg = CurvesConfig { version: 1, groups: vec![group.clone()] };
    // Write to curves.json for compatibility
    match curves::write_curves(&cfg) {
        Ok(()) => app.status = "Saved curves to /etc/hyperfan/curves.json".to_string(),
        Err(e) => app.status = format!("Failed to save curves: {}", e),
    }
    // Also persist into system profile.json including curves
    let existing_overrides = crate::config::try_load_system_config().ok().map(|c| c.pwm_overrides).unwrap_or_default();
    let saved_profile = SavedConfig {
        mappings: app
            .mappings
            .iter()
            .map(|m| crate::config::SavedMapping { fan: m.fan.clone(), pwm: m.pwm.clone() })
            .collect(),
        metric: app.metric,
        curves: Some(CurvesConfig { version: 1, groups: vec![group] }),
        fan_aliases: app.fan_aliases.clone(),
        pwm_aliases: app.pwm_aliases.clone(),
        temp_aliases: app.temp_aliases.clone(),
        controller_groups: app.groups.clone(),
        pwm_overrides: existing_overrides,
    };
    if let Err(e) = write_system_config(&saved_profile) {
        app.status = format!("Saved curves.json; failed to save curves into system profile: {}", e);
    }
    app.show_curve_popup = false;
}

// Convenience: create a curve group from the currently selected controller group
pub fn editor_add_group_from_current_controller_group(app: &mut App) {
    if app.groups.is_empty() { return; }
    let g = &app.groups[app.group_idx.min(app.groups.len() - 1)];
    // Pick temp source from current temps selection or first
    let temp_src = match app.temps.get(app.temps_idx) {
        Some((s, _)) => s.clone(),
        None => app.temps.first().map(|(s, _)| s.clone()).unwrap_or_default(),
    };
    let group = CurveGroup {
        name: g.name.clone(),
        members: g.members.clone(),
        temp_source: temp_src,
        curve: CurveSpec {
            points: vec![
                CurvePoint { temp_c: 30.0, pwm_pct: 20 },
                CurvePoint { temp_c: 40.0, pwm_pct: 30 },
                CurvePoint { temp_c: 50.0, pwm_pct: 50 },
                CurvePoint { temp_c: 60.0, pwm_pct: 70 },
                CurvePoint { temp_c: 70.0, pwm_pct: 100 },
            ],
            min_pwm_pct: 0, max_pwm_pct: 100, floor_pwm_pct: 0, hysteresis_pct: 5, write_min_delta: 5, apply_delay_ms: 0,
        },
    };
    app.editor_groups.push(group);
    app.editor_group_idx = app.editor_groups.len().saturating_sub(1);
    app.editor_point_idx = 0;
}

// Convenience: create a curve group from the currently selected PWM
pub fn editor_add_group_from_current_pwm(app: &mut App) {
    let pwm = match app.pwms.get(app.pwms_idx) { Some((s, _)) => s.clone(), None => return };
    let temp_src = match app.temps.get(app.temps_idx) {
        Some((s, _)) => s.clone(),
        None => app.temps.first().map(|(s, _)| s.clone()).unwrap_or_default(),
    };
    let name = format!("{} curve", pwm.split(':').last().unwrap_or("PWM"));
    let group = CurveGroup {
        name,
        members: vec![pwm],
        temp_source: temp_src,
        curve: CurveSpec {
            points: vec![
                CurvePoint { temp_c: 30.0, pwm_pct: 20 },
                CurvePoint { temp_c: 40.0, pwm_pct: 30 },
                CurvePoint { temp_c: 50.0, pwm_pct: 50 },
                CurvePoint { temp_c: 60.0, pwm_pct: 70 },
                CurvePoint { temp_c: 70.0, pwm_pct: 100 },
            ],
            min_pwm_pct: 0, max_pwm_pct: 100, floor_pwm_pct: 0, hysteresis_pct: 5, write_min_delta: 5, apply_delay_ms: 0,
        },
    };
    app.editor_groups.push(group);
    app.editor_group_idx = app.editor_groups.len().saturating_sub(1);
    app.editor_point_idx = 0;
}

pub fn start_auto_detect(app: &mut App) {
    // Only open the popup and await user confirmation
    app.show_auto_detect = true;
    app.auto_detect_await_confirm = true;
    app.auto_detect_progress = String::new();
    app.auto_detect_percent = 0.0;
    // Clear previous results
    match app.auto_detect_results.lock() {
        Ok(mut results) => results.clear(),
        Err(poisoned) => {
            // Recover and warn user, rather than panic
            let mut results = poisoned.into_inner();
            results.clear();
            app.warning_message = "Auto-detect: recovered from a corrupted results lock; state was reset".to_string();
            app.show_warning_popup = true;
        }
    }
}

pub fn confirm_auto_detect(app: &mut App) {
    // Switch from confirm state to running state
    app.auto_detect_await_confirm = false;
    app.auto_detect_progress = "Initializing auto-detection...".to_string();
    app.auto_detect_percent = 0.0;

    // Clear previous results
    {
        match app.auto_detect_results.lock() {
            Ok(mut results) => results.clear(),
            Err(poisoned) => {
                let mut results = poisoned.into_inner();
                results.clear();
                app.warning_message = "Auto-detect: recovered from a corrupted results lock; state was reset".to_string();
                app.show_warning_popup = true;
            }
        }
    }

    // Check if already running
    {
        match app.auto_detect_running.lock() {
            Ok(mut running) => {
                if *running { return; }
                *running = true;
            }
            Err(poisoned) => {
                // Recover by assuming not running and setting to true
                let mut running = poisoned.into_inner();
                if *running { return; }
                *running = true;
                app.warning_message = "Auto-detect: recovered from a corrupted running flag".to_string();
                app.show_warning_popup = true;
            }
        }
    }

    // Clone Arc references for the thread
    let running_flag: Arc<Mutex<bool>> = Arc::clone(&app.auto_detect_running);
    let results_store: Arc<Mutex<Vec<hwmon::FanPwmPairing>>> = Arc::clone(&app.auto_detect_results);

    // Run detection in background thread
    thread::spawn(move || {
        match hwmon::auto_detect_pairings_with_progress() {
            Ok(pairings) => {
                match results_store.lock() {
                    Ok(mut results) => { *results = pairings; }
                    Err(poisoned) => {
                        let mut results = poisoned.into_inner();
                        *results = pairings;
                        if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open("/tmp/hyperfan_autodetect.log") {
                            let _ = writeln!(f, "auto-detect: warning: recovered from poisoned results mutex while writing results");
                        }
                    }
                }
            }
            Err(e) => {
                if let Ok(mut f) = fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open("/tmp/hyperfan_autodetect.log")
                {
                    let _ = writeln!(f, "auto-detect: error: {}", e);
                }
            }
        }

        match running_flag.lock() {
            Ok(mut running) => { *running = false; }
            Err(poisoned) => {
                let mut running = poisoned.into_inner();
                *running = false;
                if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open("/tmp/hyperfan_autodetect.log") {
                    let _ = writeln!(f, "auto-detect: warning: recovered from poisoned running mutex while clearing flag");
                }
            }
        }
    });
}

pub fn apply_auto_detect(app: &mut App) {
    let results = match app.auto_detect_results.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            app.warning_message = "Auto-detect: results store was corrupted; attempting to use last known results".to_string();
            app.show_warning_popup = true;
            poisoned.into_inner()
        }
    };
    // Replace existing mappings with detected results
    let mut new_mappings: Vec<Mapping> = Vec::new();
    for pairing in results.iter() {
        let fan = format!("{}:{}", pairing.fan_chip, pairing.fan_label);
        let pwm = format!("{}:{}", pairing.pwm_chip, pairing.pwm_label);
        new_mappings.push(Mapping { fan, pwm });
    }
    app.mappings = new_mappings;
    if !app.mappings.is_empty() {
        app.control_idx = app.mappings.len() - 1;
    } else {
        app.control_idx = 0;
    }
    // Save to user config
    let _ = save_mappings(&app.mappings);
    // Save to system profile.json (preserve existing curves and overrides if any)
    let existing_cfg = crate::config::try_load_system_config().ok();
    let existing_curves = existing_cfg.as_ref().and_then(|c| c.curves.clone());
    let existing_overrides = existing_cfg.map(|c| c.pwm_overrides).unwrap_or_default();
    let saved = SavedConfig {
        mappings: app.mappings.clone().into_iter().map(|m| crate::config::SavedMapping { fan: m.fan, pwm: m.pwm }).collect(),
        metric: app.metric,
        curves: existing_curves,
        fan_aliases: app.fan_aliases.clone(),
        pwm_aliases: app.pwm_aliases.clone(),
        temp_aliases: app.temp_aliases.clone(),
        controller_groups: app.groups.clone(),
        pwm_overrides: existing_overrides,
    };
    match write_system_config(&saved) {
        Ok(()) => app.status = "Applied detected mappings and saved to /etc/hyperfan/profile.json".to_string(),
        Err(e) => app.status = format!("Applied detected mappings (failed to save system profile: {})", e),
    }
    app.show_auto_detect = false;
    // Drop the results guard before mutably borrowing `app` in refresh
    drop(results);
    // Ensure CONTROL section immediately reflects live RPM/PWM values
    // without waiting for the next periodic refresh tick.
    app.refresh();
}
