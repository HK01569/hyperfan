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
use ratatui::prelude::*;

mod ui_components;
mod ui_curve_editor;
mod ui_main;

use ui_components::*;
use ui_curve_editor::render_curve_editor;
use ui_main::render_main_view;

/// Main UI rendering function that delegates to appropriate view
pub fn ui(f: &mut Frame, app: &App) {
    let size = f.area();

    // Groups manager page
    if app.show_groups_manager {
        render_groups_manager(f, app, size);
        
        // Render popups for groups manager
        if app.show_map_pwm_popup {
            render_map_pwm_popup(f, app, size);
        }
        if app.show_group_name_popup {
            render_group_name_popup(f, app, size);
        }
        return;
    }

    // Curve editor page
    if app.show_curve_editor {
        render_curve_editor(f, app, size);
        return;
    }

    // Main view (default)
    render_main_view(f, app, size);
    
    // Render popups for main view
    if app.show_auto_detect {
        render_auto_detect_popup(f, app, size);
    }
    if app.show_curve_popup {
        render_curve_popup(f, app, size);
    }
    if app.show_set_pwm_popup {
        render_set_pwm_popup(f, app, size);
    }
    if app.show_warning_popup {
        render_warning_popup(f, app, size);
    }
    if app.show_confirm_save_popup {
        render_confirm_save_popup(f, app, size);
    }
}
