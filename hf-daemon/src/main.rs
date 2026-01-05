//! Hyperfan Daemon (hyperfand)
//!
//! A **hardened**, world-class privileged service for hardware fan control.
//! Communicates with unprivileged GUI via Unix domain socket.
//!
//! # Supported Platforms
//! - **Linux**: systemd, OpenRC, runit
//! - **BSD**: FreeBSD, OpenBSD, NetBSD, DragonFlyBSD (rc.d)
//!
//! # Security Model
//! - **Privilege**: Runs as root for /sys hardware access only
//! - **Socket**: Unix domain socket with owner-only permissions (0600)
//! - **Authentication**: Validates client executable path (Linux) or socket permissions (BSD)
//! - **Validation**: Strict allowlist-based path validation
//! - **Defense**: Path traversal, injection, and symlink attack prevention
//! - **Audit**: Peer credential logging (UID/GID/PID) for all operations
//! - **Limits**: Connection limits, message size limits, rate limiting
//! - **Isolation**: Restrictive umask, working directory set to /
//! - **Signals**: Graceful shutdown with resource cleanup
//!
//! # Hardening Measures
//! - Environment sanitization (clear dangerous env vars)
//! - Resource limits (RLIMIT_NOFILE, RLIMIT_CORE)
//! - Restrictive umask (0077)
//! - No core dumps in production
//! - Symlink attack prevention on socket creation
//! - Maximum message size enforcement
//! - Connection timeout enforcement
//! - Per-client rate limiting

mod server;
mod fan_control;
mod drift_protection;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{info, error, warn, debug};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Global shutdown flag for clean termination
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

// ============================================================================
// Kernel Module Loading
// ============================================================================

/// Common SuperIO/EC kernel modules that provide PWM fan control
const HWMON_MODULES: &[&str] = &[
    "nct6775",      // Nuvoton NCT6775/NCT6776/NCT6779/NCT6791/NCT6792/NCT6793/NCT6795/NCT6796/NCT6797/NCT6798
    "it87",         // ITE IT87xx SuperIO chips
    "w83627ehf",    // Winbond W83627EHF/EHG/DHG
    "f71882fg",     // Fintek F71882FG/F71889FG
    "asus-ec-sensors", // ASUS EC sensors (read-only, but useful)
];

/// Attempt to load kernel modules for hardware monitoring
/// This ensures PWM controls are available even if modules aren't loaded at boot
fn load_hwmon_modules() {
    use std::process::Command;
    
    info!("Loading hardware monitoring kernel modules...");
    
    for module in HWMON_MODULES {
        // Check if module is already loaded
        let lsmod = Command::new("lsmod")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(module))
            .unwrap_or(false);
        
        if lsmod {
            debug!("Module {} already loaded", module);
            continue;
        }
        
        // Try to load the module
        match Command::new("modprobe").arg(module).output() {
            Ok(output) => {
                if output.status.success() {
                    info!("Loaded kernel module: {}", module);
                } else {
                    // Module not available for this hardware - this is normal
                    debug!("Module {} not available: {}", module, 
                           String::from_utf8_lossy(&output.stderr).trim());
                }
            }
            Err(e) => {
                debug!("Could not run modprobe for {}: {}", module, e);
            }
        }
    }
    
    // Give modules time to initialize and create hwmon entries
    std::thread::sleep(std::time::Duration::from_millis(hf_core::constants::detection::DAEMON_STARTUP_DELAY_MS));
    
    info!("Hardware monitoring module loading complete");
}

// ============================================================================
// Platform Detection (Runtime)
// ============================================================================

/// Detect if running on BSD at runtime
fn is_bsd() -> bool {
    use std::process::Command;
    
    // Fast path: check BSD-specific paths
    if Path::new("/etc/rc.subr").exists() {
        return true;
    }
    
    // Fallback: check uname
    Command::new("uname")
        .arg("-s")
        .output()
        .map(|o| {
            let os = String::from_utf8_lossy(&o.stdout).to_lowercase();
            os.contains("bsd") || os.contains("dragonfly")
        })
        .unwrap_or(false)
}

/// Get default socket path based on detected OS
fn get_default_socket_path() -> &'static str {
    if is_bsd() {
        "/var/run/hyperfan.sock"
    } else if Path::new("/run").exists() {
        "/run/hyperfan.sock"
    } else {
        "/var/run/hyperfan.sock"
    }
}

/// Get PID file path based on detected OS
fn get_pid_file_path() -> &'static str {
    if is_bsd() {
        "/var/run/hyperfand.pid"
    } else if Path::new("/run").exists() {
        "/run/hyperfand.pid"
    } else {
        "/var/run/hyperfand.pid"
    }
}

// ============================================================================
// Security Hardening
// ============================================================================

/// Sanitize the process environment by removing dangerous variables
fn sanitize_environment() {
    const DANGEROUS_VARS: &[&str] = &[
        "LD_PRELOAD",
        "LD_LIBRARY_PATH",
        "LD_AUDIT",
        "LD_DEBUG",
        "LD_PROFILE",
        "MALLOC_CHECK_",
        "HOSTALIASES",
        "LOCALDOMAIN",
        "RES_OPTIONS",
        "TMPDIR",
        "IFS",
        "PATH", // We'll set our own
    ];
    
    for var in DANGEROUS_VARS {
        std::env::remove_var(var);
    }
    
    // Set a minimal, secure PATH
    std::env::set_var("PATH", "/usr/sbin:/usr/bin:/sbin:/bin");
    
    // Ensure locale is predictable
    std::env::set_var("LC_ALL", "C");
    std::env::set_var("LANG", "C");
    
    debug!("Environment sanitized");
}

/// Set restrictive resource limits
fn set_resource_limits() {
    // Disable core dumps (security: prevent credential/key leakage)
    set_rlimit(libc::RLIMIT_CORE as i32, 0, 0);
    
    // Limit open file descriptors (prevent fd exhaustion attacks)
    // We need: socket + connections + /sys files + logging
    set_rlimit(libc::RLIMIT_NOFILE as i32, 1024, 1024);
    
    // Limit address space (prevent memory exhaustion)
    // 256 MB should be more than enough for this daemon
    set_rlimit(libc::RLIMIT_AS as i32, 256 * 1024 * 1024, 256 * 1024 * 1024);
    
    // Limit data segment
    set_rlimit(libc::RLIMIT_DATA as i32, 64 * 1024 * 1024, 64 * 1024 * 1024);
    
    debug!("Resource limits applied");
}

fn set_rlimit(resource: i32, soft: u64, hard: u64) {
    let limit = libc::rlimit {
        rlim_cur: soft as libc::rlim_t,
        rlim_max: hard as libc::rlim_t,
    };
    // SAFETY: setrlimit is safe when called with valid resource type and properly initialized rlimit struct.
    // The resource parameter is validated to be a known RLIMIT_* constant, and the limit struct is properly initialized.
    unsafe {
        // On Linux, RLIMIT_* constants are u32, setrlimit expects __rlimit_resource_t
        #[allow(clippy::useless_conversion)]
        if libc::setrlimit(resource as libc::__rlimit_resource_t, &limit) != 0 {
            warn!("Failed to set rlimit for resource {}", resource);
        }
    }
}

/// Set restrictive umask
fn set_secure_umask() {
    // 0077 = owner has all permissions, group/other have none
    // SAFETY: umask is always safe to call - it simply sets the file creation mask for the process.
    unsafe { libc::umask(0o077) };
    debug!("Umask set to 0077");
}

/// Change to root directory (prevent directory-based attacks)
fn secure_working_directory() {
    if std::env::set_current_dir("/").is_err() {
        warn!("Could not chdir to /");
    }
    debug!("Working directory set to /");
}

/// Verify we're running as root with proper checks
fn verify_privileges() -> Result<(), &'static str> {
    // SAFETY: geteuid and getuid are always safe - they just return the process's user IDs.
    let euid = unsafe { libc::geteuid() };
    let uid = unsafe { libc::getuid() };
    
    // Must be running as root
    if euid != 0 {
        return Err("Daemon must run as root (euid=0) for hardware access");
    }
    
    // Warn if setuid (potential security issue)
    if uid != 0 && euid == 0 {
        warn!("Running as setuid root - this is not recommended");
    }
    
    info!("Running as root (uid={}, euid={})", uid, euid);
    Ok(())
}

/// Validate socket path for security
fn validate_socket_path(path: &str) -> Result<(), String> {
    let p = Path::new(path);
    
    // Must be absolute path
    if !p.is_absolute() {
        return Err("Socket path must be absolute".into());
    }
    
    // No path traversal
    if path.contains("..") {
        return Err("Socket path contains path traversal".into());
    }
    
    // No null bytes
    if path.contains('\0') {
        return Err("Socket path contains null byte".into());
    }
    
    // Must be in a safe directory
    let safe_dirs = ["/run/", "/var/run/", "/tmp/"];
    if !safe_dirs.iter().any(|d| path.starts_with(d)) {
        return Err(format!("Socket path must be under {:?}", safe_dirs));
    }
    
    // Check parent directory exists and is owned by root
    if let Some(parent) = p.parent() {
        if !parent.exists() {
            return Err(format!("Parent directory does not exist: {:?}", parent));
        }
        
        // On Unix, verify parent is owned by root
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if let Ok(meta) = parent.metadata() {
                if meta.uid() != 0 {
                    warn!("Socket parent directory not owned by root: {:?}", parent);
                }
            }
        }
    }
    
    // Check for existing file (symlink attack prevention)
    if p.exists() {
        // If it's a symlink, refuse to use it
        if p.symlink_metadata().map(|m| m.file_type().is_symlink()).unwrap_or(false) {
            return Err("Socket path is a symlink - refusing for security".into());
        }
    }
    
    Ok(())
}

// ============================================================================
// PID File Management  
// ============================================================================

/// Write PID file with secure permissions
fn write_pid_file() -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    
    let path = get_pid_file_path();
    
    // Check for stale PID file
    if Path::new(path).exists() {
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(old_pid) = content.trim().parse::<i32>() {
                // Check if process is still running
                // SAFETY: kill with signal 0 is safe - it only checks if the process exists without sending a signal.
                // The PID is validated to be a valid i32 from the PID file.
                if unsafe { libc::kill(old_pid, 0) } == 0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::AddrInUse,
                        format!("Another instance is running (PID {})", old_pid)
                    ));
                }
            }
        }
        // Stale PID file, remove it
        let _ = std::fs::remove_file(path);
    }
    
    // Create PID file with restrictive permissions (0644)
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true) // Fail if exists (race condition protection)
        .mode(0o644)
        .open(path)?;
    
    writeln!(file, "{}", std::process::id())?;
    file.sync_all()?;
    
    debug!("PID file written: {}", path);
    Ok(())
}

// ============================================================================
// Cleanup
// ============================================================================

fn cleanup(socket_path: &str) {
    debug!("Starting cleanup...");
    
    // Remove socket
    if Path::new(socket_path).exists() {
        if let Err(e) = std::fs::remove_file(socket_path) {
            warn!("Failed to remove socket: {}", e);
        }
    }
    
    // Remove PID file
    let pid_file = get_pid_file_path();
    if Path::new(pid_file).exists() {
        if let Err(e) = std::fs::remove_file(pid_file) {
            warn!("Failed to remove PID file: {}", e);
        }
    }
    
    info!("Cleanup complete");
}

// ============================================================================
// CLI
// ============================================================================

fn print_help() {
    eprintln!("hyperfand {} - Hardened Hyperfan privileged daemon", VERSION);
    eprintln!();
    eprintln!("USAGE:");
    eprintln!("    hyperfand [OPTIONS]");
    eprintln!();
    eprintln!("OPTIONS:");
    eprintln!("    -f, --foreground    Run in foreground (don't daemonize)");
    eprintln!("    -s, --socket PATH   Socket path (auto-detected per OS)");
    eprintln!("    -v, --version       Print version");
    eprintln!("    -h, --help          Print this help");
    eprintln!();
    eprintln!("ENVIRONMENT:");
    eprintln!("    HYPERFAN_LOG        Log level (trace, debug, info, warn, error)");
    eprintln!();
    eprintln!("SECURITY:");
    eprintln!("    - Runs with minimal privileges for /sys access");
    eprintln!("    - Validates all paths against strict allowlist");
    eprintln!("    - Logs all operations with peer credentials");
    eprintln!("    - Rate limits connections to prevent DoS");
}

fn print_version() {
    println!("hyperfand {}", VERSION);
}

// ============================================================================
// Main Entry Point
// ============================================================================

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // PHASE 0: Install global panic handler for crash resistance
    // This ensures panics are logged and don't silently crash the daemon
    std::panic::set_hook(Box::new(|panic_info| {
        let location = panic_info.location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown".to_string());
        
        let message = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic".to_string()
        };
        
        // Log to stderr (will be captured by journald if running as service)
        eprintln!("PANIC at {}: {}", location, message);
        eprintln!("Daemon will attempt to continue with fallback fan speeds");
        
        // Don't abort - let the panic unwind and be caught by catch_unwind if used
    }));
    
    // PHASE 1: Pre-initialization security hardening
    // These must happen before ANY other code runs
    sanitize_environment();
    set_secure_umask();
    set_resource_limits();
    secure_working_directory();
    
    // PHASE 2: Parse arguments (minimal code, no allocations if possible)
    let args: Vec<String> = std::env::args().collect();
    let mut socket_path = get_default_socket_path().to_string();
    
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_help();
                return Ok(());
            }
            "-v" | "--version" => {
                print_version();
                return Ok(());
            }
            "-f" | "--foreground" => {
                // Foreground mode is always on (no daemonization implemented)
            }
            "-s" | "--socket" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: --socket requires a path argument");
                    std::process::exit(1);
                }
                socket_path = args[i].clone();
            }
            arg => {
                eprintln!("Unknown argument: {}", arg);
                print_help();
                std::process::exit(1);
            }
        }
        i += 1;
    }

    // PHASE 3: Initialize logging to systemd journal
    let log_level = std::env::var("HYPERFAN_LOG")
        .unwrap_or_else(|_| "info".to_string());
    
    // Try to use journald first (for systemd systems), fall back to stdout
    let mut use_journald = std::path::Path::new("/run/systemd/journal/socket").exists();
    
    if use_journald {
        // Use journald for system logging
        match tracing_journald::layer() {
            Ok(journald_layer) => {
                use tracing_subscriber::prelude::*;
                tracing_subscriber::registry()
                    .with(journald_layer)
                    .with(tracing_subscriber::EnvFilter::new(&log_level))
                    .init();
            }
            Err(e) => {
                // Journald layer creation failed, fall back to stdout
                eprintln!("Failed to create journald layer: {}, falling back to stdout", e);
                use_journald = false;
                tracing_subscriber::fmt()
                    .with_target(false)
                    .with_level(true)
                    .with_env_filter(&log_level)
                    .init();
            }
        }
    } else {
        // Fallback to stdout logging (for non-systemd systems)
        tracing_subscriber::fmt()
            .with_target(false)
            .with_level(true)
            .with_env_filter(&log_level)
            .init();
    }

    info!("STARTUP: hyperfand {} starting (hardened mode)", VERSION);
    info!("STARTUP: Platform: {}", if is_bsd() { "BSD" } else { "Linux" });
    info!("STARTUP: Logging to {}", if use_journald { "systemd journal" } else { "stdout" });

    // PHASE 4: Privilege and security checks
    if let Err(e) = verify_privileges() {
        error!("{}", e);
        std::process::exit(1);
    }
    
    if let Err(e) = validate_socket_path(&socket_path) {
        error!("Invalid socket path: {}", e);
        std::process::exit(1);
    }

    // PHASE 4.5: Load hardware monitoring kernel modules (Linux only)
    if !is_bsd() {
        load_hwmon_modules();
    }

    // PHASE 5: PID file (detect other instances)
    if let Err(e) = write_pid_file() {
        error!("Could not write PID file: {}", e);
        std::process::exit(1);
    }

    // PHASE 6: Setup signal handlers
    let socket_path_clone = socket_path.clone();
    if let Err(e) = ctrlc::set_handler(move || {
        info!("SIGNAL: Received SIGINT/SIGTERM - initiating shutdown");
        SHUTDOWN.store(true, Ordering::SeqCst);
        cleanup(&socket_path_clone);
        info!("SHUTDOWN: Daemon terminated gracefully");
        std::process::exit(0);
    }) {
        warn!("Failed to set signal handler: {}. Shutdown via signals may not work cleanly.", e);
        // Continue - daemon can still be killed with SIGKILL or via other means
    }

    info!("STARTUP: Socket path: {}", socket_path);
    info!("STARTUP: PID: {}", std::process::id());
    info!("STARTUP: Log level: {}", log_level);

    // PHASE 7: Initialize fan control state
    let fan_control_state = Arc::new(fan_control::FanControlState::new());
    let shutdown_flag = Arc::new(AtomicBool::new(false));

    // PHASE 8: Start fan control loop in background
    let fan_state_clone = fan_control_state.clone();
    let shutdown_clone = shutdown_flag.clone();
    let control_handle = tokio::spawn(async move {
        fan_control::run_control_loop(fan_state_clone, shutdown_clone).await;
    });

    info!("Fan control loop started");

    // PHASE 9: Start server (passes fan_control_state for ReloadConfig)
    let result = server::run_server(&socket_path, fan_control_state.clone()).await;

    // PHASE 10: Shutdown fan control loop
    shutdown_flag.store(true, Ordering::SeqCst);
    let _ = control_handle.await;
    
    // PHASE 11: Cleanup on exit
    cleanup(&socket_path);
    
    if let Err(e) = result {
        error!("Server error: {}", e);
        std::process::exit(1);
    }

    Ok(())
}
