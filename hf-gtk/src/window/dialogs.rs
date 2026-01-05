//! Dialogs module - all dialog functions for the window

use gtk4::prelude::*;
use gtk4::glib;
use libadwaita as adw;
use libadwaita::prelude::*;
use std::rc::Rc;
use std::time::Duration;

use crate::widgets::{EcControlPage, SettingsPage};

/// Ko-fi support URL
const KOFI_URL: &str = "https://ko-fi.com/henryk44801";
/// GitHub repository URL
const GITHUB_URL: &str = "https://github.com/HK01569/hyperfan";

/// Show EC Control as a modal dialog overlay
pub fn show_ec_control_dialog(btn: &gtk4::Button) {
    let dialog = adw::Dialog::builder()
        .title("EC Direct Control")
        .content_width(800)
        .content_height(600)
        .presentation_mode(adw::DialogPresentationMode::Floating)
        .build();

    let ec_page = EcControlPage::new();
    
    let content = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Vertical)
        .vexpand(true)
        .hexpand(true)
        .build();
    
    let header = adw::HeaderBar::builder()
        .show_end_title_buttons(true)
        .build();
    content.append(&header);
    
    let ec_widget = ec_page.widget();
    ec_widget.set_vexpand(true);
    ec_widget.set_hexpand(true);
    content.append(ec_widget);
    
    dialog.set_child(Some(&content));

    if let Some(root) = btn.root() {
        if let Some(window) = root.downcast_ref::<gtk4::Window>() {
            dialog.present(Some(window));
        }
    }
}

/// Show dialog prompting user about unsaved settings changes
pub fn show_unsaved_changes_dialog<F: Fn() + 'static>(
    widget: &impl IsA<gtk4::Widget>,
    settings_page: Rc<SettingsPage>,
    on_discard: F,
) {
    let window = widget.root()
        .and_then(|r| r.downcast::<gtk4::Window>().ok());

    let dialog = adw::AlertDialog::builder()
        .heading("Unsaved Changes")
        .body("You have unsaved settings changes. Do you want to discard them?")
        .build();

    dialog.add_response("cancel", "Cancel");
    dialog.add_response("discard", "Discard");
    dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");

    let settings_for_response = settings_page.clone();
    dialog.connect_response(None, move |dialog, response| {
        if response == "discard" {
            settings_for_response.reset_dirty();
            on_discard();
        }
        dialog.close();
    });

    dialog.present(window.as_ref());
}

/// Show dialog when daemon is not detected
pub fn show_daemon_not_running_dialog(window: &adw::ApplicationWindow, settings_page: Rc<SettingsPage>) {
    let dialog = adw::AlertDialog::builder()
        .heading("Daemon Not Running")
        .body("Hyperfan didn't detect the daemon running - install it from settings")
        .build();

    dialog.add_response("cancel", "Cancel");
    dialog.add_response("ok", "OK");
    dialog.add_response("settings", "Settings");
    
    dialog.set_response_appearance("settings", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("ok"));
    dialog.set_close_response("cancel");

    let window_clone = window.clone();
    let settings_page_clone = settings_page.clone();
    
    dialog.connect_response(None, move |_dialog, response| {
        if response == "settings" {
            // Navigate to settings page
            if let Some(stack) = window_clone.content()
                .and_then(|c| c.first_child())
                .and_then(|c| c.last_child())
                .and_then(|c| c.first_child())
                .and_then(|c| c.last_child())
                .and_then(|c| c.downcast::<gtk4::Stack>().ok())
            {
                stack.set_visible_child_name("settings");
                
                let settings_clone = settings_page_clone.clone();
                glib::timeout_add_local_once(Duration::from_millis(100), move || {
                    settings_clone.flash_daemon_install_card();
                });
            }
        }
    });

    dialog.present(Some(window));
}

/// Show the preferences dialog
#[allow(dead_code)]
pub fn show_preferences_dialog(parent: &adw::ApplicationWindow) {
    let dialog = adw::PreferencesDialog::builder()
        .build();

    // General page
    let general_page = adw::PreferencesPage::builder()
        .title("General")
        .icon_name("preferences-system-symbolic")
        .build();

    // Startup group
    let startup_group = adw::PreferencesGroup::builder()
        .title("Startup")
        .description("Control how Hyperfan starts with your system")
        .build();

    // Boot autostart toggle
    let boot_row = adw::SwitchRow::builder()
        .title("Start at boot")
        .subtitle("Apply fan curves before login screen (requires root)")
        .build();

    boot_row.set_active(is_boot_service_enabled());

    boot_row.connect_active_notify(|row| {
        let enabled = row.is_active();
        if let Err(e) = set_boot_service_enabled(enabled) {
            tracing::error!("Failed to set boot service: {}", e);
            row.set_active(!enabled);
        }
    });

    startup_group.add(&boot_row);
    general_page.add(&startup_group);

    // About group
    let about_group = adw::PreferencesGroup::builder()
        .title("About")
        .build();

    let version_row = adw::ActionRow::builder()
        .title("Version")
        .subtitle(env!("CARGO_PKG_VERSION"))
        .build();

    about_group.add(&version_row);
    general_page.add(&about_group);

    dialog.add(&general_page);
    dialog.present(Some(parent));
}

/// Check if the boot service is enabled
fn is_boot_service_enabled() -> bool {
    #[cfg(target_os = "linux")]
    {
        let service_path = std::path::Path::new("/etc/systemd/system/hyperfan.service");
        if !service_path.exists() {
            return false;
        }
        std::process::Command::new("systemctl")
            .args(["is-enabled", "hyperfan.service"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    
    #[cfg(any(target_os = "freebsd", target_os = "openbsd", target_os = "netbsd"))]
    {
        let rc_path = std::path::Path::new("/usr/local/etc/rc.d/hyperfan");
        if !rc_path.exists() {
            return false;
        }
        std::fs::read_to_string("/etc/rc.conf")
            .map(|s| s.contains("hyperfan_enable=\"YES\""))
            .unwrap_or(false)
    }
    
    #[cfg(not(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd", target_os = "netbsd")))]
    {
        false
    }
}

/// Enable or disable the boot service
fn set_boot_service_enabled(enabled: bool) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let service_content = r#"[Unit]
Description=Hyperfan Fan Control Daemon
After=local-fs.target

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/usr/bin/hyperfan-daemon --apply-curves

[Install]
WantedBy=multi-user.target
"#;

        if enabled {
            let service_path = "/etc/systemd/system/hyperfan.service";
            let result = std::process::Command::new("pkexec")
                .args(["bash", "-c", &format!(
                    "echo '{}' > {} && systemctl daemon-reload && systemctl enable hyperfan.service",
                    service_content, service_path
                )])
                .status();

            match result {
                Ok(status) if status.success() => Ok(()),
                Ok(_) => Err("Failed to enable boot service".to_string()),
                Err(e) => Err(format!("Failed to run pkexec: {}", e)),
            }
        } else {
            let result = std::process::Command::new("pkexec")
                .args(["bash", "-c", 
                    "systemctl disable hyperfan.service 2>/dev/null; rm -f /etc/systemd/system/hyperfan.service; systemctl daemon-reload"
                ])
                .status();

            match result {
                Ok(status) if status.success() => Ok(()),
                Ok(_) => Err("Failed to disable boot service".to_string()),
                Err(e) => Err(format!("Failed to run pkexec: {}", e)),
            }
        }
    }
    
    #[cfg(any(target_os = "freebsd", target_os = "openbsd", target_os = "netbsd"))]
    {
        let rc_script = r#"#!/bin/sh
# PROVIDE: hyperfan
# REQUIRE: DAEMON
# KEYWORD: shutdown

. /etc/rc.subr

name="hyperfan"
rcvar="hyperfan_enable"
command="/usr/local/bin/hyperfan-daemon"
command_args="--apply-curves"

load_rc_config $name
run_rc_command "$1"
"#;

        if enabled {
            let result = std::process::Command::new("doas")
                .args(["sh", "-c", &format!(
                    "echo '{}' > /usr/local/etc/rc.d/hyperfan && chmod +x /usr/local/etc/rc.d/hyperfan && echo 'hyperfan_enable=\"YES\"' >> /etc/rc.conf",
                    rc_script
                )])
                .status();

            match result {
                Ok(status) if status.success() => Ok(()),
                Ok(_) => Err("Failed to enable boot service".to_string()),
                Err(e) => Err(format!("Failed to run doas: {}", e)),
            }
        } else {
            let result = std::process::Command::new("doas")
                .args(["sh", "-c", 
                    "rm -f /usr/local/etc/rc.d/hyperfan; sed -i '' '/hyperfan_enable/d' /etc/rc.conf"
                ])
                .status();

            match result {
                Ok(status) if status.success() => Ok(()),
                Ok(_) => Err("Failed to disable boot service".to_string()),
                Err(e) => Err(format!("Failed to run doas: {}", e)),
            }
        }
    }
    
    #[cfg(not(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd", target_os = "netbsd")))]
    {
        let _ = enabled;
        Err("Boot service not supported on this platform".to_string())
    }
}

/// Show support dialog with Ko-fi and GitHub links
pub fn show_support_dialog(widget: &impl IsA<gtk4::Widget>) {
    let window = widget.root()
        .and_then(|r| r.downcast::<gtk4::Window>().ok());

    let dialog = adw::AlertDialog::builder()
        .heading("Support Hyperfan")
        .body("Thank you for considering supporting Hyperfan! Your support helps keep this project alive and growing.")
        .build();

    dialog.add_response("cancel", "Cancel");
    dialog.add_response("github", "GitHub");
    dialog.add_response("kofi", "Ko-fi");
    
    dialog.set_response_appearance("kofi", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("kofi"));
    dialog.set_close_response("cancel");

    dialog.connect_response(None, move |_dialog, response| {
        let url = match response {
            "kofi" => Some(KOFI_URL),
            "github" => Some(GITHUB_URL),
            _ => None,
        };
        
        if let Some(url) = url {
            if let Err(e) = open::that(url) {
                tracing::error!("Failed to open URL {}: {}", url, e);
            }
        }
    });

    dialog.present(window.as_ref());
}
