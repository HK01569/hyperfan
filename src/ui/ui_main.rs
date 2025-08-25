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

use crate::app::{App, Focus};
use std::collections::HashMap;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};

/// Render the main view (default view with fans, pwms, temps columns)
pub fn render_main_view(f: &mut Frame, app: &App, size: Rect) {
    // Layout: header | columns | control | status
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Min(8),
            Constraint::Length(1),
        ])
        .split(size);

    // Header
    render_header(f, app, chunks[0]);
    
    // Three columns
    render_columns(f, app, chunks[1]);
    
    // Control block
    render_control_block(f, app, chunks[2]);
    
    // Status bar
    render_status_bar(f, app, chunks[3]);
}

fn render_header(f: &mut Frame, app: &App, area: Rect) {
    let header_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(80), Constraint::Percentage(20)])
        .split(area);

    let header_text = format!(
        " CPU: {}    |    Motherboard: {}    |    hwmon chips: {} ",
        if app.cpu_name.is_empty() { "?" } else { &app.cpu_name },
        if app.mb_name.is_empty() { "?" } else { &app.mb_name },
        app.readings.len()
    );
    let header = Paragraph::new(header_text).style(Style::default().fg(Color::Yellow));
    f.render_widget(header, header_cols[0]);

    let metric_label = match app.metric {
        crate::config::Metric::C => "Metric: °C",
        crate::config::Metric::F => "Metric: °F",
        crate::config::Metric::K => "Metric: K"
    };
    let metric_widget = Paragraph::new(metric_label)
        .alignment(Alignment::Right)
        .style(Style::default().fg(Color::Gray));
    f.render_widget(metric_widget, header_cols[1]);
}

fn render_columns(f: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(34),
            Constraint::Percentage(33),
        ])
        .split(area);

    render_fans_column(f, app, cols[0]);
    render_pwms_column(f, app, cols[1]);
    render_temps_column(f, app, cols[2]);
}

fn render_fans_column(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" FANS ({}) ", app.fans.len()))
        .border_style(if app.focus == Focus::Fans {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        });

    let header_style = Style::default().fg(Color::Gray).add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    let highlight = Style::default().bg(Color::Blue).fg(Color::White);

    let mut items: Vec<ListItem> = Vec::with_capacity(app.fans.len() + 1);
    items.push(ListItem::new(format!("{:<40} {:>6}", "Name", "RPM")).style(header_style));
    items.extend(app.fans.iter().map(|(name, rpm)| {
        let disp = app.fan_aliases.get(name).cloned().unwrap_or_else(|| name.clone());
        ListItem::new(format!("{:<40} {:>6} RPM", disp, rpm))
    }));

    let mut state = ListState::default();
    if !app.fans.is_empty() {
        state.select(Some(app.fans_idx + 1));
    }

    let list = List::new(items).block(block).highlight_style(highlight);
    f.render_stateful_widget(list, area, &mut state);
}

fn render_pwms_column(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" PWM ({}) ", app.pwms.len()))
        .border_style(if app.focus == Focus::Pwms {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        });

    let header_style = Style::default().fg(Color::Gray).add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    let highlight = Style::default().bg(Color::Blue).fg(Color::White);

    let mut items: Vec<ListItem> = Vec::with_capacity(app.pwms.len() + 1);
    items.push(ListItem::new(format!("{:<40} {:>6}", "Name", "%")).style(header_style));
    items.extend(app.pwms.iter().map(|(name, val)| {
        let pct = ((*val as f64) * 100.0 / 255.0).round() as u64;
        let disp = app.pwm_aliases.get(name).cloned().unwrap_or_else(|| name.clone());
        ListItem::new(format!("{:<40} {:>5}%", disp, pct))
    }));

    let mut state = ListState::default();
    if !app.pwms.is_empty() {
        state.select(Some(app.pwms_idx + 1));
    }

    let list = List::new(items).block(block).highlight_style(highlight);
    f.render_stateful_widget(list, area, &mut state);
}

fn render_temps_column(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" TEMP ({}) ", app.temps.len()))
        .border_style(if app.focus == Focus::Temps {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        });

    let header_style = Style::default().fg(Color::Gray).add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    let highlight = Style::default().bg(Color::Blue).fg(Color::White);

    let unit = match app.metric {
        crate::config::Metric::C => "°C",
        crate::config::Metric::F => "°F",
        crate::config::Metric::K => "K"
    };

    let mut items: Vec<ListItem> = Vec::with_capacity(app.temps.len() + 1);
    items.push(ListItem::new(format!("{:<40} {:>7}", "Name", unit)).style(header_style));
    items.extend(app.temps.iter().map(|(name, c)| {
        let (val, unit_str) = app.convert_temp(*c);
        let disp = app.temp_aliases.get(name).cloned().unwrap_or_else(|| name.clone());
        ListItem::new(format!("{:<40} {:>5.1} {}", disp, val, unit_str))
    }));

    let mut state = ListState::default();
    if !app.temps.is_empty() {
        state.select(Some(app.temps_idx + 1));
    }

    let list = List::new(items).block(block).highlight_style(highlight);
    f.render_stateful_widget(list, area, &mut state);
}

fn render_control_block(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" CONTROL (FAN -> PWM) ")
        .border_style(if app.focus == Focus::Control {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        });

    if app.mappings.is_empty() {
        let text = "(no mappings) Press 'm' to add mapping from current selections.";
        let paragraph = Paragraph::new(text)
            .block(block)
            .style(Style::default().fg(Color::Gray));
        f.render_widget(paragraph, area);
        return;
    }

    // Build sorted CONTROL mappings by fan display name
    let mut mapped: Vec<(usize, String, String, String, String)> = app.mappings
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let fan_key = m.fan.clone();
            let pwm_key = m.pwm.clone();
            let fan_disp = app.fan_aliases.get(&m.fan).cloned().unwrap_or_else(|| m.fan.clone());
            let pwm_disp = app.pwm_aliases.get(&m.pwm).cloned().unwrap_or_else(|| m.pwm.clone());
            (i, fan_key, pwm_key, fan_disp, pwm_disp)
        })
        .collect();

    // Count duplicates for display names
    let mut fan_counts: HashMap<String, usize> = HashMap::new();
    let mut pwm_counts: HashMap<String, usize> = HashMap::new();
    for (_i, _fk, _pk, fdisp, pdisp) in mapped.iter() {
        *fan_counts.entry(fdisp.clone()).or_insert(0) += 1;
        *pwm_counts.entry(pdisp.clone()).or_insert(0) += 1;
    }

    // Sort by fan display name
    mapped.sort_by(|a, b| a.3.cmp(&b.3));

    let chip_from_key = |key: &str| -> String {
        key.split(':').next().unwrap_or(key).to_string()
    };

    let mut control_items: Vec<Line> = Vec::new();
    for (orig_i, fan_key, pwm_key, fan_disp, pwm_disp) in mapped.into_iter() {
        let marker = if app.focus == Focus::Control && orig_i == app.control_idx {
            "> "
        } else {
            "  "
        };
        
        let fan_rpm = app.fans
            .iter()
            .find(|(name, _)| name == &app.mappings[orig_i].fan)
            .map(|(_, rpm)| *rpm)
            .unwrap_or(0);
        
        let pwm_raw = app.pwms
            .iter()
            .find(|(name, _)| name == &app.mappings[orig_i].pwm)
            .map(|(_, v)| *v)
            .unwrap_or(0);
        let pwm_pct = ((pwm_raw as f64) * 100.0 / 255.0).round() as u64;

        let fan_show = if fan_counts.get(&fan_disp).copied().unwrap_or(0) > 1 {
            format!("{} [{}]", fan_disp, chip_from_key(&fan_key))
        } else {
            fan_disp
        };
        let pwm_show = if pwm_counts.get(&pwm_disp).copied().unwrap_or(0) > 1 {
            format!("{} [{}]", pwm_disp, chip_from_key(&pwm_key))
        } else {
            pwm_disp
        };

        let line = format!(
            "{}{} -> {}   |  RPM: {}   PWM: {}%",
            marker, fan_show, pwm_show, fan_rpm, pwm_pct
        );
        control_items.push(Line::from(line));
    }

    let control_list = Paragraph::new(control_items)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(control_list, area);
}

fn render_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let mut status_text = app.status.clone();
    
    // Add curve status indicator if curves are active
    if !app.editor_groups.is_empty() {
        let curve_count = app.editor_groups.len();
        let curve_status = format!(" | CURVES ACTIVE: {} group(s)", curve_count);
        status_text.push_str(&curve_status);
    }
    
    let status = Paragraph::new(status_text.as_str())
        .style(Style::default().fg(Color::Gray));
    f.render_widget(status, area);
}
