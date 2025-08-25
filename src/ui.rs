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
use crate::hwmon;
use crate::curves; // for interp_pwm_percent in read-only plot
use std::collections::HashMap;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, BorderType, Clear, Gauge, List, ListItem, ListState, Paragraph, Wrap};

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

pub fn ui(f: &mut Frame, app: &App) {
    let size = f.area();

    // Groups manager page
    if app.show_groups_manager {
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(size);

        // Left: groups list
        let mut groups_block = Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" PWM Groups ");
        groups_block = if app.groups_focus_right { groups_block } else { groups_block.border_style(Style::default().fg(Color::Cyan)) };
        let mut items: Vec<ListItem> = Vec::new();
        if app.groups.is_empty() {
            items.push(ListItem::new("(no groups) press 'n' to create"));
        } else {
            for (i, g) in app.groups.iter().enumerate() {
                let sel = if i == app.group_idx { "> " } else { "  " };
                items.push(ListItem::new(format!("{}{}  [{} member(s)]", sel, g.name, g.members.len())));
            }

        // Map PWM -> FAN popup overlay
        if app.show_map_pwm_popup {
            let area = centered_rect(60, 60, size);
            let block = Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" Map PWM to FAN ");
            let inner = block.inner(area);
            f.render_widget(Clear, area);
            f.render_widget(block, area);

            // Header with selected PWM
            let header_text = if let Some((pwm_name, _)) = app.pwms.get(app.groups_pwm_idx) {
                let disp_pwm = app.pwm_aliases.get(pwm_name).cloned().unwrap_or_else(|| pwm_name.clone());
                format!("Select a FAN to control with PWM: {}", disp_pwm)
            } else {
                "Select a FAN to control with the selected PWM".to_string()
            };

            // Build fan list with selection
            let mut items: Vec<ListItem> = Vec::new();
            for (i, (name, rpm)) in app.fans.iter().enumerate() {
                let disp = app.fan_aliases.get(name).cloned().unwrap_or_else(|| name.clone());
                let sel = if i == app.map_fan_idx { "> " } else { "  " };
                items.push(ListItem::new(format!("{}{: <40} {:>6} RPM", sel, disp, rpm)));
            }
            if app.fans.is_empty() { items.push(ListItem::new("(no fans detected)")); }

            // Split inner area into list and footer
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(2), Constraint::Min(5), Constraint::Length(2)])
                .split(inner);
            // Header
            let header = Paragraph::new(header_text).alignment(Alignment::Center);
            f.render_widget(header, chunks[0]);

            let mut state = ListState::default();
            if !app.fans.is_empty() { state.select(Some(app.map_fan_idx.min(app.fans.len().saturating_sub(1)))); }
            let list = List::new(items);
            f.render_stateful_widget(list, chunks[1], &mut state);

            let help = Paragraph::new("↑/↓ select FAN  |  Enter apply  |  Esc cancel").alignment(Alignment::Center).style(Style::default().fg(Color::Gray));
            f.render_widget(help, chunks[2]);
        }
        }
        let list = List::new(items.clone()).block(groups_block);
        // In graph mode, dim the groups list to de-emphasize
        if app.editor_graph_mode {
            let area = layout[0];
            let dim_block = Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" Curve Groups ").border_style(Style::default().fg(Color::DarkGray));
            let inner = dim_block.inner(area);
            f.render_widget(dim_block, area);
            let dim_list = List::new(items).style(Style::default().fg(Color::DarkGray));
            f.render_widget(dim_list, inner);
        } else {
            f.render_widget(list, layout[0]);
        }

        // Right side split: top = Group Members, bottom = CONTROL pairs
        let right_split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
            .split(layout[1]);

        // Top: PWM selection list with membership markers
        let mut members_block = Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" Group Members (toggle with Space) ");
        members_block = if app.groups_focus_right { members_block.border_style(Style::default().fg(Color::Cyan)) } else { members_block };
        let members_inner = members_block.inner(right_split[0]);
        f.render_widget(members_block, right_split[0]);

        let mut member_items: Vec<ListItem> = Vec::new();
        if app.groups.is_empty() {
            member_items.push(ListItem::new("No groups. Press 'n' to create."));
        } else {
            let g = &app.groups[app.group_idx];
            for (i, (name, _val)) in app.pwms.iter().enumerate() {
                let in_group = g.members.iter().any(|m| m == name);
                let marker = if in_group { "[x]" } else { "[ ]" };
                let disp = app.pwm_aliases.get(name).cloned().unwrap_or_else(|| name.clone());
                let sel = if i == app.groups_pwm_idx { "> " } else { "  " };
                member_items.push(ListItem::new(format!("{}{} {}", sel, marker, disp)));
            }
            if app.pwms.is_empty() { member_items.push(ListItem::new("(no PWM controllers detected)")); }
        }
        let members_list = List::new(member_items);
        let mut members_state = ListState::default();
        if !app.pwms.is_empty() { members_state.select(Some(app.groups_pwm_idx.min(app.pwms.len().saturating_sub(1)))); }
        f.render_stateful_widget(members_list, members_inner, &mut members_state);

        // Bottom: CONTROL pairs listing (fan -> pwm)
        let control_block = Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" CONTROL Pairs ");
        let control_inner = control_block.inner(right_split[1]);
        f.render_widget(control_block, right_split[1]);

        let mut control_items: Vec<ListItem> = Vec::new();
        if app.mappings.is_empty() {
            control_items.push(ListItem::new("(no control mappings yet)"));
        } else {
            // Build tuples with original keys and display names
            let mut pairs: Vec<(String, String, String, String)> = app
                .mappings
                .iter()
                .map(|m| {
                    let fan_key = m.fan.clone();
                    let pwm_key = m.pwm.clone();
                    let fan_disp = app
                        .fan_aliases
                        .get(&m.fan)
                        .cloned()
                        .unwrap_or_else(|| m.fan.clone());
                    let pwm_disp = app
                        .pwm_aliases
                        .get(&m.pwm)
                        .cloned()
                        .unwrap_or_else(|| m.pwm.clone());
                    (fan_key, pwm_key, fan_disp, pwm_disp)
                })
                .collect();

            // Count duplicates by display name
            let mut fan_counts: HashMap<String, usize> = HashMap::new();
            let mut pwm_counts: HashMap<String, usize> = HashMap::new();
            for (_fk, _pk, fdisp, pdisp) in pairs.iter() {
                *fan_counts.entry(fdisp.clone()).or_insert(0) += 1;
                *pwm_counts.entry(pdisp.clone()).or_insert(0) += 1;
            }

            // Sort alphabetically by fan display name
            pairs.sort_by(|a, b| a.2.cmp(&b.2));

            let chip_from_key = |key: &str| -> String {
                key.split(':').next().unwrap_or(key).to_string()
            };

            for (fan_key, pwm_key, fan_disp, pwm_disp) in pairs.into_iter() {
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
                control_items.push(ListItem::new(format!("{}  ->  {}", fan_show, pwm_show)));
            }
        }
        let control_list = List::new(control_items);
        f.render_widget(control_list, control_inner);

        // New/Rename Group Name popup overlay
        if app.show_group_name_popup {
            let area = centered_rect(50, 30, size);
            let title = if app.group_rename_mode { " Rename Group " } else { " New Group Name " };
            let block = Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(title);
            let inner = block.inner(area);
            f.render_widget(Clear, area);
            f.render_widget(block, area);
            let mut lines = Vec::new();
            lines.push(Line::from("Enter a name for the group (max 20 chars)."));
            lines.push(Line::from("Allowed: letters, numbers, and '-' (no consecutive dashes)"));
            lines.push(Line::from(""));
            // Top inner padding
            lines.push(Line::from(""));
            lines.push(Line::from(format!("Name: {}", app.group_name_input)));
            lines.push(Line::from(""));
            if app.group_rename_mode {
                lines.push(Line::from("Enter: rename  |  Esc: cancel"));
            } else {
                lines.push(Line::from("Enter: create  |  Esc: cancel"));
            }
            let p = Paragraph::new(lines).alignment(Alignment::Left).wrap(Wrap { trim: false });
            f.render_widget(p, inner);
        }

        // Bottom status line with controls/help for Groups page
        let help = "Groups: ←/→ focus | ↑/↓ navigate | Space toggle member | n new | r rename | X delete group | s save | Esc exit";
        let status = Paragraph::new(help).style(Style::default().fg(Color::Gray));
        let bottom = Rect { x: size.x, y: size.y + size.height.saturating_sub(1), width: size.width, height: 1 };
        f.render_widget(status, bottom);

        return;
    }

    // Dedicated Curve Editor page
    if app.show_curve_editor {
        // Horizontal split: left = CONTROL pairs, right = Graph
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(size);

        // Left: CONTROL pairs list
        let mut left_block = Block::default()
            .borders(Borders::ALL)
            .title(" CONTROL Pairs ");
        if !app.editor_focus_right { left_block = left_block.border_style(Style::default().fg(Color::Cyan)); }

        let mut items: Vec<ListItem> = Vec::new();
        if app.mappings.is_empty() {
            items.push(ListItem::new("(no control mappings yet)"));
        } else {
            for (i, m) in app.mappings.iter().enumerate() {
                let fan_disp = app.fan_aliases.get(&m.fan).cloned().unwrap_or_else(|| m.fan.clone());
                let pwm_disp = app.pwm_aliases.get(&m.pwm).cloned().unwrap_or_else(|| m.pwm.clone());
                let marker = if i == app.control_idx { "> " } else { "  " };
                items.push(ListItem::new(format!("{}{}  -  {}", marker, fan_disp, pwm_disp)));
            }
        }
        let mut state = ListState::default();
        if !app.mappings.is_empty() { state.select(Some(app.control_idx.min(app.mappings.len().saturating_sub(1)))); }
        let highlight = Style::default().bg(Color::Blue).fg(Color::White);
        let list = List::new(items).block(left_block).highlight_style(highlight);
        f.render_stateful_widget(list, layout[0], &mut state);

        // Right: Graph area (editable when editor_graph_mode)
        let mut right_block = Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" Graph ");
        if app.editor_focus_right { right_block = right_block.border_style(Style::default().fg(Color::Cyan)); }
        let inner = right_block.inner(layout[1]);
        f.render_widget(right_block, layout[1]);

        if app.editor_graph_mode {
            // Split right panel: header | canvas | footer
            let vchunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Min(5), Constraint::Length(4)])
                .split(inner);

            // Header info
            let mut header_lines: Vec<Line> = Vec::new();
            if let Some(g) = app.editor_groups.get(app.editor_group_idx) {
                header_lines.push(Line::from(format!("Name: {}  |  Temp: {}", g.name, g.temp_source)));
            } else {
                header_lines.push(Line::from("No curve defined yet"));
            }
            header_lines.push(Line::from("←/→ move (Shift=±5)  |  ↑/↓ adjust (Ctrl=smooth)  |  Home/End jump  |  PgUp/PgDn smart jump  |  Ctrl+1/2/3 quick temps"));
            header_lines.push(Line::from("0-9 type value  |  m smooth curve  |  Enter commit  |  Esc cancel  |  s save  |  d default  |  h delay  |  y hysteresis  |  t temp source"));
            let header_p = Paragraph::new(header_lines).wrap(Wrap { trim: false });
            f.render_widget(header_p, vchunks[0]);

            // Bordered canvas area
            let canvas_block = Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" Curve ");
            let canvas_inner = canvas_block.inner(vchunks[1]);
            f.render_widget(canvas_block, vchunks[1]);

            // Graph content inside the canvas
            let mut graph_lines: Vec<Line> = Vec::new();
            if let Some(g) = app.editor_groups.get(app.editor_group_idx) {
                // Determine graph area using the canvas inner rect
                let total_h = canvas_inner.height as usize;
                let total_w = canvas_inner.width as usize;

                // Layout inside the canvas:
                //  - 1 line top padding
                //  - graph_rows lines of bars
                //  - 1 line x-axis baseline
                //  - 1 line x-axis tick labels
                let graph_rows = total_h.saturating_sub(3).max(5);

                // Dense layout: no left Y-axis labels to maximize bar columns
                let y_label_w = 0usize;
                // Drop inner padding if width is tight; otherwise keep a small 1-col pad
                let mut inner_pad = if total_w >= 105 { 1usize } else { 0usize };
                let mut content_w = total_w.saturating_sub(y_label_w + inner_pad * 2);
                // If we can fit 101, force exactly 101 columns for 1°C resolution
                let cols = if content_w >= 101 { 101 } else { content_w.min(101).max(10) };
                // If padding squeezed us below 101 but we still have spare width, relax padding
                if cols < 101 && inner_pad > 0 && total_w.saturating_sub(y_label_w) >= 101 {
                    inner_pad = 0;
                    content_w = total_w.saturating_sub(y_label_w + inner_pad * 2);
                }

                // Top inner padding
                graph_lines.push(Line::from(""));

                // Build graph rows (top -> bottom)
                let _row_step = 100.0 / graph_rows as f32;
                let sel_t = app.editor_graph_sel.min(100);
                let sel_pwm = app.editor_graph[sel_t] as usize;
                // Row index for selected PWM (with reversed iteration, higher index is top)
                let sel_row = ((sel_pwm * graph_rows) / 100).min(graph_rows.saturating_sub(1));
                let x_sel = ((cols.saturating_sub(1) as u32) * sel_t as u32 / 100) as usize;

                // Removed debug information for cleaner display

                // Enhanced overlays - show current temperature and preview changes
                let mut x_curr_temp: Option<usize> = None;
                let mut curr_temp_value: f64 = 0.0;
                if let Some(g) = app.editor_groups.get(app.editor_group_idx) {
                    if let Some((_, c)) = app.temps.iter().find(|(name, _)| name == &g.temp_source) {
                        curr_temp_value = c.clamp(0.0, 100.0);
                        let x = ((cols.saturating_sub(1)) as f64 * (curr_temp_value / 100.0)).round() as usize;
                        x_curr_temp = Some(x);
                    }
                }
                for row in (0..graph_rows).rev() {
                    let mut spans: Vec<Span> = Vec::with_capacity(y_label_w + inner_pad + cols + inner_pad);
                    // Threshold percent for this row
                    let threshold = ((row + 1) as f32 * 100.0 / graph_rows as f32).ceil() as u8;
                    // Only show 50% marker row to reduce visual noise
                    let is_marker_row = threshold.abs_diff(50) <= (100.0 / graph_rows as f32).ceil() as u8;

                    // No Y label/axis in dense mode
                    if y_label_w > 0 { spans.push(Span::raw(" ".repeat(y_label_w))); }

                    // Left inner padding
                    spans.push(Span::raw(" ".repeat(inner_pad)));

                    // Content columns
                    for x in 0..cols {
                        let t = (x as u32 * 100 / (cols as u32 - 1)) as usize;
                        let pwm = app.editor_graph[t.min(100)];
                        let is_filled = pwm >= threshold;
                        // Align selection guide to the nearest drawable column for the selected temperature
                        let is_sel_col = x == x_sel;
                        // Determine if the current cell is on the plotted curve point (discrete dot per column)
                        let curve_row = ((pwm as usize * graph_rows) / 100).min(graph_rows.saturating_sub(1));
                        let on_curve_point = row == curve_row;

                        // Simplified overlay logic
                        let is_curr_temp_col = x_curr_temp.map(|v| v == x).unwrap_or(false);

                        // Enhanced base: smoother gradient and better visual hierarchy
                        let mut ch = if is_filled { '█' } else if is_marker_row { '─' } else { ' ' };
                        let mut style = if is_filled { 
                            // Gradient effect based on PWM level
                            let intensity = (pwm as f32 / 100.0 * 255.0) as u8;
                            if intensity > 200 { Style::default().fg(Color::Red) }
                            else if intensity > 150 { Style::default().fg(Color::Yellow) }
                            else if intensity > 100 { Style::default().fg(Color::Green) }
                            else { Style::default().fg(Color::Blue) }
                        } else { Style::default().fg(Color::DarkGray) };

                        // Enhanced curve point: animated and more visible
                        if on_curve_point {
                            ch = '●';
                            style = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
                        }

                        // Enhanced selection column highlight with preview
                        if is_sel_col {
                            if is_filled || on_curve_point {
                                style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD | Modifier::REVERSED);
                            } else {
                                ch = '│';
                                style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
                            }
                            // Show preview value if typing
                            if !app.editor_graph_input.is_empty() {
                                if let Ok(preview_val) = app.editor_graph_input.parse::<u8>() {
                                    let preview_val = preview_val.min(100);
                                    let preview_filled = preview_val >= threshold;
                                    if preview_filled && !is_filled {
                                        ch = '▓'; // Show preview fill
                                        style = Style::default().fg(Color::Magenta).add_modifier(Modifier::SLOW_BLINK);
                                    } else if !preview_filled && is_filled {
                                        ch = '░'; // Show preview empty
                                        style = Style::default().fg(Color::Magenta).add_modifier(Modifier::SLOW_BLINK);
                                    }
                                }
                            }
                        }
                        // Enhanced current temperature indicator
                        else if is_curr_temp_col && !on_curve_point {
                            if !is_filled {
                                ch = '┊';
                                style = Style::default().fg(Color::Red).add_modifier(Modifier::BOLD);
                            } else {
                                // Highlight current temp even when filled
                                style = style.add_modifier(Modifier::UNDERLINED);
                            }
                        }
                        spans.push(Span::styled(ch.to_string(), style));
                    }

                    // Right inner padding
                    spans.push(Span::raw(" ".repeat(inner_pad)));
                    graph_lines.push(Line::from(spans));
                }

                // Baseline: simple faint line to ground the bars
                let mut base: Vec<Span> = Vec::new();
                base.push(Span::raw(" ".repeat(y_label_w + inner_pad)));
                base.push(Span::styled("─".repeat(cols), Style::default().fg(Color::DarkGray)));
                base.push(Span::raw(" ".repeat(inner_pad)));
                graph_lines.push(Line::from(base));

                // Enhanced footer with real-time feedback and curve statistics
                let sel = app.editor_graph_sel.min(100);
                let pct = app.editor_graph[sel];
                let typed_disp = if app.editor_graph_input.is_empty() { String::from("--") } else { app.editor_graph_input.clone() };
                
                // Calculate curve statistics for better feedback
                let min_pwm = *app.editor_graph.iter().min().unwrap_or(&0);
                let max_pwm = *app.editor_graph.iter().max().unwrap_or(&0);
                let avg_pwm = app.editor_graph.iter().map(|&x| x as u32).sum::<u32>() / 101;
                
                // Current temperature feedback
                let curr_temp_info = if curr_temp_value > 0.0 {
                    let curr_pwm = crate::curves::interp_pwm_percent(&g.curve.points, curr_temp_value);
                    format!(" | Current: {:.1}°C -> {}%", curr_temp_value, curr_pwm)
                } else {
                    String::new()
                };
                
                // Preview feedback when typing
                let preview_info = if !app.editor_graph_input.is_empty() {
                    if let Ok(preview_val) = app.editor_graph_input.parse::<u8>() {
                        let preview_val = preview_val.min(100);
                        let delta = preview_val as i16 - pct as i16;
                        let delta_str = if delta > 0 { format!(" (+{}%)", delta) } 
                                       else if delta < 0 { format!(" ({}%)", delta) } 
                                       else { " (no change)".to_string() };
                        format!(" | Preview: {}%{}", preview_val, delta_str)
                    } else {
                        " | Preview: invalid".to_string()
                    }
                } else {
                    String::new()
                };
                
                // Enhanced tick marks with better spacing
                let mut tick_spans: Vec<Span> = Vec::new();
                let tick_positions = [0u16, 25, 50, 75, 100];
                for (i, pos) in tick_positions.iter().enumerate() {
                    let label = format!("{}°", pos);
                    let x = (cols as u32 * (*pos as u32) / 100) as usize;
                    let used = tick_spans.iter().map(|s| s.content.len()).sum::<usize>();
                    let pad = if i == 0 { x } else { x.saturating_sub(used) };
                    if pad > 0 { tick_spans.push(Span::raw(" ".repeat(pad))); }
                    // Highlight current temperature tick
                    let style = if (curr_temp_value - *pos as f64).abs() < 2.5 {
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Gray)
                    };
                    tick_spans.push(Span::styled(label, style));
                }
                
                let info = Paragraph::new(vec![
                    Line::from(tick_spans),
                    Line::from(format!(
                        "Selected: {:>3}°C -> {:>3}% (typed: {}){}{}   |   Range: {}%-{}% (avg: {}%)   |   Hysteresis: {}%   Delay: {}ms",
                        sel, pct, typed_disp, preview_info, curr_temp_info, min_pwm, max_pwm, avg_pwm, g.curve.hysteresis_pct, g.curve.apply_delay_ms
                    )),
                ]).wrap(Wrap { trim: false });
                f.render_widget(info, vchunks[2]);
            } else {
                // No group yet: show prompt in header/footer and leave canvas empty
                let mut footer_lines: Vec<Line> = Vec::new();
                footer_lines.push(Line::from(""));
                footer_lines.push(Line::from("Press 'u' to start editing once a group exists."));
                let footer_p = Paragraph::new(footer_lines).wrap(Wrap { trim: false });
                f.render_widget(footer_p, vchunks[2]);
            }

            // Render graph content paragraph inside the canvas (if any)
            let p_graph = Paragraph::new(graph_lines).wrap(Wrap { trim: false });
            f.render_widget(p_graph, canvas_inner);
        } else {
            // Read-only view: always show a minimal outline and a flat line at current PWM%
            // Header
            let mut header_lines: Vec<Line> = Vec::new();
            if let Some(g) = app.editor_groups.get(app.editor_group_idx) {
                header_lines.push(Line::from(format!("Name: {}  |  Temp: {}", g.name, g.temp_source)));
            } else {
                header_lines.push(Line::from("No curve group defined yet"));
            }
            header_lines.push(Line::from("Press 'u' to edit graph. s=save, e/Esc=exit, h=delay (ms)"));

            // Determine current PWM percent from selected CONTROL mapping
            let mut flat_pwm_pct: u8 = 0;
            if let Some(m) = app.mappings.get(app.control_idx.min(app.mappings.len().saturating_sub(1))) {
                if let Some((_, raw)) = app.pwms.iter().find(|(name, _)| name == &m.pwm) {
                    flat_pwm_pct = ((*raw as f64) * 100.0 / 255.0).round() as u8;
                }
            }

            // Build clean outline area
            let total_h = inner.height as usize;
            let total_w = inner.width as usize;
            // Reserve lines for header and footer
            let graph_rows = total_h.saturating_sub(6).max(5);
            // Simple column layout without excessive padding
            let cols = total_w.saturating_sub(2).clamp(20, 101);

            // Precompute curve row per column to plot the curve (cyan dots)
            let curve_rows: Option<Vec<usize>> = if let Some(g) = app.editor_groups.get(app.editor_group_idx) {
                let mut v = Vec::with_capacity(cols);
                for x in 0..cols {
                    let t = (x as u32 * 100 / (cols.saturating_sub(1) as u32)) as f64;
                    let pwm = curves::interp_pwm_percent(&g.curve.points, t) as usize;
                    let r = ((pwm * graph_rows) / 100).min(graph_rows.saturating_sub(1));
                    v.push(r);
                }
                Some(v)
            } else { None };

            let mut lines: Vec<Line> = Vec::new();
            // Header lines
            lines.extend(header_lines);
            lines.push(Line::from(""));

            // Draw clean graph with minimal visual elements
            for row in (0..graph_rows).rev() {
                let mut spans: Vec<Span> = Vec::with_capacity(cols + 1);
                // Simple left border
                spans.push(Span::styled("│", Style::default().fg(Color::DarkGray)));
                
                let threshold = ((row + 1) as f32 * 100.0 / graph_rows as f32).ceil() as u8;
                // Only show 50% reference line
                let is_mid_row = threshold.abs_diff(50) <= (100.0 / graph_rows as f32).ceil() as u8;
                
                for x in 0..cols {
                    let on_curve = curve_rows.as_ref().map_or(false, |cr| cr.get(x).copied().unwrap_or(usize::MAX) == row);
                    if on_curve {
                        spans.push(Span::styled("●", Style::default().fg(Color::Cyan)));
                    } else if is_mid_row {
                        spans.push(Span::styled("─", Style::default().fg(Color::DarkGray)));
                    } else {
                        spans.push(Span::raw(" "));
                    }
                }
                lines.push(Line::from(spans));
            }
            // Clean bottom axis
            lines.push(Line::from(""));
            let mut bottom: Vec<Span> = Vec::with_capacity(cols + 1);
            bottom.push(Span::styled("└", Style::default().fg(Color::DarkGray)));
            bottom.push(Span::styled("─".repeat(cols), Style::default().fg(Color::DarkGray)));
            lines.push(Line::from(bottom));

            // X-axis ticks labels (0 25 50 75 100)
            let mut tick_spans: Vec<Span> = Vec::new();
            let tick_positions = [0u16, 25, 50, 75, 100];
            for (i, pos) in tick_positions.iter().enumerate() {
                let label = format!("{}°", pos);
                let x = (cols as u32 * (*pos as u32) / 100) as usize;
                let used = tick_spans.iter().map(|s| s.content.len()).sum::<usize>();
                let pad = if i == 0 { x } else { x.saturating_sub(used) };
                if pad > 0 { tick_spans.push(Span::raw(" ".repeat(pad))); }
                tick_spans.push(Span::styled(label, Style::default().fg(Color::Gray)));
            }
            lines.push(Line::from(tick_spans));

            let p = Paragraph::new(lines).wrap(Wrap { trim: false });
            f.render_widget(p, inner);
        }

        // Delay popup overlay
        if app.show_curve_delay_popup {
            let area = centered_rect(50, 30, size);
            let block = Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" Set Apply Delay (ms) ");
            let inner = block.inner(area);
            f.render_widget(Clear, area);
            f.render_widget(block, area);
            let lines = vec![
                Line::from(""),
                Line::from("Enter milliseconds (0..600000):"),
                Line::from(Span::styled(app.curve_delay_input.as_str(), Style::default().fg(Color::Cyan))),
                Line::from("Enter to apply, Esc to cancel"),
                Line::from(""),
            ];
            let p = Paragraph::new(lines).alignment(Alignment::Center).wrap(Wrap { trim: false });
            f.render_widget(p, inner);
        }

        // Hysteresis popup overlay
        if app.show_curve_hyst_popup {
            let area = centered_rect(50, 30, size);
            let block = Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" Set Hysteresis (%) ");
            let inner = block.inner(area);
            f.render_widget(Clear, area);
            f.render_widget(block, area);
            let lines = vec![
                Line::from(""),
                Line::from("Enter percent (0..50):"),
                Line::from(Span::styled(app.curve_hyst_input.as_str(), Style::default().fg(Color::Cyan))),
                Line::from("Enter to apply, Esc to cancel"),
                Line::from(""),
            ];
            let p = Paragraph::new(lines).alignment(Alignment::Center).wrap(Wrap { trim: false });
            f.render_widget(p, inner);
        }

        // Temperature source selection popup overlay
        if app.show_temp_source_popup {
            let area = centered_rect(70, 60, size);
            let block = Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" Select Temperature Source ");
            let inner = block.inner(area);
            f.render_widget(Clear, area);
            f.render_widget(block, area);
            
            let mut lines = vec![
                Line::from(""),
                Line::from("Available temperature sensors:"),
                Line::from(""),
            ];
            
            // Show temperature sources grouped by CONTROL mappings
            let mut control_temps: std::collections::HashMap<String, Vec<(String, f64)>> = std::collections::HashMap::new();
            
            // Group temps by their associated control mappings
            for (temp_name, temp_value) in &app.temps {
                let mut found_control = false;
                for mapping in &app.mappings {
                    // Check if this temp is used by any curve group in this control mapping
                    if let Some(group) = app.editor_groups.iter().find(|g| g.temp_source == *temp_name) {
                        if group.members.iter().any(|member| member == &mapping.pwm) {
                            control_temps.entry(format!("{} -> {}", mapping.fan, mapping.pwm))
                                .or_insert_with(Vec::new)
                                .push((temp_name.clone(), *temp_value));
                            found_control = true;
                            break;
                        }
                    }
                }
                if !found_control {
                    control_temps.entry("Available".to_string())
                        .or_insert_with(Vec::new)
                        .push((temp_name.clone(), *temp_value));
                }
            }
            
            let mut temp_list = Vec::new();
            for (control_name, temps) in control_temps {
                lines.push(Line::from(Span::styled(format!("[{}]", control_name), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
                for (temp_name, temp_value) in temps {
                    temp_list.push((temp_name.clone(), temp_value));
                    let is_selected = temp_list.len() - 1 == app.temp_source_selection;
                    let is_current = app.editor_groups.get(app.editor_group_idx)
                        .map(|g| g.temp_source == temp_name)
                        .unwrap_or(false);
                    
                    let mut style = Style::default();
                    let mut prefix = "  ";
                    
                    if is_current {
                        style = style.fg(Color::Green).add_modifier(Modifier::BOLD);
                        prefix = "* ";
                    }
                    if is_selected {
                        style = style.bg(Color::Blue).add_modifier(Modifier::REVERSED);
                        prefix = "> ";
                    }
                    
                    let display_name = app.temp_aliases.get(&temp_name).unwrap_or(&temp_name);
                    lines.push(Line::from(Span::styled(
                        format!("{}{} ({:.1}°C)", prefix, display_name, temp_value),
                        style
                    )));
                }
                lines.push(Line::from(""));
            }
            
            lines.extend(vec![
                Line::from(""),
                Line::from("↑/↓ navigate  |  Enter select  |  Esc cancel"),
                Line::from(Span::styled("* = currently assigned", Style::default().fg(Color::Green))),
            ]);
            
            let p = Paragraph::new(lines).wrap(Wrap { trim: false });
            f.render_widget(p, inner);
        }

        // Save confirmation popup for editor
        if app.show_editor_save_confirm {
            let area = centered_rect(50, 30, size);
            let block = Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" Save changes? ");
            let inner = block.inner(area);
            f.render_widget(Clear, area);
            f.render_widget(block, area);
            let lines = vec![
                Line::from("Save curve changes to /etc/hyperfan/curves.json?"),
                Line::from("Enter: save    Esc: cancel"),
            ];
            let p = Paragraph::new(lines).alignment(Alignment::Center);
            f.render_widget(p, inner);
        }

        // Status line at bottom overlay
        let footer = "e/Esc - exit | s - save | d - default curves | h - delay (ms) | y - hysteresis (%) | ↑↓ select pair | u - edit graph | r - rename control pair";
        let status = Paragraph::new(footer).style(Style::default().fg(Color::Gray));
        let bottom = Rect { x: size.x, y: size.y + size.height.saturating_sub(1), width: size.width, height: 1 };
        f.render_widget(status, bottom);
        return;
    }

    // Layout: header | columns | control | status
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(7),
            Constraint::Length(1),
        ])
        .split(size);

    // Header split: left info and right metric indicator
    let header_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(80), Constraint::Percentage(20)])
        .split(chunks[0]);

    let header_text = format!(
        " CPU: {}    |    Motherboard: {}    |    hwmon chips: {} ",
        if app.cpu_name.is_empty() { "?" } else { &app.cpu_name },
        if app.mb_name.is_empty() { "?" } else { &app.mb_name },
        app.readings.len()
    );
    let header = Paragraph::new(header_text).style(Style::default().fg(Color::Yellow));
    f.render_widget(header, header_cols[0]);

    let metric_label = match app.metric { crate::config::Metric::C => "Metric: °C", crate::config::Metric::F => "Metric: °F", crate::config::Metric::K => "Metric: K" };
    let metric_widget = Paragraph::new(metric_label).alignment(Alignment::Right).style(Style::default().fg(Color::Gray));
    f.render_widget(metric_widget, header_cols[1]);

    // Three columns
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(34),
            Constraint::Percentage(33),
        ])
        .split(chunks[1]);

    // Build lists
    let (fans_block, pwms_block, temps_block) = (
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" FANS ({}) ", app.fans.len())),
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" PWM ({}) ", app.pwms.len())),
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" TEMP ({}) ", app.temps.len())),
    );

    let highlight = Style::default().bg(Color::Blue).fg(Color::White);
    let focus_style = |focus: Focus| -> Style {
        if app.focus == focus {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        }
    };

    let header_style =
        Style::default().fg(Color::Gray).add_modifier(Modifier::BOLD | Modifier::UNDERLINED);

    // Fans: header + rows
    let mut fans_items: Vec<ListItem> = Vec::with_capacity(app.fans.len() + 1);
    fans_items.push(ListItem::new(format!("{:<40} {:>6}", "Name", "RPM")).style(header_style));
    fans_items.extend(
        app.fans
            .iter()
            .map(|(name, rpm)| {
                let disp = app.fan_aliases.get(name).cloned().unwrap_or_else(|| name.clone());
                ListItem::new(format!("{:<40} {:>6} RPM", disp, rpm))
            }),
    );

    // PWMs: header + rows (as % from raw 0-255)
    let mut pwms_items: Vec<ListItem> = Vec::with_capacity(app.pwms.len() + 1);
    pwms_items.push(ListItem::new(format!("{:<40} {:>6}", "Name", "%")).style(header_style));
    pwms_items.extend(app.pwms.iter().map(|(name, val)| {
        let pct = ((*val as f64) * 100.0 / 255.0).round() as u64;
        let disp = app.pwm_aliases.get(name).cloned().unwrap_or_else(|| name.clone());
        ListItem::new(format!("{:<40} {:>5}%", disp, pct))
    }));

    // Temps: header + rows (unit conversion)
    let mut temps_items: Vec<ListItem> = Vec::with_capacity(app.temps.len() + 1);
    let unit = match app.metric { crate::config::Metric::C => "°C", crate::config::Metric::F => "°F", crate::config::Metric::K => "K" };
    temps_items.push(ListItem::new(format!("{:<40} {:>7}", "Name", unit)).style(header_style));
    temps_items.extend(app.temps.iter().map(|(name, c)| {
        let (val, unit_str) = app.convert_temp(*c);
        let disp = app.temp_aliases.get(name).cloned().unwrap_or_else(|| name.clone());
        ListItem::new(format!("{:<40} {:>5.1} {}", disp, val, unit_str))
    }));

    let fans_block = fans_block.border_style(focus_style(Focus::Fans));
    let pwms_block = pwms_block.border_style(focus_style(Focus::Pwms));
    let temps_block = temps_block.border_style(focus_style(Focus::Temps));

    let mut fans_state = ListState::default();
    if !app.fans.is_empty() {
        fans_state.select(Some(app.fans_idx + 1));
    }
    let mut pwms_state = ListState::default();
    if !app.pwms.is_empty() {
        pwms_state.select(Some(app.pwms_idx + 1));
    }
    let mut temps_state = ListState::default();
    if !app.temps.is_empty() {
        temps_state.select(Some(app.temps_idx + 1));
    }

    let fans_list = List::new(fans_items).block(fans_block).highlight_style(highlight);
    let pwms_list = List::new(pwms_items).block(pwms_block).highlight_style(highlight);
    let temps_list = List::new(temps_items).block(temps_block).highlight_style(highlight);

    f.render_stateful_widget(fans_list, cols[0], &mut fans_state);
    f.render_stateful_widget(pwms_list, cols[1], &mut pwms_state);
    f.render_stateful_widget(temps_list, cols[2], &mut temps_state);

    // CONTROL block
    let control_block = Block::default()
        .borders(Borders::ALL)
        .title(" CONTROL (FAN -> PWM) ")
        .border_style(focus_style(Focus::Control));

    let mut control_text = String::new();
    if app.mappings.is_empty() {
        control_text.push_str("(no mappings) Press 'm' to add mapping from current selections.");
    }

    let _control_style = if app.focus == Focus::Control && !app.mappings.is_empty() {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(Color::Gray)
    };

    // Build lookup helpers for current values
    // Build sorted CONTROL mappings by fan display name, preserve selection marker
    let mut control_items: Vec<Line> = Vec::new();
    let mut mapped: Vec<(usize, String, String, String, String)> = app
        .mappings
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

    let chip_from_key = |key: &str| -> String { key.split(':').next().unwrap_or(key).to_string() };

    for (orig_i, fan_key, pwm_key, fan_disp, pwm_disp) in mapped.into_iter() {
        let marker = if app.focus == Focus::Control && orig_i == app.control_idx { "> " } else { "  " };
        let fan_rpm = app
            .fans
            .iter()
            .find(|(name, _)| name == &app.mappings[orig_i].fan)
            .map(|(_, rpm)| *rpm)
            .unwrap_or(0);
        let pwm_raw = app
            .pwms
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
        .block(control_block)
        .wrap(Wrap { trim: false });
    f.render_widget(control_list, chunks[2]);

    // Draw auto-detect popup if active
    if app.show_auto_detect {
        let popup_area = centered_rect(70, 70, size);
        f.render_widget(Clear, popup_area);

        let popup_block = Block::default()
            .title(" Auto-Detect Fan/PWM Pairings ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        let popup_inner = popup_block.inner(popup_area);
        f.render_widget(popup_block, popup_area);

        // Check if detection is running or complete
        let is_running = match app.auto_detect_running.lock() {
            Ok(guard) => *guard,
            Err(poisoned) => *poisoned.into_inner(),
        };
        let results = match app.auto_detect_results.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        // Split inner area for text and progress bar
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(10), Constraint::Length(3), Constraint::Min(5)])
            .split(popup_inner);

        let detect_text = vec![
            Line::from("Automatically detecting fan/PWM pairings..."),
            Line::from(""),
            Line::from("This process will:"),
            Line::from("  1. Test each PWM controller"),
            Line::from("  2. Ramp fans up and down"),
            Line::from("  3. Detect which fans respond"),
            Line::from("  4. Create mappings with confidence scores"),
            Line::from(""),
            Line::from("⚠️  WARNING! ALL fans will spin to 100% to protect your box. It will be loud :)"),
        ];

        let detect_paragraph = Paragraph::new(detect_text).alignment(Alignment::Left);
        f.render_widget(detect_paragraph, chunks[0]);

        // If awaiting user confirmation, show confirm text and do not start yet
        if app.auto_detect_await_confirm {
            let lines = vec![
                Line::from("Auto-Detect Fan/PWM Pairings"),
                Line::from(""),
                Line::from("This will test each PWM and observe fan RPM changes."),
                Line::from(""),
                Line::from("WARNING: All controlled fans may briefly run at 100%!"),
                Line::from("Ensure it's safe to proceed."),
                Line::from(""),
                Line::from("Press Enter to START. Press Esc to cancel."),
            ];
            let p = Paragraph::new(lines).alignment(Alignment::Left);
            f.render_widget(p, chunks[0]);
        }
        // Draw progress bar if running
        else if is_running {
            // Get current progress
            let progress = hwmon::get_auto_detect_progress();
            let progress_text = format!("Testing PWM controllers... {:.0}%", progress * 100.0);

            let progress_bar = Gauge::default()
                .block(Block::default().borders(Borders::NONE))
                .gauge_style(Style::default().fg(Color::Cyan).bg(Color::Black))
                .percent((progress * 100.0) as u16)
                .label(progress_text);

            f.render_widget(progress_bar, chunks[1]);

            let status_text = vec![Line::from(""), Line::from("Please wait... (Press Esc to cancel)")];
            let status_paragraph = Paragraph::new(status_text).alignment(Alignment::Center);
            f.render_widget(status_paragraph, chunks[2]);
        } else if !results.is_empty() {
            // Show results summary with Accept/Discard actions
            let mut result_text = vec![
                Line::from(format!("Detection complete! {} pairing(s) found:", results.len())),
                Line::from(""),
            ];

            for pairing in results.iter() {
                result_text.push(Line::from(format!(
                    "  {} → {}   (confidence: {:.0}%)",
                    pairing.fan_label,
                    pairing.pwm_label,
                    pairing.confidence * 100.0
                )));
            }

            result_text.push(Line::from(""));
            result_text.push(Line::from("Press Enter to ACCEPT (saves to /etc/hyperfan/profile.json and updates CONTROL)"));
            result_text.push(Line::from("Press Esc to DISCARD (no changes saved)"));

            let result_paragraph = Paragraph::new(result_text).alignment(Alignment::Left);
            f.render_widget(result_paragraph, chunks[2]);
        } else if !is_running && results.is_empty() {
            // No results found
            let no_result_text = vec![
                Line::from(""),
                Line::from("No pairings detected. Ensure fans are connected."),
                Line::from(""),
                Line::from("Press Esc to close"),
            ];
            let no_result_paragraph = Paragraph::new(no_result_text).alignment(Alignment::Center);
            f.render_widget(no_result_paragraph, chunks[2]);
        }
    }

    // Draw fan curve popup if active
    if app.show_curve_popup {
        let popup_area = centered_rect(60, 60, size);
        f.render_widget(Clear, popup_area);

        let popup_block = Block::default()
            .title(" Fan Curve Configuration ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow));

        let popup_inner = popup_block.inner(popup_area);
        f.render_widget(popup_block, popup_area);

        // Create curve visualization
        let mut curve_text = vec![
            Line::from("Temperature -> PWM % Curve:"),
            Line::from(""),
        ];

        for (temp, pwm) in &app.curve_temp_points {
            curve_text.push(Line::from(format!("  {}°C -> {}%", temp, pwm)));
        }

        curve_text.push(Line::from(""));
        curve_text.push(Line::from("Press Enter to save, Esc to cancel"));
        curve_text.push(Line::from("(Curve editing coming soon)"));

        let curve_paragraph = Paragraph::new(curve_text).alignment(Alignment::Left);
        f.render_widget(curve_paragraph, popup_inner);
    }

    // Draw Set PWM popup if active
    if app.show_set_pwm_popup {
        let popup_area = centered_rect(50, 40, size);
        f.render_widget(Clear, popup_area);

        let popup_block = Block::default()
            .title(" Set PWM Percent ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Magenta));

        let inner = popup_block.inner(popup_area);
        f.render_widget(popup_block, popup_area);

        let mut lines: Vec<Line> = Vec::new();
        if let Some((chip, idx, label)) = &app.set_pwm_target {
            lines.push(Line::from(format!("Target: {} pwm{} ({})", chip, idx, label)));
        } else {
            lines.push(Line::from("No PWM target resolved (map a fan->pwm first)"));
        }
        lines.push(Line::from(""));
        lines.push(Line::from("Enter value 0-100 (%), then press Enter"));
        lines.push(Line::from("Use ↑/↓ to adjust (Shift for ±5)"));
        lines.push(Line::from("(Esc to cancel)"));
        lines.push(Line::from(""));
        let percent_disp = if app.set_pwm_input.is_empty() {
            "00".to_string()
        } else if app.set_pwm_input == "100" {
            "100".to_string()
        } else {
            format!("{:0>2}", app.set_pwm_input)
        };
        lines.push(Line::from(format!("Percent: {}%", percent_disp)));
        if let Some((is_error, msg)) = &app.set_pwm_feedback {
            lines.push(Line::from(""));
            let style = if *is_error {
                Style::default().fg(Color::Red)
            } else {
                Style::default().fg(Color::Green)
            };
            lines.push(Line::styled(msg.clone(), style));
        }

        let p = Paragraph::new(lines).alignment(Alignment::Left);
        f.render_widget(p, inner);
    }

    // Draw Warning popup if active
    if app.show_warning_popup {
        let popup_area = centered_rect(50, 30, size);
        f.render_widget(Clear, popup_area);

        let popup_block = Block::default()
            .title(" Warning ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red));

        let inner = popup_block.inner(popup_area);
        f.render_widget(popup_block, popup_area);

        let lines = vec![
            Line::from(app.warning_message.clone()),
            Line::from(""),
            Line::from("Press Enter or Esc to close."),
        ];

        let p = Paragraph::new(lines).alignment(Alignment::Left);
        f.render_widget(p, inner);
    }

    // Draw Confirm Save popup if active
    if app.show_confirm_save_popup {
        let popup_area = centered_rect(50, 30, size);
        f.render_widget(Clear, popup_area);

        let popup_block = Block::default()
            .title(" Confirm Save System Config ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green));

        let inner = popup_block.inner(popup_area);
        f.render_widget(popup_block, popup_area);

        let lines = vec![
            Line::from("You're about to overwrite /etc/hyperfan/profile.json."),
            Line::from(""),
            Line::from("Press Enter to confirm overwrite."),
            Line::from("Press Esc to cancel."),
        ];

        let p = Paragraph::new(lines).alignment(Alignment::Left);
        f.render_widget(p, inner);
    }

    // Status + bottom help (main screen only)
    // Keep the existing single-line layout but render two lines inside the Paragraph:
    // 1) dynamic status, 2) static help with keybindings (includes rename)
    let mut status_lines: Vec<Line> = Vec::new();
    status_lines.push(Line::from(app.status.as_str()));
    status_lines.push(Line::from(
        "Keys: Tab/Shift+Tab switch | ↑/↓ move | ←/→ switch focus | m map | r rename | d delete | c curves | g groups | a auto-detect | s save | R refresh | Enter set/apply",
    ));
    let status = Paragraph::new(status_lines).style(Style::default().fg(Color::Gray));
    f.render_widget(status, chunks[3]);
}
