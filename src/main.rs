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
mod events;

use std::io::stdout;

use crossterm::event::{self, Event};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::Terminal;

use app::App;
use config::{config_path, load_saved_config, write_system_config};
use events::handle_key_event;
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
            if let Event::Key(key_event) = event::read()? {
                // Use the new modular event handler
                if handle_key_event(&mut app, key_event)? {
                    return Ok(());
                }
            }
        }

        if app.last_refresh.elapsed() >= app.refresh_interval {
            app.refresh();
            
            // Apply curves continuously if curve data exists
            if !app.editor_groups.is_empty() {
                handlers::apply_curves_to_hardware(&mut app);
            }
        }

        // Check if auto-detect has completed and transition to confirmation state
        if app.show_auto_detect && !app.auto_detect_await_confirm {
            let is_running = app.auto_detect_running.lock()
                .map(|g| *g)
                .unwrap_or(false);
            
            // If auto-detect was running but is now stopped, transition to confirmation
            if !is_running {
                let has_results = app.auto_detect_results.lock()
                    .map(|g| !g.is_empty())
                    .unwrap_or(false);
                
                // Only transition if we have results or if detection actually ran
                if has_results || app.auto_detect_percent > 0.0 {
                    app.auto_detect_await_confirm = true;
                }
            }
        }
    }
}
// system helpers moved to `src/system.rs`
