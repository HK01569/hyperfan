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

mod hwmon;
mod app;
mod config;
mod system;
mod handlers;
mod ui;
mod service;
mod ec;
mod curves;
mod logger;

use std::io::stdout;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::Terminal;

use app::App;
use config::{config_path, load_saved_config, write_system_config};
use handlers::{
    apply_auto_detect, apply_set_pwm, apply_save_system_config, cancel_save_system_config,
    confirm_auto_detect, delete_mapping, focus_next, focus_prev, map_current, move_down, move_up,
    save_curve, start_auto_detect, start_save_system_config, start_set_pwm,
    // curve editor
    toggle_curve_editor, editor_add_group, editor_prev_group, editor_next_group,
    editor_set_temp_from_current, editor_add_member_current, editor_remove_member_current,
    editor_point_prev, editor_point_next, editor_point_adjust_pwm, editor_add_point, editor_remove_point,
    editor_save_curves, editor_apply_default_curves,
    // curve editor graph mode
    editor_graph_toggle_mode, editor_graph_move_sel, editor_graph_adjust, editor_graph_commit_points,
    editor_graph_jump_to_temp, editor_graph_jump_to_next_point, editor_graph_smooth_adjust, editor_graph_apply_smoothing,
    // curve editor delay popup
    editor_start_delay_popup, editor_apply_delay_popup, editor_cancel_delay_popup,
    // curve editor hysteresis popup
    editor_start_hyst_popup, editor_apply_hyst_popup, editor_cancel_hyst_popup,
    // curve editor temperature source popup
    editor_start_temp_source_popup, editor_apply_temp_source_popup, editor_cancel_temp_source_popup, editor_temp_source_move_selection,
    // rename
    start_rename, apply_rename, revert_rename_default, cancel_rename,
    // groups
    toggle_groups_manager, groups_add_group, groups_add_member_current, groups_remove_member_current, groups_save,
    groups_delete_current, groups_toggle_member_selected, groups_apply_new_name, groups_cancel_new_name, groups_start_rename_current,
    // groups mapping popup
    groups_start_map_pwm, groups_cancel_map_pwm, groups_apply_map_pwm,
};
use ui::ui;

 

fn main() -> anyhow::Result<()> {
    // Check if running as root
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("Error: hyperfan requires root privileges to control fans and load sensor modules.");
        eprintln!("Please run with: sudo {}", std::env::args().next().unwrap_or_else(|| "hyperfan".to_string()));
        std::process::exit(1);
    }

    // Gather args once
    let args: Vec<String> = std::env::args().collect();

    // Optional logging to /etc/hyperfan/logs.json
    let logging_enabled = args.iter().any(|a| a == "--logging");
    if logging_enabled {
        logger::init_logging();
        logger::log_event("startup", serde_json::json!({
            "mode": "cli",
            "args": args,
        }));
    }

    // Simple CLI handling: `hyperfan save` writes config to /etc/hyperfan/profile.json and exits
    if args.get(1).map(|s| s.as_str()) == Some("save") {
        match load_saved_config() {
            Some(saved) => {
                write_system_config(&saved)?;
                println!("Wrote config to /etc/hyperfan/profile.json");
                return Ok(());
            }
            None => {
                eprintln!(
                    "No user config found at {}. Create mappings in the TUI first, then run: sudo hyperfan save",
                    config_path().display()
                );
                std::process::exit(1);
            }
        }
    }

    // Headless service mode: `hyperfan --service`
    if args.iter().any(|a| a == "--service") {
        // Load sensor modules before entering loop
        system::load_sensor_modules();
        if logging_enabled {
            logger::log_event("service_start", serde_json::json!({}));
        }
        return service::run_service();
    }

    // Dump EC profile: `hyperfan --dump-ec`
    if args.iter().any(|a| a == "--dump-ec") {
        // Load sensor modules to ensure chips are present
        system::load_sensor_modules();
        match ec::dump_ec_profile() {
            Ok(p) => {
                println!("Wrote EC profile to {}", p.display());
                return Ok(());
            }
            Err(e) => {
                eprintln!("dump-ec error: {}", e);
                std::process::exit(1);
            }
        }
    }

    // Auto-detect and load sensor modules
    system::load_sensor_modules();

    // Terminal init
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    if logging_enabled {
        logger::log_event("tui_start", serde_json::json!({}));
    }
    let res = run_app(&mut terminal);

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("error: {err}");
        if logging_enabled {
            logger::log_event("fatal_error", serde_json::json!({ "error": err.to_string() }));
        }
        std::process::exit(1);
    }

    Ok(())
}

fn run_app(terminal: &mut Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>) -> anyhow::Result<()> {
    let mut app = App::new();
    app.refresh();

    loop {
        // draw
        terminal.draw(|f| ui(f, &app))?;

        // tick
        let timeout = app
            .refresh_interval
            .saturating_sub(app.last_refresh.elapsed());
        if event::poll(timeout).unwrap_or(false) {
            if let Event::Key(KeyEvent { code, modifiers, .. }) = event::read()? {
                match (code, modifiers) {
                    // Disable quit while inside curve editor
                    (KeyCode::Char('q'), _) if app.show_curve_editor => { /* ignore */ }
                    (KeyCode::Char('q'), _) => return Ok(()),
                    // Groups Map PWM->FAN popup takes precedence
                    (KeyCode::Esc, _) if app.show_map_pwm_popup => { groups_cancel_map_pwm(&mut app); }
                    (KeyCode::Enter, _) if app.show_map_pwm_popup => { groups_apply_map_pwm(&mut app); }
                    (KeyCode::Up, _) if app.show_map_pwm_popup => { if app.map_fan_idx > 0 { app.map_fan_idx -= 1; } }
                    (KeyCode::Down, _) if app.show_map_pwm_popup => { if app.map_fan_idx + 1 < app.fans.len() { app.map_fan_idx += 1; } }
                    (KeyCode::Esc, _) if app.show_warning_popup => { app.show_warning_popup = false; app.warning_message.clear(); }
                    // New Group Name popup input handling
                    (KeyCode::Esc, _) if app.show_group_name_popup => { groups_cancel_new_name(&mut app); }
                    (KeyCode::Char(c), _) if app.show_group_name_popup => {
                        // Accept only letters, numbers and single '-' (no consecutive '-') with max 20 chars
                        let allowed = c.is_ascii_alphanumeric() || c == '-';
                        if allowed && app.group_name_input.len() < 20 {
                            if c == '-' {
                                if !app.group_name_input.ends_with('-') { app.group_name_input.push('-'); }
                            } else {
                                app.group_name_input.push(c);
                            }
                        }
                    }
                    (KeyCode::Backspace, _) if app.show_group_name_popup => { app.group_name_input.pop(); }
                    (KeyCode::Enter, _) if app.show_group_name_popup => { groups_apply_new_name(&mut app); }
                    (KeyCode::Esc, _) if app.show_rename_popup => { cancel_rename(&mut app); }
                    (KeyCode::Esc, _) if app.show_set_pwm_popup => { app.show_set_pwm_popup = false; app.set_pwm_input.clear(); app.set_pwm_target = None; app.set_pwm_feedback = None; app.set_pwm_typed = false; }
                    (KeyCode::Esc, _) if app.show_confirm_save_popup => { cancel_save_system_config(&mut app); }
                    (KeyCode::Esc, _) if app.show_auto_detect => {
                        // If detection is running, request graceful cancel (restores original PWMs)
                        let running = match app.auto_detect_running.lock() {
                            Ok(guard) => *guard,
                            Err(poisoned) => {
                                app.warning_message = "Auto-detect state was corrupted; attempting to cancel safely".to_string();
                                app.show_warning_popup = true;
                                *poisoned.into_inner()
                            }
                        };
                        if running { hwmon::request_cancel_autodetect(); }
                        app.show_auto_detect = false;
                        app.auto_detect_await_confirm = false;
                    },
                    // Curve editor Esc behavior
                    (KeyCode::Esc, _) if app.show_editor_save_confirm => { app.show_editor_save_confirm = false; }
                    (KeyCode::Enter, _) if app.show_editor_save_confirm => { editor_save_curves(&mut app); app.show_editor_save_confirm = false; }
                    (KeyCode::Esc, _) if app.show_curve_editor && app.editor_graph_mode => {
                        // cancel graph mode, do not commit
                        app.editor_graph_mode = false;
                        app.editor_graph_input.clear();
                        app.editor_graph_typed = false;
                    }
                    (KeyCode::Esc, _) if app.show_curve_editor => {
                        if app.editor_dirty { app.show_editor_save_confirm = true; } else { toggle_curve_editor(&mut app); }
                    },
                    (KeyCode::Esc, _) if app.show_curve_popup => app.show_curve_popup = false,
                    (KeyCode::Esc, _) if app.show_groups_manager => { toggle_groups_manager(&mut app); }
                    (KeyCode::Esc, _) => return Ok(()),
                    // Input handling for Rename popup
                    (KeyCode::Char(c), _) if app.show_rename_popup => {
                        app.rename_input.push(c);
                    }
                    (KeyCode::Backspace, _) if app.show_rename_popup => {
                        app.rename_input.pop();
                    }
                    (KeyCode::Enter, _) if app.show_rename_popup => { apply_rename(&mut app); }
                    (KeyCode::Char('d'), _) if app.show_rename_popup => { revert_rename_default(&mut app); }
                    // Input handling for Set PWM popup (two-digit overwrite, special 100)
                    (KeyCode::Char(c), _) if app.show_set_pwm_popup && c.is_ascii_digit() => {
                        let d = c;
                        // Determine current state string s shown to user
                        let mut s = if app.set_pwm_input.is_empty() { "00".to_string() } else { app.set_pwm_input.clone() };
                        if !app.set_pwm_typed {
                            // First digit typed: start fresh from 00 -> 0d
                            app.set_pwm_typed = true;
                            let dval = d.to_digit(10).unwrap_or(0);
                            s = format!("{:02}", dval);
                        } else {
                            // Already typing
                            if s == "100" {
                                // restart from 0d
                                let dval = d.to_digit(10).unwrap_or(0);
                                s = format!("{:02}", dval);
                            } else {
                                // shift last two digits and append new
                                if s.len() < 2 { s = format!("{:0>2}", s); }
                                let last = s.chars().nth(1).unwrap_or('0');
                                let new_two = format!("{}{}", last, d);
                                // special-case: 10 + 0 -> 100
                                if s == "10" && d == '0' {
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
                            if v <= 99 { app.set_pwm_input = format!("{:02}", v); }
                            else { app.set_pwm_input = "99".to_string(); }
                        }
                    }
                    (KeyCode::Backspace, _) if app.show_set_pwm_popup => {
                        // Backspace: 56 -> 05 -> 00, 100 -> 10 -> 01 -> 00
                        let mut s = if app.set_pwm_input.is_empty() { "00".to_string() } else { app.set_pwm_input.clone() };
                        if s == "100" { s = "10".to_string(); }
                        else {
                            if s.len() < 2 { s = format!("{:0>2}", s); }
                            let first = s.chars().next().unwrap_or('0');
                            s = format!("0{}", first);
                        }
                        if s == "00" { app.set_pwm_typed = false; }
                        app.set_pwm_input = if s == "00" { String::new() } else { s };
                    }
                    (KeyCode::Delete, _) if app.show_set_pwm_popup => { app.set_pwm_input.clear(); app.set_pwm_typed = false; }
                    (KeyCode::Up, m) if app.show_set_pwm_popup => {
                        let step = if m.contains(KeyModifiers::SHIFT) { 5 } else { 1 };
                        let cur = app.set_pwm_input.parse::<i16>().unwrap_or(0);
                        let mut v = cur + step;
                        if v > 100 { v = 100; }
                        if v < 0 { v = 0; }
                        app.set_pwm_input = v.to_string();
                    }
                    (KeyCode::Down, m) if app.show_set_pwm_popup => {
                        let step = if m.contains(KeyModifiers::SHIFT) { 5 } else { 1 };
                        let cur = app.set_pwm_input.parse::<i16>().unwrap_or(0);
                        let mut v = cur - step;
                        if v > 100 { v = 100; }
                        if v < 0 { v = 0; }
                        app.set_pwm_input = v.to_string();
                    }
                    (KeyCode::Char('R'), _) => app.refresh(),
                    // Cycle temperature metric (°C/°F/K) only when NOT in curve editor
                    (KeyCode::Char('u'), _) if !app.show_curve_editor => { app.cycle_metric(); },
                    (KeyCode::Tab, _) => focus_next(&mut app),
                    (KeyCode::BackTab, _) => focus_prev(&mut app),
                    // Groups page keys (before generic arrows)
                    (KeyCode::Left, _) if app.show_groups_manager => { app.groups_focus_right = false; }
                    (KeyCode::Right, _) if app.show_groups_manager => { app.groups_focus_right = true; }
                    (KeyCode::Up, _) if app.show_groups_manager => {
                        if app.groups_focus_right {
                            if app.groups_pwm_idx > 0 { app.groups_pwm_idx -= 1; }
                        } else {
                            if app.group_idx > 0 { app.group_idx -= 1; }
                        }
                    }
                    (KeyCode::Down, _) if app.show_groups_manager => {
                        if app.groups_focus_right {
                            if app.groups_pwm_idx + 1 < app.pwms.len() { app.groups_pwm_idx += 1; }
                        } else {
                            if app.group_idx + 1 < app.groups.len() { app.group_idx += 1; }
                        }
                    }
                    (KeyCode::Char('n'), _) if app.show_groups_manager => { groups_add_group(&mut app); }
                    (KeyCode::Char('X'), _) if app.show_groups_manager => { groups_delete_current(&mut app); }
                    (KeyCode::Char(' '), _) if app.show_groups_manager => { groups_toggle_member_selected(&mut app); }
                    (KeyCode::Char('p'), _) if app.show_groups_manager => { groups_add_member_current(&mut app); }
                    (KeyCode::Char('x'), _) if app.show_groups_manager => { groups_remove_member_current(&mut app); }
                    (KeyCode::Char('s'), _) if app.show_groups_manager => { groups_save(&mut app); }
                    // Start mapping selected PWM (right panel) to a FAN
                    (KeyCode::Char('m'), _) if app.show_groups_manager && app.groups_focus_right => { groups_start_map_pwm(&mut app); }
                    // Delay popup input (takes precedence)
                    (KeyCode::Enter, _) if app.show_curve_delay_popup => { editor_apply_delay_popup(&mut app); }
                    (KeyCode::Esc, _) if app.show_curve_delay_popup => { editor_cancel_delay_popup(&mut app); }
                    (KeyCode::Backspace, _) if app.show_curve_delay_popup => { app.curve_delay_input.pop(); }
                    (KeyCode::Char(c), _) if app.show_curve_delay_popup && c.is_ascii_digit() => { app.curve_delay_input.push(c); }

                    // Hysteresis popup input (takes precedence)
                    (KeyCode::Esc, _) if app.show_curve_hyst_popup => { editor_cancel_hyst_popup(&mut app); }
                    (KeyCode::Enter, _) if app.show_curve_hyst_popup => { editor_apply_hyst_popup(&mut app); }
                    // Temperature source popup handling
                    (KeyCode::Esc, _) if app.show_temp_source_popup => { editor_cancel_temp_source_popup(&mut app); }
                    (KeyCode::Enter, _) if app.show_temp_source_popup => { editor_apply_temp_source_popup(&mut app); }
                    (KeyCode::Up, _) if app.show_temp_source_popup => { editor_temp_source_move_selection(&mut app, -1); }
                    (KeyCode::Down, _) if app.show_temp_source_popup => { editor_temp_source_move_selection(&mut app, 1); }
                    (KeyCode::Backspace, _) if app.show_curve_hyst_popup => { app.curve_hyst_input.pop(); }
                    (KeyCode::Char(c), _) if app.show_curve_hyst_popup && c.is_ascii_digit() => { app.curve_hyst_input.push(c); }
                    
                    // Editor graph-mode keys (take precedence)
                    (KeyCode::Char('u'), _) if app.show_curve_editor => { editor_graph_toggle_mode(&mut app); }
                    (KeyCode::Left, m) if app.show_curve_editor && app.editor_graph_mode => {
                        let step = if m.contains(KeyModifiers::SHIFT) { -5 } else { -1 };
                        editor_graph_move_sel(&mut app, step);
                    }
                    (KeyCode::Right, m) if app.show_curve_editor && app.editor_graph_mode => {
                        let step = if m.contains(KeyModifiers::SHIFT) { 5 } else { 1 };
                        editor_graph_move_sel(&mut app, step);
                    }
                    // Enhanced navigation: jump to temperature points
                    (KeyCode::Home, _) if app.show_curve_editor && app.editor_graph_mode => {
                        editor_graph_jump_to_temp(&mut app, 0);
                    }
                    (KeyCode::End, _) if app.show_curve_editor && app.editor_graph_mode => {
                        editor_graph_jump_to_temp(&mut app, 100);
                    }
                    (KeyCode::PageUp, _) if app.show_curve_editor && app.editor_graph_mode => {
                        editor_graph_jump_to_next_point(&mut app, false);
                    }
                    (KeyCode::PageDown, _) if app.show_curve_editor && app.editor_graph_mode => {
                        editor_graph_jump_to_next_point(&mut app, true);
                    }
                    // Quick jump to common temperature points
                    (KeyCode::Char('1'), m) if app.show_curve_editor && app.editor_graph_mode && m.contains(KeyModifiers::CONTROL) => {
                        editor_graph_jump_to_temp(&mut app, 25);
                    }
                    (KeyCode::Char('2'), m) if app.show_curve_editor && app.editor_graph_mode && m.contains(KeyModifiers::CONTROL) => {
                        editor_graph_jump_to_temp(&mut app, 50);
                    }
                    (KeyCode::Char('3'), m) if app.show_curve_editor && app.editor_graph_mode && m.contains(KeyModifiers::CONTROL) => {
                        editor_graph_jump_to_temp(&mut app, 75);
                    }
                    // Graph mode: numeric typing (0-100) to set selected column
                    (KeyCode::Char(c), _) if app.show_curve_editor && app.editor_graph_mode && c.is_ascii_digit() => {
                        let d = c;
                        // Work on local string s representing the input buffer
                        let mut s = if app.editor_graph_input.is_empty() { "00".to_string() } else { app.editor_graph_input.clone() };
                        if !app.editor_graph_typed {
                            app.editor_graph_typed = true;
                            let dval = d.to_digit(10).unwrap_or(0);
                            s = format!("{:02}", dval);
                        } else {
                            if s == "100" {
                                let dval = d.to_digit(10).unwrap_or(0);
                                s = format!("{:02}", dval);
                            } else {
                                if s.len() < 2 { s = format!("{:0>2}", s); }
                                let last = s.chars().nth(1).unwrap_or('0');
                                let new_two = format!("{}{}", last, d);
                                if s == "10" && d == '0' { s = "100".to_string(); } else { s = new_two; }
                            }
                        }
                        // Clamp and apply to selected bin
                        let sel = app.editor_graph_sel.min(100);
                        if s == "100" { app.editor_graph_input = s; app.editor_graph[sel] = 100; }
                        else if let Ok(v) = s.parse::<u16>() { let v = v.min(99) as u8; app.editor_graph_input = format!("{:02}", v); app.editor_graph[sel] = v; }
                    }
                    (KeyCode::Backspace, _) if app.show_curve_editor && app.editor_graph_mode => {
                        // Backspace: 56 -> 05 -> 00, 100 -> 10 -> 01 -> 00
                        let mut s = if app.editor_graph_input.is_empty() { "00".to_string() } else { app.editor_graph_input.clone() };
                        if s == "100" { s = "10".to_string(); }
                        else {
                            if s.len() < 2 { s = format!("{:0>2}", s); }
                            let first = s.chars().next().unwrap_or('0');
                            s = format!("0{}", first);
                        }
                        if s == "00" { app.editor_graph_typed = false; app.editor_graph_input.clear(); } else { app.editor_graph_input = s; }
                        // Apply to selected bin
                        let sel = app.editor_graph_sel.min(100);
                        if app.editor_graph_input.is_empty() { app.editor_graph[sel] = 0; }
                        else if app.editor_graph_input == "100" { app.editor_graph[sel] = 100; }
                        else if let Ok(v) = app.editor_graph_input.parse::<u16>() { app.editor_graph[sel] = v.min(99) as u8; }
                    }
                    (KeyCode::Delete, _) if app.show_curve_editor && app.editor_graph_mode => {
                        app.editor_graph_input.clear();
                        app.editor_graph_typed = false;
                        let sel = app.editor_graph_sel.min(100);
                        app.editor_graph[sel] = 0;
                    }
                    (KeyCode::Up, m) if app.show_curve_editor && app.editor_graph_mode => {
                        if m.contains(KeyModifiers::CONTROL) {
                            // Smooth adjustment with falloff
                            let step = if m.contains(KeyModifiers::SHIFT) { 5 } else { 1 };
                            editor_graph_smooth_adjust(&mut app, step, 3);
                        } else {
                            let step = if m.contains(KeyModifiers::SHIFT) { 5 } else { 1 };
                            editor_graph_adjust(&mut app, step);
                        }
                    }
                    (KeyCode::Down, m) if app.show_curve_editor && app.editor_graph_mode => {
                        if m.contains(KeyModifiers::CONTROL) {
                            // Smooth adjustment with falloff
                            let step = if m.contains(KeyModifiers::SHIFT) { -5 } else { -1 };
                            editor_graph_smooth_adjust(&mut app, step, 3);
                        } else {
                            let step = if m.contains(KeyModifiers::SHIFT) { -5 } else { -1 };
                            editor_graph_adjust(&mut app, step);
                        }
                    }
                    // Apply smoothing filter
                    (KeyCode::Char('m'), _) if app.show_curve_editor && app.editor_graph_mode => {
                        editor_graph_apply_smoothing(&mut app);
                    }
                    (KeyCode::Enter, _) if app.show_curve_editor && app.editor_graph_mode => { editor_graph_commit_points(&mut app); }
                    // Curve editor: start apply-delay popup (hysteresis time in ms)
                    (KeyCode::Char('h'), _) if app.show_curve_editor => { editor_start_delay_popup(&mut app); }
                    // Curve editor: start hysteresis percent popup
                    (KeyCode::Char('y'), _) if app.show_curve_editor => { editor_start_hyst_popup(&mut app); }
                    // Curve editor: start temperature source selection popup
                    (KeyCode::Char('t'), _) if app.show_curve_editor => { editor_start_temp_source_popup(&mut app); }
                    // Editor-only keys should be matched before generic arrows
                    // In graph mode, Left/Right move selection (handled above). In non-graph, use Left/Right to switch panel focus.
                    (KeyCode::Left, _) if app.show_curve_editor && !app.editor_graph_mode && !app.show_curve_delay_popup => { app.editor_focus_right = false; }
                    (KeyCode::Right, _) if app.show_curve_editor && !app.editor_graph_mode && !app.show_curve_delay_popup => { app.editor_focus_right = true; }
                    // Curve editor: Up/Down move selection in groups list when left panel focused, following alphabetical display order
                    (KeyCode::Up, _) if app.show_curve_editor && !app.editor_graph_mode && !app.show_curve_delay_popup && !app.editor_focus_right => {
                        if app.control_idx > 0 { app.control_idx -= 1; }
                    }
                    (KeyCode::Down, _) if app.show_curve_editor && !app.editor_graph_mode && !app.show_curve_delay_popup && !app.editor_focus_right => {
                        if app.control_idx + 1 < app.mappings.len() { app.control_idx += 1; }
                    }
                    // Generic arrows
                    (KeyCode::Up, _) => move_up(&mut app),
                    (KeyCode::Down, _) => move_down(&mut app),
                    (KeyCode::Char('m'), _) => map_current(&mut app),
                    (KeyCode::Char('r'), _) if app.show_groups_manager => { groups_start_rename_current(&mut app); }
                    (KeyCode::Char('r'), _) => start_rename(&mut app),
                    (KeyCode::Char('d'), _) if !app.show_curve_editor => delete_mapping(&mut app),
                    // Open curve editor page
                    (KeyCode::Char('c'), _) => toggle_curve_editor(&mut app),
                    // Open groups manager
                    (KeyCode::Char('g'), _) => toggle_groups_manager(&mut app),
                    // Generic arrows
                    (KeyCode::Left, _) => focus_prev(&mut app),
                    (KeyCode::Right, _) => focus_next(&mut app),
                    (KeyCode::Char('n'), _) if app.show_curve_editor => { editor_add_group(&mut app); }
                    (KeyCode::Char('t'), _) if app.show_curve_editor => { editor_set_temp_from_current(&mut app); }
                    (KeyCode::Char('p'), _) if app.show_curve_editor => { editor_add_member_current(&mut app); }
                    (KeyCode::Char('x'), _) if app.show_curve_editor => { editor_remove_member_current(&mut app); }
                    (KeyCode::Char('['), _) if app.show_curve_editor => { editor_point_prev(&mut app); }
                    (KeyCode::Char(']'), _) if app.show_curve_editor => { editor_point_next(&mut app); }
                    (KeyCode::Char('+'), _) if app.show_curve_editor => { editor_point_adjust_pwm(&mut app, 1); }
                    (KeyCode::Char('='), _) if app.show_curve_editor => { editor_point_adjust_pwm(&mut app, 1); }
                    (KeyCode::Char('-'), _) if app.show_curve_editor => { editor_point_adjust_pwm(&mut app, -1); }
                    (KeyCode::Char('a'), _) if app.show_curve_editor => { editor_add_point(&mut app); }
                    (KeyCode::Backspace, _) if app.show_curve_editor => { editor_remove_point(&mut app); }
                    (KeyCode::Char('s'), _) if app.show_curve_editor => { app.show_editor_save_confirm = true; }
                    (KeyCode::Char('d'), _) if app.show_curve_editor => { editor_apply_default_curves(&mut app); }
                    
                    (KeyCode::Up, _) if app.show_curve_editor && !app.editor_focus_right => {
                        if app.control_idx > 0 { app.control_idx -= 1; }
                    }
                    (KeyCode::Down, _) if app.show_curve_editor && !app.editor_focus_right => {
                        if app.control_idx + 1 < app.mappings.len() { app.control_idx += 1; }
                    }
                    (KeyCode::Esc, _) if app.show_curve_editor => { app.show_curve_editor = false; }
                    (KeyCode::Char('q'), _) if app.show_curve_editor => { app.show_curve_editor = false; }
                    (KeyCode::Enter, _) if app.show_editor_save_confirm => { editor_save_curves(&mut app); app.show_editor_save_confirm = false; }

                    (KeyCode::Char('a'), _) => start_auto_detect(&mut app),
                    (KeyCode::Char('s'), _) => start_save_system_config(&mut app),
                    (KeyCode::Enter, _) if app.show_warning_popup => { app.show_warning_popup = false; app.warning_message.clear(); }
                    (KeyCode::Enter, _) if app.show_curve_popup => save_curve(&mut app),
                    (KeyCode::Enter, _) if app.show_auto_detect && app.auto_detect_await_confirm => { confirm_auto_detect(&mut app); },
                    (KeyCode::Enter, _) if app.show_auto_detect => {
// ... (rest of the code remains the same)
                        let is_running = match app.auto_detect_running.lock() {
                            Ok(guard) => *guard,
                            Err(poisoned) => {
                                app.warning_message = "Auto-detect state was corrupted; proceeding with recovered state".to_string();
                                app.show_warning_popup = true;
                                *poisoned.into_inner()
                            }
                        };
                        if !is_running { apply_auto_detect(&mut app); }
                    },
                    (KeyCode::Enter, _) if app.show_confirm_save_popup => apply_save_system_config(&mut app),
                    (KeyCode::Enter, _) if app.show_set_pwm_popup => apply_set_pwm(&mut app),
                    (KeyCode::Enter, _) => start_set_pwm(&mut app),
                    _ => {}
                }
            }
        }

        if app.last_refresh.elapsed() >= app.refresh_interval {
            app.refresh();
        }
    }
}
// system helpers moved to `src/system.rs`
