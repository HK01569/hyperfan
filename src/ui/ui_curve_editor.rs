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

use crate::app::App;
use super::ui_components::centered_rect;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, BorderType, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};

/// Render the curve editor UI
pub fn render_curve_editor(f: &mut Frame, app: &App, size: Rect) {
    // Vertical split: main content | bottom bar
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(10),
            Constraint::Length(3),
        ])
        .split(size);

    // Horizontal split: left = CONTROL pairs, right = Graph
    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(main_layout[0]);

    // Left panel: Control groups list
    render_control_panel(f, app, layout[0]);

    // Right panel: Graph or point editor
    if app.editor_graph_mode {
        render_graph_panel(f, app, layout[1]);
    } else {
        render_point_editor(f, app, layout[1]);
    }

    // Bottom bar: Save and Exit
    render_bottom_bar(f, app, main_layout[1]);

    // Render popups
    if app.show_curve_delay_popup {
        render_delay_popup(f, app, size);
    }
    if app.show_curve_hyst_popup {
        render_hysteresis_popup(f, app, size);
    }
    if app.show_temp_source_popup {
        render_temp_source_popup(f, app, size);
    }
    if app.show_editor_save_confirm {
        render_save_confirm_popup(f, app, size);
    }
}

fn render_control_panel(f: &mut Frame, app: &App, area: Rect) {
    let control_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(format!(" Control Groups ({}) [↑↓ to navigate, Enter to edit] ", app.editor_groups.len()));
    
    let control_block = if app.editor_focus_right {
        control_block
    } else {
        control_block.border_style(Style::default().fg(Color::Cyan))
    };

    let control_inner = control_block.inner(area);
    f.render_widget(control_block, area);

    if app.editor_groups.is_empty() {
        let empty_msg = Paragraph::new("No control groups defined\n\nPress 'n' to create a new group")
            .style(Style::default().fg(Color::Gray))
            .alignment(Alignment::Center);
        f.render_widget(empty_msg, control_inner);
        return;
    }

    // Use the actual editor_group_idx without sorting to maintain correct selection
    let control_items: Vec<ListItem> = app.editor_groups.iter().enumerate().map(|(idx, group)| {
        let style = if idx == app.editor_group_idx {
            Style::default().bg(Color::Blue).fg(Color::White)
        } else {
            Style::default()
        };
        
        // Format members more concisely
        let members_str = if group.members.len() > 2 {
            format!("{} PWMs", group.members.len())
        } else {
            group.members.iter()
                .map(|m| m.split(':').last().unwrap_or(m))
                .collect::<Vec<_>>()
                .join(", ")
        };
        
        let marker = if idx == app.editor_group_idx { "> " } else { "  " };
        let display = format!("{}{}: {}", marker, group.name, members_str);
        ListItem::new(display).style(style)
    }).collect();

    let control_list = List::new(control_items);
    f.render_widget(control_list, control_inner);
}

fn render_graph_panel(f: &mut Frame, app: &App, area: Rect) {
    let title = if app.editor_graph_mode {
        " Fan Curve Graph [INTERACTIVE] • ←→ navigate, ↑↓ adjust, Enter confirm "
    } else {
        " Fan Curve Graph • Press 'u' for interactive editing "
    };
    
    let graph_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(title)
        .border_style(if app.editor_graph_mode {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Cyan)
        });
    
    let graph_inner = graph_block.inner(area);
    f.render_widget(graph_block, area);

    // Get current group and temperature for indicator
    let current_group = app.editor_groups.get(app.editor_group_idx);
    let current_temp = if let Some(group) = current_group {
        // Find current temperature reading from the temp source
        app.temps.iter()
            .find(|(name, _)| name == &group.temp_source)
            .map(|(_, temp)| *temp as usize)
            .unwrap_or(25) // Default to 25°C if not found
    } else {
        25
    };
    
    // Create beautiful graph with enhanced visuals
    let graph_height = graph_inner.height.saturating_sub(4) as usize; // Leave space for labels
    let graph_width = graph_inner.width.saturating_sub(8) as usize;   // Leave space for Y-axis labels
    
    if graph_height > 0 && graph_width > 0 {
        let mut lines = vec![];
        
        // Title line with current group info
        if let Some(group) = current_group {
            let temp_source_short = group.temp_source.split('/').last().unwrap_or(&group.temp_source);
            let title_line = vec![
                Span::styled("Group: ", Style::default().fg(Color::Gray)),
                Span::styled(&group.name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled(" │ Sensor: ", Style::default().fg(Color::Gray)),
                Span::styled(temp_source_short, Style::default().fg(Color::Yellow)),
                Span::styled(format!(" │ Current: {}°C", current_temp), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            ];
            lines.push(Line::from(title_line));
            lines.push(Line::from(""));
        }
        
        // Scale the graph data to fit the display area
        let temp_step = 101.0 / graph_width as f64;
        
        // Draw the graph from top to bottom (high PWM to low PWM)
        for y in 0..graph_height {
            let mut line_spans = vec![];
            
            // Y-axis labels (PWM percentage)
            let pwm_level = 100 - (y * 100 / (graph_height - 1));
            if y % ((graph_height / 6).max(1)) == 0 || y == graph_height - 1 {
                line_spans.push(Span::styled(
                    format!("{:3}%│", pwm_level),
                    Style::default().fg(Color::DarkGray)
                ));
            } else {
                line_spans.push(Span::styled(
                    "    │",
                    Style::default().fg(Color::DarkGray)
                ));
            }
            
            let mut line_chars = Vec::new();
            
            for x in 0..graph_width {
                let temp_idx = ((x as f64 * temp_step) as usize).min(100);
                let graph_pwm = app.editor_graph[temp_idx];
                let graph_y = (100 - graph_pwm as usize) * (graph_height - 1) / 100;
                
                // Current temperature indicator
                let is_current_temp = temp_idx == current_temp;
                
                // Selection indicator (in interactive mode)
                let is_selected = app.editor_graph_mode && temp_idx == app.editor_graph_sel;
                
                // Determine character and color
                let (char, color) = if is_current_temp && y == graph_y {
                    ('◉', Color::Red)  // Current temperature point on curve
                } else if is_current_temp {
                    if y == 0 {
                        ('▼', Color::Red)  // Current temp marker at top
                    } else if y == graph_height - 1 {
                        ('▲', Color::Red)  // Current temp marker at bottom
                    } else {
                        ('┃', Color::Red)  // Current temp vertical line
                    }
                } else if is_selected && app.editor_graph_mode {
                    if y == graph_y {
                        ('⬢', Color::Yellow)  // Selected point
                    } else {
                        ('┊', Color::Yellow)   // Selection guide line
                    }
                } else if y == graph_y {
                    // Check if this is an actual curve point or interpolated
                    if current_group
                        .and_then(|g| g.curve.points.iter().find(|p| (p.temp_c as usize) == temp_idx))
                        .is_some() {
                        ('●', Color::Cyan)  // Actual curve point
                    } else {
                        ('━', Color::Cyan)  // Interpolated curve line
                    }
                } else if y == 0 && x % (graph_width / 8).max(1) == 0 {
                    ('┬', Color::DarkGray)  // Top grid
                } else if y == graph_height - 1 && x % (graph_width / 8).max(1) == 0 {
                    ('┴', Color::DarkGray)  // Bottom grid
                } else if x % (graph_width / 8).max(1) == 0 {
                    ('┼', Color::DarkGray)  // Grid intersection
                } else if y == 0 || y == graph_height - 1 {
                    ('─', Color::DarkGray)  // Top/bottom border
                } else {
                    (' ', Color::Reset)     // Empty space
                };
                
                line_chars.push((char, color));
            }
            
            // Convert characters to spans with colors
            let mut current_color = Color::Reset;
            let mut current_text = String::new();
            let mut char_spans = Vec::new();
            
            for (ch, color) in line_chars {
                if color != current_color {
                    if !current_text.is_empty() {
                        char_spans.push(Span::styled(current_text.clone(), Style::default().fg(current_color)));
                        current_text.clear();
                    }
                    current_color = color;
                }
                current_text.push(ch);
            }
            if !current_text.is_empty() {
                char_spans.push(Span::styled(current_text, Style::default().fg(current_color)));
            }
            
            line_spans.extend(char_spans);
            lines.push(Line::from(line_spans));
        }
        
        // X-axis temperature labels
        let mut temp_scale_spans = vec![Span::styled("    └", Style::default().fg(Color::DarkGray))];
        let mut temp_scale = String::new();
        for x in 0..graph_width {
            let temp = (x as f64 * temp_step) as usize;
            if x == 0 {
                temp_scale.push_str("0°C");
            } else if x == graph_width / 4 {
                temp_scale.push_str("25°C");
            } else if x == graph_width / 2 {
                temp_scale.push_str("50°C");
            } else if x == 3 * graph_width / 4 {
                temp_scale.push_str("75°C");
            } else if x >= graph_width - 5 && temp >= 95 {
                if temp_scale.len() + 5 <= graph_width {
                    temp_scale.push_str("100°C");
                }
                break;
            } else {
                temp_scale.push('─');
            }
        }
        temp_scale_spans.push(Span::styled(temp_scale, Style::default().fg(Color::DarkGray)));
        lines.push(Line::from(temp_scale_spans));
        
        // Legend and status
        lines.push(Line::from(""));
        let legend_spans = vec![
            Span::styled("Legend: ", Style::default().fg(Color::Gray)),
            Span::styled("━ ", Style::default().fg(Color::Cyan)),
            Span::styled("Curve ", Style::default().fg(Color::Gray)),
            Span::styled("● ", Style::default().fg(Color::Cyan)),
            Span::styled("Points ", Style::default().fg(Color::Gray)),
            Span::styled("◉ ", Style::default().fg(Color::Red)),
            Span::styled("Current Temp ", Style::default().fg(Color::Gray)),
        ];
        if app.editor_graph_mode {
            let mut interactive_spans = legend_spans;
            interactive_spans.extend(vec![
                Span::styled("⬢ ", Style::default().fg(Color::Yellow)),
                Span::styled("Selection", Style::default().fg(Color::Gray)),
            ]);
            lines.push(Line::from(interactive_spans));
            
            // Current selection info
            let sel_temp = app.editor_graph_sel;
            let sel_pwm = app.editor_graph[sel_temp.min(100)];
            let info_spans = vec![
                Span::styled("Editing: ", Style::default().fg(Color::Gray)),
                Span::styled(format!("{}°C", sel_temp), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled(" → ", Style::default().fg(Color::Gray)),
                Span::styled(format!("{}%", sel_pwm), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled(" │ Input: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    if app.editor_graph_input.is_empty() { "___".to_string() } else { app.editor_graph_input.clone() },
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
                ),
            ];
            lines.push(Line::from(info_spans));
        } else {
            lines.push(Line::from(legend_spans));
            lines.push(Line::from(Span::styled(
                "Press 'u' to enter interactive editing mode",
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)
            )));
        }
        
        let graph_text = Paragraph::new(lines);
        f.render_widget(graph_text, graph_inner);
    }
}

fn render_point_editor(f: &mut Frame, app: &App, area: Rect) {
    let title = if app.editor_graph_mode {
        " Curve Details [Graph Mode Active] "
    } else {
        " Curve Details [List Mode] "
    };
    
    let editor_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(title)
        .border_style(if !app.editor_graph_mode {
            Style::default().fg(Color::Blue)
        } else {
            Style::default().fg(Color::DarkGray)
        });
    
    let editor_inner = editor_block.inner(area);
    f.render_widget(editor_block, area);

    // Get current group's curve using the actual index
    if let Some(group) = app.editor_groups.get(app.editor_group_idx) {
        let mut lines = vec![];
        
        // Group info section with better visual hierarchy
        lines.push(Line::from(vec![
            Span::styled("━━━ ", Style::default().fg(Color::DarkGray)),
            Span::styled("GROUP INFO", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(" ━━━", Style::default().fg(Color::DarkGray)),
        ]));
        
        lines.push(Line::from(vec![
            Span::styled("Name: ", Style::default().fg(Color::Gray)),
            Span::styled(&group.name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]));
        
        // Members with icons
        let members_str = if group.members.len() > 3 {
            format!("{} PWMs", group.members.len())
        } else {
            let pwms = group.members.iter()
                .map(|m| m.split(':').last().unwrap_or(m))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{}", pwms)
        };
        lines.push(Line::from(vec![
            Span::styled("Controls: ", Style::default().fg(Color::Gray)),
            Span::raw(members_str),
        ]));
        
        // Temperature source with icon
        let temp_source = group.temp_source.split('/').last()
            .unwrap_or(&group.temp_source);
        lines.push(Line::from(vec![
            Span::styled("Sensor: ", Style::default().fg(Color::Gray)),
            Span::raw(""),
            Span::styled(temp_source, Style::default().fg(Color::Yellow)),
        ]));
        
        lines.push(Line::from(""));
        
        // Curve points section
        lines.push(Line::from(vec![
            Span::styled("━━━ ", Style::default().fg(Color::DarkGray)),
            Span::styled("CURVE POINTS", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled(" ━━━", Style::default().fg(Color::DarkGray)),
        ]));
        
        if group.curve.points.is_empty() {
            lines.push(Line::from(Span::styled(
                "  No points defined",
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)
            )));
        } else {
            for (i, point) in group.curve.points.iter().enumerate() {
                let is_selected = !app.editor_graph_mode && i == app.editor_point_idx;
                let marker = if is_selected { "▶" } else { " " };
                let point_str = format!("{} {}. {}°C → {}%", 
                    marker, i + 1, point.temp_c, point.pwm_pct);
                let style = if is_selected {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                lines.push(Line::from(Span::styled(point_str, style)));
            }
        }
        
        lines.push(Line::from(""));
        
        // Advanced settings section
        lines.push(Line::from(vec![
            Span::styled("━━━ ", Style::default().fg(Color::DarkGray)),
            Span::styled("ADVANCED", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
            Span::styled(" ━━━", Style::default().fg(Color::DarkGray)),
        ]));
        
        lines.push(Line::from(vec![
            Span::styled("Delay: ", Style::default().fg(Color::Gray)),
            Span::styled(format!("{}ms", group.curve.apply_delay_ms), Style::default().fg(Color::White)),
            Span::styled(" (response time)", Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Hysteresis: ", Style::default().fg(Color::Gray)),
            Span::styled(format!("{}%", group.curve.hysteresis_pct), Style::default().fg(Color::White)),
            Span::styled(" (deadband)", Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)),
        ]));
        
        lines.push(Line::from(""));
        
        // Context-sensitive keyboard shortcuts
        lines.push(Line::from(vec![
            Span::styled("━━━ ", Style::default().fg(Color::DarkGray)),
            Span::styled("CONTROLS", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
            Span::styled(" ━━━", Style::default().fg(Color::DarkGray)),
        ]));
        
        if app.editor_graph_mode {
            lines.push(Line::from(vec![
                Span::styled("←→", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled(": Move cursor", Style::default().fg(Color::Gray)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("↑↓", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled(": Adjust PWM", Style::default().fg(Color::Gray)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("0-9", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled(": Enter value", Style::default().fg(Color::Gray)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Enter", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled(": Set point", Style::default().fg(Color::Gray)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("u", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
                Span::styled(": Exit graph mode", Style::default().fg(Color::Gray)),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled("Tab", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
                Span::styled(": Switch panels", Style::default().fg(Color::Gray)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("↑↓", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled(": Navigate items", Style::default().fg(Color::Gray)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("a", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled(": Add point", Style::default().fg(Color::Gray)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("d", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::styled(": Delete point", Style::default().fg(Color::Gray)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("e", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(": Edit point", Style::default().fg(Color::Gray)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("u", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::styled(": Graph mode", Style::default().fg(Color::Gray)),
            ]));
        }
        
        
        let editor_text = Paragraph::new(lines);
        f.render_widget(editor_text, editor_inner);
    }
}

fn render_bottom_bar(f: &mut Frame, _app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Actions ")
        .border_style(Style::default().fg(Color::DarkGray));
    
    let inner = block.inner(area);
    f.render_widget(block, area);
    
    let actions = vec![
        Span::styled("s", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::styled(": Save all curves", Style::default().fg(Color::Gray)),
        Span::styled("  │  ", Style::default().fg(Color::DarkGray)),
        Span::styled("Esc", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        Span::styled(": Exit curve editor", Style::default().fg(Color::Gray)),
    ];
    
    let actions_line = Line::from(actions);
    let actions_paragraph = Paragraph::new(actions_line)
        .alignment(Alignment::Center);
    
    f.render_widget(actions_paragraph, inner);
}

pub fn render_delay_popup(f: &mut Frame, app: &App, size: Rect) {
    let area = centered_rect(50, 30, size);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Set Apply Delay (ms) ");
    let inner = block.inner(area);

    f.render_widget(Clear, area);
    f.render_widget(block, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(inner);

    let input = Paragraph::new(app.curve_delay_input.as_str())
        .block(Block::default().borders(Borders::ALL))
        .alignment(Alignment::Center);
    f.render_widget(input, layout[0]);

    let instructions = Paragraph::new("Enter delay in milliseconds, Esc to cancel")
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Gray));
    f.render_widget(instructions, layout[1]);
}

pub fn render_hysteresis_popup(f: &mut Frame, app: &App, size: Rect) {
    let area = centered_rect(50, 30, size);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Set Hysteresis (%) ");
    let inner = block.inner(area);

    f.render_widget(Clear, area);
    f.render_widget(block, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(inner);

    let input = Paragraph::new(app.curve_hyst_input.as_str())
        .block(Block::default().borders(Borders::ALL))
        .alignment(Alignment::Center);
    f.render_widget(input, layout[0]);

    let instructions = Paragraph::new("Enter hysteresis percentage (0-50), Esc to cancel")
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Gray));
    f.render_widget(instructions, layout[1]);
}

pub fn render_temp_source_popup(f: &mut Frame, app: &App, size: Rect) {
    let area = centered_rect(70, 60, size);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Select Temperature Source ");
    let inner = block.inner(area);

    f.render_widget(Clear, area);
    f.render_widget(block, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(5),
            Constraint::Length(2),
        ])
        .split(inner);

    // Temperature list
    let temp_items: Vec<ListItem> = app.temps.iter().enumerate().map(|(idx, (temp_full, _))| {
        let style = if idx == app.temp_source_selection {
            Style::default().bg(Color::DarkGray)
        } else {
            Style::default()
        };
        ListItem::new(temp_full.clone()).style(style)
    }).collect();

    let temp_list = List::new(temp_items)
        .block(Block::default().borders(Borders::TOP));
    f.render_widget(temp_list, layout[0]);

    // Instructions
    let instructions = Paragraph::new("↑/↓ to select, Enter to confirm, Esc to cancel")
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Gray));
    f.render_widget(instructions, layout[1]);
}

fn render_save_confirm_popup(f: &mut Frame, _app: &App, size: Rect) {
    let area = centered_rect(50, 30, size);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Save changes? ");
    let inner = block.inner(area);

    f.render_widget(Clear, area);
    f.render_widget(block, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(inner);

    let message = Paragraph::new("Save curve configuration changes?")
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Center);
    f.render_widget(message, layout[0]);

    let instructions = Paragraph::new("Press Enter to save, Esc to cancel")
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Gray));
    f.render_widget(instructions, layout[1]);
}
