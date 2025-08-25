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

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use crate::app::App;
use crate::handlers::*;
use crate::hwmon;

/// Main event handler that processes keyboard input
pub fn handle_key_event(app: &mut App, key_event: KeyEvent) -> anyhow::Result<bool> {
    let KeyEvent { code, modifiers, .. } = key_event;
    
    // Handle popup states first (highest priority)
    if handle_popup_events(app, code, modifiers)? {
        return Ok(false);
    }
    
    // Handle page-specific events
    if app.show_curve_editor {
        return handle_curve_editor_events(app, code, modifiers);
    }
    
    if app.show_groups_manager {
        return handle_groups_manager_events(app, code, modifiers);
    }
    
    // Handle global events
    handle_global_events(app, code, modifiers)
}

/// Handle all popup-related events (highest priority)
fn handle_popup_events(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> anyhow::Result<bool> {
    // Groups Map PWM->FAN popup
    if app.show_map_pwm_popup {
        match code {
            KeyCode::Esc => groups_cancel_map_pwm(app),
            KeyCode::Enter => groups_apply_map_pwm(app),
            KeyCode::Up => {
                if app.map_fan_idx > 0 {
                    app.map_fan_idx -= 1;
                }
            }
            KeyCode::Down => {
                if app.map_fan_idx + 1 < app.fans.len() {
                    app.map_fan_idx += 1;
                }
            }
            _ => {}
        }
        return Ok(true);
    }
    
    // Warning popup
    if app.show_warning_popup {
        if matches!(code, KeyCode::Esc | KeyCode::Enter) {
            app.show_warning_popup = false;
            app.warning_message.clear();
        }
        return Ok(true);
    }
    
    // Group name popup
    if app.show_group_name_popup {
        handle_group_name_popup(app, code)?;
        return Ok(true);
    }
    
    // Rename popup
    if app.show_rename_popup {
        handle_rename_popup(app, code)?;
        return Ok(true);
    }
    
    // Set PWM popup
    if app.show_set_pwm_popup {
        handle_set_pwm_popup(app, code, modifiers)?;
        return Ok(true);
    }
    
    // Confirm save popup
    if app.show_confirm_save_popup {
        match code {
            KeyCode::Esc => cancel_save_system_config(app),
            KeyCode::Enter => apply_save_system_config(app),
            _ => {}
        }
        return Ok(true);
    }
    
    // Auto-detect popup
    if app.show_auto_detect {
        handle_auto_detect_popup(app, code)?;
        return Ok(true);
    }
    
    // Curve editor popups
    if handle_curve_editor_popups(app, code, modifiers)? {
        return Ok(true);
    }
    
    Ok(false)
}

/// Handle group name popup input
fn handle_group_name_popup(app: &mut App, code: KeyCode) -> anyhow::Result<()> {
    match code {
        KeyCode::Esc => groups_cancel_new_name(app),
        KeyCode::Enter => groups_apply_new_name(app),
        KeyCode::Backspace => {
            app.group_name_input.pop();
        }
        KeyCode::Char(c) => {
            // Accept only letters, numbers and single '-' (no consecutive '-') with max 20 chars
            let allowed = c.is_ascii_alphanumeric() || c == '-';
            if allowed && app.group_name_input.len() < 20 {
                if c == '-' {
                    if !app.group_name_input.ends_with('-') {
                        app.group_name_input.push('-');
                    }
                } else {
                    app.group_name_input.push(c);
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Handle rename popup input
fn handle_rename_popup(app: &mut App, code: KeyCode) -> anyhow::Result<()> {
    match code {
        KeyCode::Esc => cancel_rename(app),
        KeyCode::Enter => apply_rename(app),
        KeyCode::Backspace => {
            app.rename_input.pop();
        }
        KeyCode::Char('d') => revert_rename_default(app),
        KeyCode::Char(c) => app.rename_input.push(c),
        _ => {}
    }
    Ok(())
}

/// Handle set PWM popup input (two-digit overwrite, special 100)
fn handle_set_pwm_popup(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> anyhow::Result<()> {
    match code {
        KeyCode::Esc => {
            app.show_set_pwm_popup = false;
            app.set_pwm_input.clear();
            app.set_pwm_target = None;
            app.set_pwm_feedback = None;
            app.set_pwm_typed = false;
        }
        KeyCode::Enter => apply_set_pwm(app),
        KeyCode::Char(c) if c.is_ascii_digit() => {
            handle_pwm_digit_input(app, c);
        }
        KeyCode::Backspace => handle_pwm_backspace(app),
        KeyCode::Delete => {
            app.set_pwm_input.clear();
            app.set_pwm_typed = false;
        }
        KeyCode::Up => adjust_pwm_value(app, modifiers, 1),
        KeyCode::Down => adjust_pwm_value(app, modifiers, -1),
        _ => {}
    }
    Ok(())
}

/// Handle PWM digit input logic
fn handle_pwm_digit_input(app: &mut App, digit: char) {
    let mut s = if app.set_pwm_input.is_empty() {
        "00".to_string()
    } else {
        app.set_pwm_input.clone()
    };
    
    if !app.set_pwm_typed {
        // First digit typed: start fresh from 00 -> 0d
        app.set_pwm_typed = true;
        let dval = digit.to_digit(10).unwrap_or(0);
        s = format!("{:02}", dval);
    } else {
        // Already typing
        if s == "100" {
            // restart from 0d
            let dval = digit.to_digit(10).unwrap_or(0);
            s = format!("{:02}", dval);
        } else {
            // shift last two digits and append new
            if s.len() < 2 {
                s = format!("{:0>2}", s);
            }
            let last = s.chars().nth(1).unwrap_or('0');
            let new_two = format!("{}{}", last, digit);
            // special-case: 10 + 0 -> 100
            if s == "10" && digit == '0' {
                s = "100".to_string();
            } else {
                s = new_two;
            }
        }
    }
    
    // Clamp check and assign
    if s == "100" {
        app.set_pwm_input = s;
    } else if let Ok(v) = s.parse::<u16>() {
        if v <= 99 {
            app.set_pwm_input = format!("{:02}", v);
        } else {
            app.set_pwm_input = "99".to_string();
        }
    }
}

/// Handle PWM backspace logic
fn handle_pwm_backspace(app: &mut App) {
    let mut s = if app.set_pwm_input.is_empty() {
        "00".to_string()
    } else {
        app.set_pwm_input.clone()
    };
    
    if s == "100" {
        s = "10".to_string();
    } else {
        if s.len() < 2 {
            s = format!("{:0>2}", s);
        }
        let first = s.chars().next().unwrap_or('0');
        s = format!("0{}", first);
    }
    
    if s == "00" {
        app.set_pwm_typed = false;
    }
    app.set_pwm_input = if s == "00" { String::new() } else { s };
}

/// Adjust PWM value with arrow keys
fn adjust_pwm_value(app: &mut App, modifiers: KeyModifiers, direction: i16) {
    let step = if modifiers.contains(KeyModifiers::SHIFT) { 5 } else { 1 };
    let cur = app.set_pwm_input.parse::<i16>().unwrap_or(0);
    let v = (cur + direction * step).clamp(0, 100);
    app.set_pwm_input = v.to_string();
}

/// Handle auto-detect popup events
fn handle_auto_detect_popup(app: &mut App, code: KeyCode) -> anyhow::Result<()> {
    match code {
        KeyCode::Esc => {
            // If detection is running, request graceful cancel (restores original PWMs)
            let running = match app.auto_detect_running.lock() {
                Ok(guard) => *guard,
                Err(poisoned) => {
                    app.warning_message = "Auto-detect state was corrupted; attempting to cancel safely".to_string();
                    app.show_warning_popup = true;
                    *poisoned.into_inner()
                }
            };
            if running {
                hwmon::request_cancel_autodetect();
            }
            app.show_auto_detect = false;
            app.auto_detect_await_confirm = false;
        }
        KeyCode::Enter if app.auto_detect_await_confirm => confirm_auto_detect(app),
        KeyCode::Enter => {
            let is_running = match app.auto_detect_running.lock() {
                Ok(guard) => *guard,
                Err(poisoned) => {
                    app.warning_message = "Auto-detect state was corrupted; proceeding with recovered state".to_string();
                    app.show_warning_popup = true;
                    *poisoned.into_inner()
                }
            };
            if !is_running {
                apply_auto_detect(app);
            }
        }
        _ => {}
    }
    Ok(())
}

/// Handle curve editor popup events
fn handle_curve_editor_popups(app: &mut App, code: KeyCode, _modifiers: KeyModifiers) -> anyhow::Result<bool> {
    // Editor save confirm
    if app.show_editor_save_confirm {
        match code {
            KeyCode::Esc => app.show_editor_save_confirm = false,
            KeyCode::Enter => {
                editor_save_curves(app);
                app.show_editor_save_confirm = false;
            }
            _ => {}
        }
        return Ok(true);
    }
    
    // Delay popup
    if app.show_curve_delay_popup {
        match code {
            KeyCode::Enter => editor_apply_delay_popup(app),
            KeyCode::Esc => editor_cancel_delay_popup(app),
            KeyCode::Backspace => {
                app.curve_delay_input.pop();
            }
            KeyCode::Char(c) if c.is_ascii_digit() => app.curve_delay_input.push(c),
            _ => {}
        }
        return Ok(true);
    }
    
    // Hysteresis popup
    if app.show_curve_hyst_popup {
        match code {
            KeyCode::Esc => editor_cancel_hyst_popup(app),
            KeyCode::Enter => editor_apply_hyst_popup(app),
            KeyCode::Backspace => {
                app.curve_hyst_input.pop();
            }
            KeyCode::Char(c) if c.is_ascii_digit() => app.curve_hyst_input.push(c),
            _ => {}
        }
        return Ok(true);
    }
    
    // Temperature source popup
    if app.show_temp_source_popup {
        match code {
            KeyCode::Esc => editor_cancel_temp_source_popup(app),
            KeyCode::Enter => editor_apply_temp_source_popup(app),
            KeyCode::Up => editor_temp_source_move_selection(app, -1),
            KeyCode::Down => editor_temp_source_move_selection(app, 1),
            _ => {}
        }
        return Ok(true);
    }
    
    // Curve popup
    if app.show_curve_popup {
        match code {
            KeyCode::Esc => app.show_curve_popup = false,
            KeyCode::Enter => save_curve(app),
            _ => {}
        }
        return Ok(true);
    }
    
    Ok(false)
}

/// Handle curve editor specific events
fn handle_curve_editor_events(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> anyhow::Result<bool> {
    // Disable quit while inside curve editor
    if matches!(code, KeyCode::Char('q')) {
        return Ok(false);
    }
    
    // Graph mode handling
    if app.editor_graph_mode {
        return handle_graph_mode_events(app, code, modifiers);
    }
    
    match (code, modifiers) {
        (KeyCode::Esc, _) => {
            if app.editor_dirty {
                app.show_editor_save_confirm = true;
            } else {
                toggle_curve_editor(app);
            }
        }
        (KeyCode::Char('u'), _) => editor_graph_toggle_mode(app),
        (KeyCode::Char('h'), _) => editor_start_delay_popup(app),
        (KeyCode::Char('y'), _) => editor_start_hyst_popup(app),
        (KeyCode::Char('t'), _) => editor_start_temp_source_popup(app),
        (KeyCode::Left, _) => app.editor_focus_right = false,
        (KeyCode::Right, _) => app.editor_focus_right = true,
        (KeyCode::Up, _) if !app.editor_focus_right => {
            if app.editor_group_idx > 0 {
                app.editor_group_idx -= 1;
                app.editor_point_idx = 0;
            }
        }
        (KeyCode::Down, _) if !app.editor_focus_right => {
            if app.editor_group_idx + 1 < app.editor_groups.len() {
                app.editor_group_idx += 1;
                app.editor_point_idx = 0;
            }
        }
        (KeyCode::Char('n'), _) => editor_add_group(app),
        (KeyCode::Char('p'), _) => editor_add_member_current(app),
        (KeyCode::Char('x'), _) => editor_remove_member_current(app),
        (KeyCode::Char('['), _) => editor_point_prev(app),
        (KeyCode::Char(']'), _) => editor_point_next(app),
        (KeyCode::Char('+' | '='), _) => editor_point_adjust_pwm(app, 1),
        (KeyCode::Char('-'), _) => editor_point_adjust_pwm(app, -1),
        (KeyCode::Char('a'), _) => editor_add_point(app),
        (KeyCode::Backspace, _) => editor_remove_point(app),
        (KeyCode::Char('s'), _) => app.show_editor_save_confirm = true,
        (KeyCode::Char('d'), _) => editor_apply_default_curves(app),
        _ => {}
    }
    
    Ok(false)
}

/// Handle graph mode specific events
fn handle_graph_mode_events(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> anyhow::Result<bool> {
    match (code, modifiers) {
        (KeyCode::Esc, _) => {
            // Cancel graph mode, do not commit
            app.editor_graph_mode = false;
            app.editor_graph_input.clear();
            app.editor_graph_typed = false;
        }
        (KeyCode::Enter, _) => editor_graph_commit_points(app),
        (KeyCode::Left, m) => {
            let step = if m.contains(KeyModifiers::SHIFT) { -5 } else { -1 };
            editor_graph_move_sel(app, step);
        }
        (KeyCode::Right, m) => {
            let step = if m.contains(KeyModifiers::SHIFT) { 5 } else { 1 };
            editor_graph_move_sel(app, step);
        }
        (KeyCode::Home, _) => editor_graph_jump_to_temp(app, 0),
        (KeyCode::End, _) => editor_graph_jump_to_temp(app, 100),
        (KeyCode::PageUp, _) => editor_graph_jump_to_next_point(app, false),
        (KeyCode::PageDown, _) => editor_graph_jump_to_next_point(app, true),
        // Quick jump to common temperature points
        (KeyCode::Char('1'), m) if m.contains(KeyModifiers::CONTROL) => {
            editor_graph_jump_to_temp(app, 25);
        }
        (KeyCode::Char('2'), m) if m.contains(KeyModifiers::CONTROL) => {
            editor_graph_jump_to_temp(app, 50);
        }
        (KeyCode::Char('3'), m) if m.contains(KeyModifiers::CONTROL) => {
            editor_graph_jump_to_temp(app, 75);
        }
        (KeyCode::Char('m'), _) => editor_graph_apply_smoothing(app),
        (KeyCode::Char(c), _) if c.is_ascii_digit() => {
            handle_graph_digit_input(app, c);
        }
        (KeyCode::Backspace, _) => handle_graph_backspace(app),
        (KeyCode::Delete, _) => {
            app.editor_graph_input.clear();
            app.editor_graph_typed = false;
            let sel = app.editor_graph_sel.min(100);
            app.editor_graph[sel] = 0;
        }
        (KeyCode::Up, m) => {
            if m.contains(KeyModifiers::CONTROL) {
                // Smooth adjustment with falloff
                let step = if m.contains(KeyModifiers::SHIFT) { 5 } else { 1 };
                editor_graph_smooth_adjust(app, step, 3);
            } else {
                let step = if m.contains(KeyModifiers::SHIFT) { 5 } else { 1 };
                editor_graph_adjust(app, step);
            }
        }
        (KeyCode::Down, m) => {
            if m.contains(KeyModifiers::CONTROL) {
                // Smooth adjustment with falloff
                let step = if m.contains(KeyModifiers::SHIFT) { -5 } else { -1 };
                editor_graph_smooth_adjust(app, step, 3);
            } else {
                let step = if m.contains(KeyModifiers::SHIFT) { -5 } else { -1 };
                editor_graph_adjust(app, step);
            }
        }
        _ => {}
    }
    
    Ok(false)
}

/// Handle graph mode digit input
fn handle_graph_digit_input(app: &mut App, digit: char) {
    let mut s = if app.editor_graph_input.is_empty() {
        "00".to_string()
    } else {
        app.editor_graph_input.clone()
    };
    
    if !app.editor_graph_typed {
        app.editor_graph_typed = true;
        let dval = digit.to_digit(10).unwrap_or(0);
        s = format!("{:02}", dval);
    } else {
        if s == "100" {
            let dval = digit.to_digit(10).unwrap_or(0);
            s = format!("{:02}", dval);
        } else {
            if s.len() < 2 {
                s = format!("{:0>2}", s);
            }
            let last = s.chars().nth(1).unwrap_or('0');
            let new_two = format!("{}{}", last, digit);
            if s == "10" && digit == '0' {
                s = "100".to_string();
            } else {
                s = new_two;
            }
        }
    }
    
    // Clamp and apply to selected bin
    let sel = app.editor_graph_sel.min(100);
    if s == "100" {
        app.editor_graph_input = s;
        app.editor_graph[sel] = 100;
    } else if let Ok(v) = s.parse::<u16>() {
        let v = v.min(99) as u8;
        app.editor_graph_input = format!("{:02}", v);
        app.editor_graph[sel] = v;
    }
}

/// Handle graph mode backspace
fn handle_graph_backspace(app: &mut App) {
    let mut s = if app.editor_graph_input.is_empty() {
        "00".to_string()
    } else {
        app.editor_graph_input.clone()
    };
    
    if s == "100" {
        s = "10".to_string();
    } else {
        if s.len() < 2 {
            s = format!("{:0>2}", s);
        }
        let first = s.chars().next().unwrap_or('0');
        s = format!("0{}", first);
    }
    
    if s == "00" {
        app.editor_graph_typed = false;
        app.editor_graph_input.clear();
    } else {
        app.editor_graph_input = s;
    }
    
    // Apply to selected bin
    let sel = app.editor_graph_sel.min(100);
    if app.editor_graph_input.is_empty() {
        app.editor_graph[sel] = 0;
    } else if app.editor_graph_input == "100" {
        app.editor_graph[sel] = 100;
    } else if let Ok(v) = app.editor_graph_input.parse::<u16>() {
        app.editor_graph[sel] = v.min(99) as u8;
    }
}

/// Handle groups manager specific events
fn handle_groups_manager_events(app: &mut App, code: KeyCode, _modifiers: KeyModifiers) -> anyhow::Result<bool> {
    match code {
        KeyCode::Esc => toggle_groups_manager(app),
        KeyCode::Left => app.groups_focus_right = false,
        KeyCode::Right => app.groups_focus_right = true,
        KeyCode::Up => {
            if app.groups_focus_right {
                if app.groups_pwm_idx > 0 {
                    app.groups_pwm_idx -= 1;
                }
            } else if app.group_idx > 0 {
                app.group_idx -= 1;
            }
        }
        KeyCode::Down => {
            if app.groups_focus_right {
                if app.groups_pwm_idx + 1 < app.pwms.len() {
                    app.groups_pwm_idx += 1;
                }
            } else if app.group_idx + 1 < app.groups.len() {
                app.group_idx += 1;
            }
        }
        KeyCode::Char('n') => groups_add_group(app),
        KeyCode::Char('X') => groups_delete_current(app),
        KeyCode::Char(' ') => groups_toggle_member_selected(app),
        KeyCode::Char('p') => groups_add_member_current(app),
        KeyCode::Char('x') => groups_remove_member_current(app),
        KeyCode::Char('s') => groups_save(app),
        KeyCode::Char('r') => groups_start_rename_current(app),
        KeyCode::Char('m') if app.groups_focus_right => groups_start_map_pwm(app),
        _ => return Ok(false),
    }
    Ok(false)
}

/// Handle global events (lowest priority)
fn handle_global_events(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> anyhow::Result<bool> {
    match (code, modifiers) {
        (KeyCode::Char('q'), _) => return Ok(true), // Signal to quit
        (KeyCode::Esc, _) => return Ok(true), // Signal to quit
        (KeyCode::Char('R'), _) => app.refresh(),
        (KeyCode::Char('u'), _) => app.cycle_metric(),
        (KeyCode::Tab, _) => focus_next(app),
        (KeyCode::BackTab, _) => focus_prev(app),
        (KeyCode::Up, _) => move_up(app),
        (KeyCode::Down, _) => move_down(app),
        (KeyCode::Left, _) => focus_prev(app),
        (KeyCode::Right, _) => focus_next(app),
        (KeyCode::Char('m'), _) => map_current(app),
        (KeyCode::Char('r'), _) => start_rename(app),
        (KeyCode::Char('d'), _) => delete_mapping(app),
        (KeyCode::Char('c'), _) => toggle_curve_editor(app),
        (KeyCode::Char('g'), _) => toggle_groups_manager(app),
        (KeyCode::Char('a'), _) => start_auto_detect(app),
        (KeyCode::Char('s'), _) => start_save_system_config(app),
        (KeyCode::Enter, _) => start_set_pwm(app),
        _ => {}
    }
    Ok(false)
}
