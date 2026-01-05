//! Service Management
//!
//! Manages the privileged hyperfand service installation across multiple init systems.
//!
//! # Supported Init Systems
//! - **systemd** (most Linux distros)
//! - **OpenRC** (Gentoo, Alpine, Artix)
//! - **runit** (Void Linux, Artix)
//! - **BSD rc.d** (FreeBSD, OpenBSD, NetBSD, DragonFlyBSD)
//!
//! The daemon provides secure IPC for hardware access without requiring
//! the GUI to run with elevated privileges.

use std::path::Path;
use std::process::Command;

const DAEMON_BINARY: &str = "hyperfand";

/// Get socket path based on detected OS (runtime detection)
pub fn get_socket_path() -> &'static str {
    if is_bsd() {
        "/var/run/hyperfan.sock"
    } else if Path::new("/run").exists() {
        "/run/hyperfan.sock"
    } else {
        "/var/run/hyperfan.sock"
    }
}

/// Detect if running on BSD at runtime
pub fn is_bsd() -> bool {
    // Check for BSD-specific paths and files
    Path::new("/etc/rc.subr").exists()
        || std::fs::read_to_string("/etc/os-release")
            .map(|s| s.to_lowercase().contains("bsd"))
            .unwrap_or(false)
        || Command::new("uname")
            .arg("-s")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_lowercase().contains("bsd"))
            .unwrap_or(false)
}

/// Detected init system
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitSystem {
    Systemd,
    OpenRC,
    Runit,
    BsdRc, // FreeBSD, OpenBSD, NetBSD rc.d
    Unknown,
}

impl std::fmt::Display for InitSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InitSystem::Systemd => write!(f, "systemd"),
            InitSystem::OpenRC => write!(f, "OpenRC"),
            InitSystem::Runit => write!(f, "runit"),
            InitSystem::BsdRc => write!(f, "BSD rc.d"),
            InitSystem::Unknown => write!(f, "unknown"),
        }
    }
}

/// Detect the init system in use (runtime detection)
pub fn detect_init_system() -> InitSystem {
    // Check for BSD first (runtime detection)
    if is_bsd() {
        return InitSystem::BsdRc;
    }

    // Check for systemd
    if Path::new("/run/systemd/system").exists() {
        return InitSystem::Systemd;
    }

    // Check for OpenRC
    if Path::new("/sbin/openrc").exists() || Path::new("/usr/sbin/openrc").exists() {
        return InitSystem::OpenRC;
    }

    // Check for runit
    if Path::new("/run/runit").exists() || Path::new("/etc/runit").exists() {
        return InitSystem::Runit;
    }

    // Check PID 1 process name (Linux /proc filesystem)
    if let Ok(cmdline) = std::fs::read_to_string("/proc/1/comm") {
        let init = cmdline.trim();
        if init.contains("systemd") {
            return InitSystem::Systemd;
        } else if init.contains("openrc") || init == "init" {
            if Path::new("/etc/init.d").exists() && Path::new("/etc/conf.d").exists() {
                return InitSystem::OpenRC;
            }
        } else if init.contains("runit") {
            return InitSystem::Runit;
        }
    }

    // Final BSD check via rc.d paths
    if Path::new("/etc/rc.d").exists() && Path::new("/etc/rc.conf").exists() {
        return InitSystem::BsdRc;
    }

    InitSystem::Unknown
}

// ============================================================================
// Service file templates
// ============================================================================

fn systemd_service(daemon_path: &str) -> String {
    format!(
        r#"[Unit]
Description=Hyperfan Fan Control Daemon
Documentation=https://github.com/hyperfan/hyperfan
After=local-fs.target

[Service]
Type=simple
ExecStart={} --foreground
Restart=on-failure
RestartSec=5

# Security hardening
NoNewPrivileges=false
ProtectSystem=strict
ProtectHome=read-only
PrivateTmp=true
ReadWritePaths=/sys/class/hwmon /sys/devices /run

[Install]
WantedBy=multi-user.target
"#,
        daemon_path
    )
}

fn openrc_service(daemon_path: &str) -> String {
    format!(
        r#"#!/sbin/openrc-run
# Hyperfan Fan Control Daemon

name="hyperfand"
description="Hyperfan privileged daemon for fan control"
command="{}"
command_args="--foreground"
command_background=true
pidfile="/run/hyperfand.pid"

depend() {{
    need localmount
    after bootmisc
}}

start_pre() {{
    checkpath --directory --mode 0755 /run
}}
"#,
        daemon_path
    )
}

fn runit_run_script(daemon_path: &str) -> String {
    format!(
        r#"#!/bin/sh
# Hyperfan Fan Control Daemon
exec {} --foreground 2>&1
"#,
        daemon_path
    )
}

fn runit_finish_script() -> &'static str {
    r#"#!/bin/sh
# Cleanup on stop
rm -f /run/hyperfan.sock /run/hyperfand.pid
"#
}

/// BSD rc.d script (works on FreeBSD, OpenBSD, NetBSD, DragonFlyBSD)
fn bsd_rc_script(daemon_path: &str) -> String {
    format!(
        r#"#!/bin/sh
#
# PROVIDE: hyperfand
# REQUIRE: DAEMON
# KEYWORD: shutdown
#
# Add the following line to /etc/rc.conf to enable hyperfand:
#   hyperfand_enable="YES"

. /etc/rc.subr

name="hyperfand"
rcvar="hyperfand_enable"
desc="Hyperfan fan control daemon"

command="{}"
command_args="--foreground"
pidfile="/var/run/hyperfand.pid"

start_precmd="hyperfand_prestart"

hyperfand_prestart()
{{
    # Ensure /var/run exists
    if [ ! -d /var/run ]; then
        mkdir -p /var/run
    fi
}}

load_rc_config $name
: ${{hyperfand_enable:="NO"}}
run_rc_command "$1"
"#,
        daemon_path
    )
}

// ============================================================================
// Status checking
// ============================================================================

/// Check if the daemon service is installed
pub fn is_service_installed() -> bool {
    match detect_init_system() {
        InitSystem::Systemd => Path::new("/etc/systemd/system/hyperfan.service").exists(),
        InitSystem::OpenRC => Path::new("/etc/init.d/hyperfand").exists(),
        InitSystem::Runit => {
            Path::new("/etc/sv/hyperfand/run").exists()
                || Path::new("/var/service/hyperfand").exists()
        }
        InitSystem::BsdRc => {
            Path::new("/usr/local/etc/rc.d/hyperfand").exists()
                || Path::new("/etc/rc.d/hyperfand").exists()
        }
        InitSystem::Unknown => false,
    }
}

/// Check if the daemon service is currently running
pub fn is_service_running() -> bool {
    match detect_init_system() {
        InitSystem::Systemd => Command::new("systemctl")
            .args(["is-active", "--quiet", "hyperfan.service"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false),
        InitSystem::OpenRC => Command::new("rc-service")
            .args(["hyperfand", "status"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false),
        InitSystem::Runit => Command::new("sv")
            .args(["status", "hyperfand"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("run:"))
            .unwrap_or(false),
        InitSystem::BsdRc => Command::new("service")
            .args(["hyperfand", "status"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false),
        InitSystem::Unknown => false,
    }
}

/// Check if the daemon socket is available
pub fn is_socket_available() -> bool {
    Path::new(get_socket_path()).exists()
}

/// Check if daemon is installed in system path
fn is_daemon_in_system_path() -> bool {
    Path::new("/usr/local/bin/hyperfand").exists() || Path::new("/usr/bin/hyperfand").exists()
}

/// Get system install path for daemon
fn get_system_daemon_path() -> &'static str {
    "/usr/local/bin/hyperfand"
}

/// Find the daemon binary - checks system paths and local build directory
pub fn find_daemon_binary() -> Option<String> {
    // First check system paths
    if Path::new("/usr/local/bin/hyperfand").exists() {
        return Some("/usr/local/bin/hyperfand".to_string());
    }
    if Path::new("/usr/bin/hyperfand").exists() {
        return Some("/usr/bin/hyperfand".to_string());
    }

    // Check next to current executable (installed together or development)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let daemon_path = dir.join(DAEMON_BINARY);
            if daemon_path.exists() {
                return Some(daemon_path.to_string_lossy().to_string());
            }
        }
    }

    None
}

/// Find local daemon binary that needs to be installed (not in system path)
fn find_local_daemon_binary() -> Option<String> {
    // Check next to current executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let daemon_path = dir.join(DAEMON_BINARY);
            if daemon_path.exists() {
                return Some(daemon_path.to_string_lossy().to_string());
            }
        }
    }
    None
}

// ============================================================================
// Installation
// ============================================================================

/// Install the daemon binary and service (requires root via pkexec)
/// Uses a SINGLE pkexec call to install binary + service + start daemon
pub fn install_service() -> Result<(), String> {
    let init = detect_init_system();
    let daemon_dest = get_system_daemon_path();

    // Check if we need to install the binary
    let binary_install_script = if is_daemon_in_system_path() {
        // Already installed - no binary copy needed
        String::new()
    } else {
        // Need to install daemon binary
        let local_daemon = find_local_daemon_binary().ok_or_else(|| {
            "Could not find hyperfand binary next to hyperfan. Build hyperfan-daemon first."
                .to_string()
        })?;

        format!(
            "cp '{}' '{}' && chmod 755 '{}' && chown root:root '{}' && ",
            local_daemon, daemon_dest, daemon_dest, daemon_dest
        )
    };

    // Get the daemon path (either existing or where we'll install it)
    let daemon_path = if is_daemon_in_system_path() {
        if Path::new("/usr/local/bin/hyperfand").exists() {
            "/usr/local/bin/hyperfand".to_string()
        } else {
            "/usr/bin/hyperfand".to_string()
        }
    } else {
        daemon_dest.to_string()
    };

    match init {
        InitSystem::Systemd => install_systemd_combined(&binary_install_script, &daemon_path),
        InitSystem::OpenRC => install_openrc_combined(&binary_install_script, &daemon_path),
        InitSystem::Runit => install_runit_combined(&binary_install_script, &daemon_path),
        InitSystem::BsdRc => install_bsd_rc_combined(&binary_install_script, &daemon_path),
        InitSystem::Unknown => Err(crate::error::HyperfanError::UnsupportedInitSystem("Unknown init system. Cannot install service.".to_string()).to_string()),
    }
}

fn install_systemd_combined(binary_script: &str, daemon_path: &str) -> Result<(), String> {
    let service_content = systemd_service(daemon_path);
    let temp_file = "/tmp/hyperfan.service";

    std::fs::write(temp_file, &service_content)
        .map_err(|e| format!("Failed to write temp file: {}", e))?;

    // Combined script: install binary (if needed) + install service + start
    let script = format!(
        r#"
        {}
        cp {} /etc/systemd/system/hyperfan.service && \
        chmod 644 /etc/systemd/system/hyperfan.service && \
        systemctl daemon-reload && \
        systemctl enable hyperfan.service && \
        systemctl start hyperfan.service
    "#,
        binary_script, temp_file
    );

    run_pkexec(&script)?;
    let _ = std::fs::remove_file(temp_file);
    Ok(())
}

fn install_openrc_combined(binary_script: &str, daemon_path: &str) -> Result<(), String> {
    let service_content = openrc_service(daemon_path);
    let temp_file = "/tmp/hyperfand";

    std::fs::write(temp_file, &service_content)
        .map_err(|e| format!("Failed to write temp file: {}", e))?;

    // Combined script: install binary (if needed) + install service + start
    let script = format!(
        r#"
        {}
        cp {} /etc/init.d/hyperfand && \
        chmod 755 /etc/init.d/hyperfand && \
        rc-update add hyperfand default && \
        rc-service hyperfand start
    "#,
        binary_script, temp_file
    );

    run_pkexec(&script)?;
    let _ = std::fs::remove_file(temp_file);
    Ok(())
}

fn install_runit_combined(binary_script: &str, daemon_path: &str) -> Result<(), String> {
    let run_script = runit_run_script(daemon_path);
    let finish_script = runit_finish_script();

    let temp_run = "/tmp/hyperfand-run";
    let temp_finish = "/tmp/hyperfand-finish";

    std::fs::write(temp_run, &run_script)
        .map_err(|e| format!("Failed to write run script: {}", e))?;
    std::fs::write(temp_finish, finish_script)
        .map_err(|e| format!("Failed to write finish script: {}", e))?;

    // Combined script: install binary (if needed) + install service
    let script = format!(
        r#"
        {}
        mkdir -p /etc/sv/hyperfand && \
        cp {} /etc/sv/hyperfand/run && \
        cp {} /etc/sv/hyperfand/finish && \
        chmod 755 /etc/sv/hyperfand/run /etc/sv/hyperfand/finish && \
        ln -sf /etc/sv/hyperfand /var/service/hyperfand
    "#,
        binary_script, temp_run, temp_finish
    );

    run_pkexec(&script)?;
    let _ = std::fs::remove_file(temp_run);
    let _ = std::fs::remove_file(temp_finish);
    Ok(())
}

fn install_bsd_rc_combined(binary_script: &str, daemon_path: &str) -> Result<(), String> {
    let rc_script = bsd_rc_script(daemon_path);
    let temp_file = "/tmp/hyperfand";

    std::fs::write(temp_file, &rc_script)
        .map_err(|e| format!("Failed to write temp file: {}", e))?;

    // BSD uses /usr/local/etc/rc.d for third-party services
    // Also enable in rc.conf
    // Combined script: install binary (if needed) + install service + start
    let script = format!(
        r#"
        {}
        cp {} /usr/local/etc/rc.d/hyperfand && \
        chmod 755 /usr/local/etc/rc.d/hyperfand && \
        grep -q 'hyperfand_enable' /etc/rc.conf || echo 'hyperfand_enable="YES"' >> /etc/rc.conf && \
        service hyperfand start
    "#,
        binary_script, temp_file
    );

    run_pkexec_bsd(&script)?;
    let _ = std::fs::remove_file(temp_file);
    Ok(())
}

// ============================================================================
// Uninstallation
// ============================================================================

/// Uninstall the daemon service (requires root via pkexec)
pub fn uninstall_service() -> Result<(), String> {
    let init = detect_init_system();

    match init {
        InitSystem::Systemd => uninstall_systemd(),
        InitSystem::OpenRC => uninstall_openrc(),
        InitSystem::Runit => uninstall_runit(),
        InitSystem::BsdRc => uninstall_bsd_rc(),
        InitSystem::Unknown => Err(crate::error::HyperfanError::UnsupportedInitSystem("Unknown init system".to_string()).to_string()),
    }
}

fn uninstall_systemd() -> Result<(), String> {
    let script = r#"
        systemctl stop hyperfan.service 2>/dev/null || true
        systemctl disable hyperfan.service 2>/dev/null || true
        rm -f /etc/systemd/system/hyperfan.service
        systemctl daemon-reload
        rm -f /run/hyperfan.sock /run/hyperfand.pid
        rm -f /usr/local/bin/hyperfand /usr/bin/hyperfand
    "#;
    run_pkexec(script)
}

fn uninstall_openrc() -> Result<(), String> {
    let script = r#"
        rc-service hyperfand stop 2>/dev/null || true
        rc-update del hyperfand default 2>/dev/null || true
        rm -f /etc/init.d/hyperfand
        rm -f /run/hyperfan.sock /run/hyperfand.pid
        rm -f /usr/local/bin/hyperfand /usr/bin/hyperfand
    "#;
    run_pkexec(script)
}

fn uninstall_runit() -> Result<(), String> {
    let script = r#"
        rm -f /var/service/hyperfand
        sv stop hyperfand 2>/dev/null || true
        rm -rf /etc/sv/hyperfand
        rm -f /run/hyperfan.sock /run/hyperfand.pid
        rm -f /usr/local/bin/hyperfand /usr/bin/hyperfand
    "#;
    run_pkexec(script)
}

fn uninstall_bsd_rc() -> Result<(), String> {
    let script = r#"
        service hyperfand stop 2>/dev/null || true
        rm -f /usr/local/etc/rc.d/hyperfand
        rm -f /etc/rc.d/hyperfand
        sed -i '' '/hyperfand_enable/d' /etc/rc.conf 2>/dev/null || true
        rm -f /var/run/hyperfan.sock /var/run/hyperfand.pid
        rm -f /usr/local/bin/hyperfand
    "#;
    run_pkexec_bsd(script)
}

// ============================================================================
// Reinstallation (Update)
// ============================================================================

/// Reinstall/update the daemon binary and restart service (requires root via pkexec)
/// This stops the service, replaces the binary, and restarts it.
pub fn reinstall_service() -> Result<(), String> {
    let init = detect_init_system();
    
    // Find the new daemon binary
    let local_daemon = find_local_daemon_binary().ok_or_else(|| {
        "Could not find hyperfand binary next to hyperfan. Build hf-daemon first.".to_string()
    })?;
    
    let dest = get_system_daemon_path();
    
    match init {
        InitSystem::Systemd => {
            let script = format!(
                r#"
                systemctl stop hyperfan.service 2>/dev/null || true
                cp '{}' '{}' && \
                chmod 755 '{}' && \
                chown root:root '{}' && \
                systemctl start hyperfan.service
            "#,
                local_daemon, dest, dest, dest
            );
            run_pkexec(&script)
        }
        InitSystem::OpenRC => {
            let script = format!(
                r#"
                rc-service hyperfand stop 2>/dev/null || true
                cp '{}' '{}' && \
                chmod 755 '{}' && \
                chown root:root '{}' && \
                rc-service hyperfand start
            "#,
                local_daemon, dest, dest, dest
            );
            run_pkexec(&script)
        }
        InitSystem::Runit => {
            let script = format!(
                r#"
                sv stop hyperfand 2>/dev/null || true
                cp '{}' '{}' && \
                chmod 755 '{}' && \
                chown root:root '{}' && \
                sv start hyperfand
            "#,
                local_daemon, dest, dest, dest
            );
            run_pkexec(&script)
        }
        InitSystem::BsdRc => {
            let script = format!(
                r#"
                service hyperfand stop 2>/dev/null || true
                cp '{}' '{}' && \
                chmod 755 '{}' && \
                chown root:wheel '{}' && \
                service hyperfand start
            "#,
                local_daemon, dest, dest, dest
            );
            run_pkexec_bsd(&script)
        }
        InitSystem::Unknown => Err("Unknown init system. Cannot reinstall service.".to_string()),
    }
}

// ============================================================================
// Helper functions
// ============================================================================

fn run_pkexec(script: &str) -> Result<(), String> {
    let output = Command::new("pkexec")
        .args(["sh", "-c", script])
        .output()
        .map_err(|e| format!("Failed to run pkexec: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        Err(crate::error::HyperfanError::PrivilegeEscalation(format!("Command failed: {} {}", stderr, stdout)).to_string())
    }
}

/// Run privileged command on BSD (uses doas or sudo)
fn run_pkexec_bsd(script: &str) -> Result<(), String> {
    // BSD systems typically use doas or sudo instead of pkexec
    // Try doas first (OpenBSD default), then sudo, then pkexec
    let elevate_cmds = [
        ("doas", vec!["sh", "-c", script]),
        ("sudo", vec!["-n", "sh", "-c", script]), // -n for non-interactive first
        ("sudo", vec!["sh", "-c", script]),
        ("pkexec", vec!["sh", "-c", script]),
    ];

    for (cmd, args) in &elevate_cmds {
        if let Ok(output) = Command::new(cmd).args(args).output() {
            if output.status.success() {
                return Ok(());
            }
            // If command exists but failed for auth reasons, try next
            if !output.status.success() && cmd == &"sudo" && args.contains(&"-n") {
                continue;
            }
        }
    }

    // Fallback: try pkexec with graphical prompt
    let output = Command::new("pkexec")
        .args(["sh", "-c", script])
        .output()
        .map_err(|e| format!("Failed to run privilege escalation: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(crate::error::HyperfanError::PrivilegeEscalation(format!("Command failed: {}", stderr)).to_string())
    }
}

/// Get service status as a human-readable string
pub fn get_service_status() -> String {
    let init = detect_init_system();

    if !is_service_installed() {
        return format!("Not installed ({})", init);
    }

    if is_service_running() {
        if is_socket_available() {
            format!("Running ({})", init)
        } else {
            format!("Running - socket pending ({})", init)
        }
    } else {
        format!("Stopped ({})", init)
    }
}

/// Start the daemon service
pub fn start_service() -> Result<(), String> {
    match detect_init_system() {
        InitSystem::Systemd => {
            run_pkexec("systemctl start hyperfan.service")
        }
        InitSystem::OpenRC => {
            run_pkexec("rc-service hyperfand start")
        }
        InitSystem::Runit => {
            run_pkexec("sv start hyperfand")
        }
        InitSystem::BsdRc => {
            run_pkexec_bsd("service hyperfand start")
        }
        InitSystem::Unknown => Err("Unknown init system".to_string()),
    }
}

/// Stop the daemon service
pub fn stop_service() -> Result<(), String> {
    match detect_init_system() {
        InitSystem::Systemd => {
            run_pkexec("systemctl stop hyperfan.service")
        }
        InitSystem::OpenRC => {
            run_pkexec("rc-service hyperfand stop")
        }
        InitSystem::Runit => {
            run_pkexec("sv stop hyperfand")
        }
        InitSystem::BsdRc => {
            run_pkexec_bsd("service hyperfand stop")
        }
        InitSystem::Unknown => Err("Unknown init system".to_string()),
    }
}

/// Restart the daemon service
pub fn restart_service() -> Result<(), String> {
    match detect_init_system() {
        InitSystem::Systemd => {
            run_pkexec("systemctl restart hyperfan.service")
        }
        InitSystem::OpenRC => {
            run_pkexec("rc-service hyperfand restart")
        }
        InitSystem::Runit => {
            run_pkexec("sv restart hyperfand")
        }
        InitSystem::BsdRc => {
            run_pkexec_bsd("service hyperfand restart")
        }
        InitSystem::Unknown => Err("Unknown init system".to_string()),
    }
}
