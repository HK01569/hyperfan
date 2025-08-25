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
use crate::hwmon;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, BorderType, Clear, Gauge, List, ListItem, Paragraph, Wrap},
    Frame,
};
use ratatui::layout::Rect;

/// Helper function to create a centered rectangle for popups
pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
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

/// Render the groups manager UI
pub fn render_groups_manager(f: &mut Frame, app: &App, size: Rect) {
    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(size);

    // Left panel: Groups list
    let mut groups_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" PWM Groups ");
    groups_block = if app.groups_focus_right {
        groups_block
    } else {
        groups_block.border_style(Style::default().fg(Color::Cyan))
    };

    let groups_inner = groups_block.inner(layout[0]);
    f.render_widget(groups_block, layout[0]);

    let group_items: Vec<ListItem> = app.groups.iter().enumerate().map(|(idx, g)| {
        let style = if idx == app.group_idx {
            Style::default().bg(Color::DarkGray)
        } else {
            Style::default()
        };
        ListItem::new(g.name.clone()).style(style)
    }).collect();

    let groups_list = List::new(group_items);
    f.render_widget(groups_list, groups_inner);

    // Right panel: PWM members
    let mut members_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" PWM Members ");
    members_block = if app.groups_focus_right {
        members_block.border_style(Style::default().fg(Color::Cyan))
    } else {
        members_block
    };

    let members_inner = members_block.inner(layout[1]);
    f.render_widget(members_block, layout[1]);

    let current_group = app.groups.get(app.group_idx);
    let member_items: Vec<ListItem> = app.pwms.iter().enumerate().map(|(idx, (pwm_full, _))| {
        let is_member = current_group.map_or(false, |g| g.members.contains(pwm_full));
        let prefix = if is_member { "[x] " } else { "[ ] " };
        let style = if idx == app.groups_pwm_idx {
            Style::default().bg(Color::DarkGray)
        } else {
            Style::default()
        };
        ListItem::new(format!("{}{}", prefix, pwm_full)).style(style)
    }).collect();

    let members_list = List::new(member_items);
    f.render_widget(members_list, members_inner);
}

/// Render the map PWM to FAN popup
pub fn render_map_pwm_popup(f: &mut Frame, app: &App, size: Rect) {
    let area = centered_rect(60, 60, size);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Map PWM to FAN ");
    let inner = block.inner(area);

    f.render_widget(Clear, area);
    f.render_widget(block, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(2),
        ])
        .split(inner);

    // Show selected PWM
    let pwm_text = if let Some((pwm_full, _)) = app.pwms.get(app.groups_pwm_idx) {
        format!("Mapping PWM: {}", pwm_full)
    } else {
        "No PWM selected".to_string()
    };
    let pwm_para = Paragraph::new(pwm_text)
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    f.render_widget(pwm_para, layout[0]);

    // Show fan list
    let fan_items: Vec<ListItem> = app.fans.iter().enumerate().map(|(idx, (fan_full, _))| {
        let style = if idx == app.groups_pwm_idx {
            Style::default().bg(Color::DarkGray)
        } else {
            Style::default()
        };
        ListItem::new(fan_full.clone()).style(style)
    }).collect();

    let fan_list = List::new(fan_items)
        .block(Block::default().borders(Borders::TOP).title(" Select FAN "));
    f.render_widget(fan_list, layout[1]);

    // Instructions
    let instructions = Paragraph::new("↑/↓ to select, Enter to confirm, Esc to cancel")
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Gray));
    f.render_widget(instructions, layout[2]);
}

/// Render the group name popup (for new/rename)
pub fn render_group_name_popup(f: &mut Frame, app: &App, size: Rect) {
    let area = centered_rect(50, 30, size);
    let title = if app.group_rename_mode {
        " Rename Group "
    } else {
        " New Group Name "
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(title);
    let inner = block.inner(area);

    f.render_widget(Clear, area);
    f.render_widget(block, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(inner);

    let prompt = Paragraph::new("Enter group name:")
        .alignment(Alignment::Center);
    f.render_widget(prompt, layout[0]);

    let input = Paragraph::new(app.group_name_input.as_str())
        .block(Block::default().borders(Borders::ALL))
        .alignment(Alignment::Center);
    f.render_widget(input, layout[1]);

    let instructions = Paragraph::new("Enter to confirm, Esc to cancel")
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Gray));
    f.render_widget(instructions, layout[2]);
}

/// Render the auto-detect popup
pub fn render_auto_detect_popup(f: &mut Frame, app: &App, size: Rect) {
    let popup_area = centered_rect(70, 70, size);
    f.render_widget(Clear, popup_area);

    let block = Block::default()
        .title(" Auto-Detect PWM-to-Fan Mappings ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);
    f.render_widget(block.clone(), popup_area);

    let inner = block.inner(popup_area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Status
            Constraint::Length(4),  // Warning/Notice
            Constraint::Min(5),     // Results/Progress
            Constraint::Length(2),  // Instructions
        ])
        .split(inner);

    // Status message
    let (status_text, status_style) = if app.auto_detect_await_confirm {
        ("🎉 AUTO-DETECT COMPLETE! 🎉", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
    } else {
        let is_running = app.auto_detect_running.lock()
            .map(|g| *g)
            .unwrap_or(false);
        if is_running {
            ("⚙ Auto-detection in progress...", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        } else {
            ("Fan Auto-Detection", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        }
    };

    let status = Paragraph::new(status_text)
        .alignment(Alignment::Center)
        .style(status_style)
        .wrap(Wrap { trim: true });
    f.render_widget(status, chunks[0]);
    
    // Warning/Notice section
    let is_running = app.auto_detect_running.lock()
        .map(|g| *g)
        .unwrap_or(false);
    
    if !app.auto_detect_await_confirm && !is_running {
        let warning = Paragraph::new(vec![
            Line::from("⚠️  WARNING: Fans will cycle between low and high speeds!").style(Style::default().fg(Color::Yellow)),
            Line::from("This may cause temporary noise and temperature changes.").style(Style::default().fg(Color::Gray)),
        ])
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
        f.render_widget(warning, chunks[1]);
    } else if app.auto_detect_await_confirm {
        let notice = Paragraph::new(vec![
            Line::from("🔍 Review the detected fan-to-PWM mappings below").style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Line::from("✅ Press ENTER to save these mappings to your configuration").style(Style::default().fg(Color::Yellow)),
            Line::from("❌ Press ESC to discard and return to main menu").style(Style::default().fg(Color::Red)),
        ])
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
        f.render_widget(notice, chunks[1]);
    } else {
        // Running - show current test info if available
        let info = Paragraph::new("Testing PWM controllers...")
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::Gray));
        f.render_widget(info, chunks[1]);
    }

    // Results or progress
    if app.auto_detect_await_confirm {
        let results = app.auto_detect_results.lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        
        let mut lines = vec![];
        if results.is_empty() {
            lines.push(Line::from("No mappings detected").style(Style::default().fg(Color::Red)));
            lines.push(Line::from(""));
            lines.push(Line::from("This could mean:").style(Style::default().fg(Color::Gray)));
            lines.push(Line::from("• Fans are not connected to PWM headers"));
            lines.push(Line::from("• Fans are connected but not responding"));
            lines.push(Line::from("• PWM control is not available"));
        } else {
            lines.push(Line::from(format!("Found {} mapping(s):", results.len()))
                .style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));
            lines.push(Line::from(""));
            for pairing in &results {
                let pwm_name = pairing.pwm_label.split(':').last().unwrap_or(&pairing.pwm_label);
                let fan_name = pairing.fan_label.split(':').last().unwrap_or(&pairing.fan_label);
                let line = format!("  • PWM {} → Fan {}", pwm_name, fan_name);
                lines.push(Line::from(line).style(Style::default().fg(Color::Cyan)));
            }
        }
        
        let results_list = Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title(" Detection Results "))
            .wrap(Wrap { trim: true });
        f.render_widget(results_list, chunks[2]);
    } else {
        let is_running = app.auto_detect_running.lock()
            .map(|g| *g)
            .unwrap_or(false);
        
        if is_running {
            let progress = hwmon::get_auto_detect_progress();
            
            // Create a more compact progress display
            let progress_block = Block::default()
                .borders(Borders::ALL)
                .title(" Progress ");
            let progress_inner = progress_block.inner(chunks[2]);
            f.render_widget(progress_block, chunks[2]);
            
            // Use a single-line gauge
            let gauge_area = Rect {
                x: progress_inner.x,
                y: progress_inner.y + 1,
                width: progress_inner.width,
                height: 1,  // Single line height
            };
            
            let gauge = Gauge::default()
                .gauge_style(Style::default().fg(Color::Cyan).bg(Color::Black))
                .percent((progress * 100.0) as u16)
                .label(format!("{}%", (progress * 100.0) as u16));
            f.render_widget(gauge, gauge_area);
            
            // Add progress text below
            let progress_text = Paragraph::new("Testing PWM controllers...")
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::Gray));
            let text_area = Rect {
                x: progress_inner.x,
                y: progress_inner.y + 3,
                width: progress_inner.width,
                height: 1,
            };
            f.render_widget(progress_text, text_area);
        } else {
            let info = Paragraph::new(vec![
                Line::from("This process will:"),
                Line::from("• Test each PWM controller individually"),
                Line::from("• Cycle fan speeds to detect connections"),
                Line::from("• Take approximately 30-60 seconds"),
                Line::from(""),
                Line::from("Press Enter to begin auto-detection").style(Style::default().fg(Color::Green)),
            ])
            .block(Block::default().borders(Borders::ALL).title(" Information "))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });
            f.render_widget(info, chunks[2]);
        }
    }

    // Instructions
    let (instructions, instruction_style) = if app.auto_detect_await_confirm {
        ("🔥 ENTER = Save Changes    ESC = Discard Results 🔥", 
         Style::default().fg(Color::White).bg(Color::Blue).add_modifier(Modifier::BOLD))
    } else {
        let is_running = app.auto_detect_running.lock()
            .map(|g| *g)
            .unwrap_or(false);
        if is_running {
            ("[Esc] Cancel Detection", Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC))
        } else {
            ("[Enter] Start Detection  [Esc] Cancel", Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC))
        }
    };
    
    let instructions_widget = Paragraph::new(instructions)
        .alignment(Alignment::Center)
        .style(instruction_style);
    f.render_widget(instructions_widget, chunks[3]);
}

/// Render the fan curve popup
pub fn render_curve_popup(f: &mut Frame, app: &App, size: Rect) {
    let popup_area = centered_rect(60, 60, size);
    f.render_widget(Clear, popup_area);

    let block = Block::default()
        .title(" Fan Curve ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);
    f.render_widget(block.clone(), popup_area);

    let inner = block.inner(popup_area);
    
    let text = if let Some(mapping) = app.mappings.get(app.control_idx) {
        format!(
            "Fan: {}\nPWM: {}\n\nCurve points:\n{}",
            mapping.fan,
            mapping.pwm,
            "Default linear curve (not yet implemented)"
        )
    } else {
        "No mapping selected".to_string()
    };

    let paragraph = Paragraph::new(text)
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Left);
    f.render_widget(paragraph, inner);
}

/// Render the set PWM popup
pub fn render_set_pwm_popup(f: &mut Frame, app: &App, size: Rect) {
    let popup_area = centered_rect(50, 40, size);
    f.render_widget(Clear, popup_area);

    let block = Block::default()
        .title(" Set PWM Value ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);
    f.render_widget(block.clone(), popup_area);

    let inner = block.inner(popup_area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Min(1),
        ])
        .split(inner);

    // Show selected PWM
    let pwm_text = if let Some((pwm_full, _)) = app.pwms.get(app.pwms_idx) {
        format!("PWM: {}", pwm_full)
    } else {
        "No PWM selected".to_string()
    };
    let pwm_para = Paragraph::new(pwm_text)
        .alignment(Alignment::Center);
    f.render_widget(pwm_para, chunks[0]);

    // Input field
    let input_text = if app.set_pwm_input.is_empty() {
        "___".to_string()
    } else {
        format!("{:>3}", app.set_pwm_input)
    };
    let input = Paragraph::new(input_text)
        .block(Block::default().borders(Borders::ALL).title(" Value (0-100%) "))
        .alignment(Alignment::Center);
    f.render_widget(input, chunks[1]);

    // Instructions
    let instructions = Paragraph::new("Type 0-100, ↑/↓ to adjust, Enter to apply, Esc to cancel")
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Gray));
    f.render_widget(instructions, chunks[2]);
}

/// Render the warning popup
pub fn render_warning_popup(f: &mut Frame, app: &App, size: Rect) {
    let popup_area = centered_rect(50, 30, size);
    f.render_widget(Clear, popup_area);

    let block = Block::default()
        .title(" Warning ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Yellow));
    f.render_widget(block.clone(), popup_area);

    let inner = block.inner(popup_area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(inner);

    let warning = Paragraph::new(app.warning_message.as_str())
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Center);
    f.render_widget(warning, chunks[0]);

    let instructions = Paragraph::new("Press Enter to dismiss")
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Gray));
    f.render_widget(instructions, chunks[1]);
}

/// Render the confirm save popup
pub fn render_confirm_save_popup(f: &mut Frame, _app: &App, size: Rect) {
    let popup_area = centered_rect(50, 30, size);
    f.render_widget(Clear, popup_area);

    let block = Block::default()
        .title(" Confirm Save ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);
    f.render_widget(block.clone(), popup_area);

    let inner = block.inner(popup_area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(inner);

    let message = Paragraph::new("Save current configuration to system?")
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Center);
    f.render_widget(message, chunks[0]);

    let instructions = Paragraph::new("Press Enter to save, Esc to cancel")
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Gray));
    f.render_widget(instructions, chunks[1]);
}
