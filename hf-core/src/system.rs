//! System information gathering
//!
//! Cross-platform support for Linux and BSD systems.
//! - Linux: Uses /proc and /sys filesystems
//! - BSD: Uses sysctl and kenv commands

use crate::error::Result;
use std::fs;
use std::process::Command;
use std::sync::OnceLock;

use crate::data::SystemSummary;

/// Kilobytes per megabyte for memory conversion
const KB_PER_MB: u64 = 1024;
/// Bytes per megabyte
const BYTES_PER_MB: u64 = 1024 * 1024;

/// PERFORMANCE: Cache static system info (hostname, CPU, etc.) - these never change
/// Only memory_available_mb is dynamic and needs refreshing
static CACHED_STATIC_INFO: OnceLock<CachedStaticInfo> = OnceLock::new();

#[derive(Clone)]
struct CachedStaticInfo {
    hostname: String,
    kernel_version: String,
    cpu_model: String,
    cpu_cores: u32,
    memory_total_mb: u32,
    motherboard_name: String,
}

fn get_cached_static_info() -> &'static CachedStaticInfo {
    CACHED_STATIC_INFO.get_or_init(|| CachedStaticInfo {
        hostname: read_hostname(),
        kernel_version: read_kernel_version(),
        cpu_model: read_cpu_name(),
        cpu_cores: read_cpu_cores(),
        memory_total_mb: read_memory_total(),
        motherboard_name: read_mb_name(),
    })
}

/// Gather a summary of system hardware and OS information
/// Works on both Linux and BSD systems
/// PERFORMANCE: Uses cached static info, only reads dynamic memory available
pub fn get_system_summary() -> Result<SystemSummary> {
    let static_info = get_cached_static_info();
    Ok(SystemSummary {
        hostname: static_info.hostname.clone(),
        kernel_version: static_info.kernel_version.clone(),
        cpu_model: static_info.cpu_model.clone(),
        cpu_cores: static_info.cpu_cores,
        memory_total_mb: static_info.memory_total_mb,
        memory_available_mb: read_memory_available(static_info.memory_total_mb),
        motherboard_name: static_info.motherboard_name.clone(),
    })
}

/// Fast memory-only read for frequent updates
/// PERFORMANCE: Only reads /proc/meminfo, no subprocess spawning
pub fn get_memory_available_mb() -> u32 {
    let static_info = get_cached_static_info();
    read_memory_available(static_info.memory_total_mb)
}

/// Get total memory (cached)
pub fn get_memory_total_mb() -> u32 {
    get_cached_static_info().memory_total_mb
}

// ============================================================================
// CPU Information
// ============================================================================

/// Read CPU model name
fn read_cpu_name() -> String {
    // Try Linux /proc/cpuinfo first
    if let Ok(cpuinfo) = fs::read_to_string("/proc/cpuinfo") {
        for line in cpuinfo.lines() {
            if line.to_ascii_lowercase().starts_with("model name") {
                if let Some((_, model_name)) = line.split_once(':') {
                    return model_name.trim().to_string();
                }
            }
        }
    }
    
    // BSD: use sysctl hw.model
    if let Ok(output) = Command::new("sysctl").args(["-n", "hw.model"]).output() {
        if output.status.success() {
            let model = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !model.is_empty() {
                return model;
            }
        }
    }
    
    // macOS fallback
    if let Ok(output) = Command::new("sysctl").args(["-n", "machdep.cpu.brand_string"]).output() {
        if output.status.success() {
            let model = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !model.is_empty() {
                return model;
            }
        }
    }
    
    "Unknown CPU".to_string()
}

/// Count CPU cores/threads
fn read_cpu_cores() -> u32 {
    // Try Linux /proc/cpuinfo first
    if let Ok(cpuinfo) = fs::read_to_string("/proc/cpuinfo") {
        let count = cpuinfo
            .lines()
            .filter(|line| line.trim_start().starts_with("processor"))
            .count();
        if count > 0 {
            return count as u32;
        }
    }
    
    // BSD/macOS: use sysctl hw.ncpu
    if let Ok(output) = Command::new("sysctl").args(["-n", "hw.ncpu"]).output() {
        if output.status.success() {
            if let Ok(cores) = String::from_utf8_lossy(&output.stdout).trim().parse::<u32>() {
                return cores;
            }
        }
    }
    
    1 // Assume at least one core
}

// ============================================================================
// Memory Information
// ============================================================================

/// Read total memory only (for caching - this never changes)
fn read_memory_total() -> u32 {
    // Try Linux /proc/meminfo first
    if let Ok(meminfo) = fs::read_to_string("/proc/meminfo") {
        for line in meminfo.lines() {
            if line.starts_with("MemTotal:") {
                let total_kb = parse_meminfo_value(line);
                if total_kb > 0 {
                    return (total_kb / KB_PER_MB) as u32;
                }
            }
        }
    }
    
    // BSD: use sysctl
    let total = sysctl_bytes("hw.physmem")
        .or_else(|| sysctl_bytes("hw.memsize")) // macOS
        .unwrap_or(0);
    
    (total / BYTES_PER_MB) as u32
}

/// Read available memory only (fast path for frequent updates)
/// PERFORMANCE: Only reads /proc/meminfo on Linux, avoids subprocess on BSD when possible
fn read_memory_available(total_mb: u32) -> u32 {
    // Try Linux /proc/meminfo first (fast - just file read)
    if let Ok(meminfo) = fs::read_to_string("/proc/meminfo") {
        for line in meminfo.lines() {
            if line.starts_with("MemAvailable:") {
                let available_kb = parse_meminfo_value(line);
                return (available_kb / KB_PER_MB) as u32;
            }
        }
    }
    
    // BSD: use sysctl (slower - spawns subprocess)
    if let (Some(free_pages), Some(page_size)) = (
        sysctl_u64("vm.stats.vm.v_free_count"),
        sysctl_u64("hw.pagesize")
    ) {
        return ((free_pages * page_size) / BYTES_PER_MB) as u32;
    }
    
    // Fallback: estimate 50% available
    total_mb / 2
}

/// Parse a meminfo line like "MemTotal:       16384000 kB"
fn parse_meminfo_value(line: &str) -> u64 {
    line.split_whitespace()
        .nth(1)
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0)
}

/// Get a sysctl value as bytes (for memory)
fn sysctl_bytes(key: &str) -> Option<u64> {
    Command::new("sysctl").args(["-n", key]).output().ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u64>().ok())
}

/// Get a sysctl value as u64
fn sysctl_u64(key: &str) -> Option<u64> {
    sysctl_bytes(key)
}

// ============================================================================
// System Information
// ============================================================================

/// Read system hostname
fn read_hostname() -> String {
    // Try Linux procfs
    if let Ok(hostname) = fs::read_to_string("/proc/sys/kernel/hostname") {
        let h = hostname.trim();
        if !h.is_empty() {
            return h.to_string();
        }
    }
    
    // Try /etc/hostname (works on both Linux and BSD)
    if let Ok(hostname) = fs::read_to_string("/etc/hostname") {
        let h = hostname.trim();
        if !h.is_empty() {
            return h.to_string();
        }
    }
    
    // BSD: use sysctl
    if let Ok(output) = Command::new("sysctl").args(["-n", "kern.hostname"]).output() {
        if output.status.success() {
            let h = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !h.is_empty() {
                return h;
            }
        }
    }
    
    // Last resort: hostname command
    if let Ok(output) = Command::new("hostname").output() {
        if output.status.success() {
            return String::from_utf8_lossy(&output.stdout).trim().to_string();
        }
    }
    
    "unknown-host".to_string()
}

/// Read kernel version string
fn read_kernel_version() -> String {
    // Try Linux procfs
    if let Ok(release) = fs::read_to_string("/proc/sys/kernel/osrelease") {
        let r = release.trim();
        if !r.is_empty() {
            return r.to_string();
        }
    }
    
    // BSD: use sysctl
    if let Ok(output) = Command::new("sysctl").args(["-n", "kern.osrelease"]).output() {
        if output.status.success() {
            let r = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !r.is_empty() {
                return r;
            }
        }
    }
    
    // Fallback: uname -r
    if let Ok(output) = Command::new("uname").args(["-r"]).output() {
        if output.status.success() {
            return String::from_utf8_lossy(&output.stdout).trim().to_string();
        }
    }
    
    "unknown-kernel".to_string()
}

/// Read motherboard name from DMI/SMBIOS
fn read_mb_name() -> String {
    // Try Linux sysfs DMI
    let board_vendor = fs::read_to_string("/sys/devices/virtual/dmi/id/board_vendor")
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let board_name = fs::read_to_string("/sys/devices/virtual/dmi/id/board_name")
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let product_name = fs::read_to_string("/sys/devices/virtual/dmi/id/product_name")
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    
    if !board_vendor.is_empty() || !board_name.is_empty() {
        let combined = format!("{} {}", board_vendor, board_name).trim().to_string();
        if !combined.is_empty() {
            return combined;
        }
    }
    
    if !product_name.is_empty() {
        return product_name;
    }
    
    // BSD: use kenv for SMBIOS data
    if let Ok(output) = Command::new("kenv").args(["smbios.planar.maker"]).output() {
        if output.status.success() {
            let vendor = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if let Ok(output2) = Command::new("kenv").args(["smbios.planar.product"]).output() {
                if output2.status.success() {
                    let product = String::from_utf8_lossy(&output2.stdout).trim().to_string();
                    let combined = format!("{} {}", vendor, product).trim().to_string();
                    if !combined.is_empty() {
                        return combined;
                    }
                }
            }
        }
    }
    
    // BSD fallback: smbios.system.product
    if let Ok(output) = Command::new("kenv").args(["smbios.system.product"]).output() {
        if output.status.success() {
            let p = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !p.is_empty() {
                return p;
            }
        }
    }
    
    // macOS: use system_profiler (expensive, so last resort)
    #[cfg(target_os = "macos")]
    if let Ok(output) = Command::new("system_profiler")
        .args(["SPHardwareDataType", "-json"])
        .output()
    {
        if output.status.success() {
            // Parse JSON for model name - simplified
            let s = String::from_utf8_lossy(&output.stdout);
            if let Some(pos) = s.find("\"model_name\"") {
                if let Some(start) = s[pos..].find(": \"") {
                    if let Some(end) = s[pos + start + 3..].find('"') {
                        return s[pos + start + 3..pos + start + 3 + end].to_string();
                    }
                }
            }
        }
    }
    
    "Unknown Motherboard".to_string()
}

// ============================================================================
// Platform Detection
// ============================================================================

/// Get the current operating system name
pub fn get_os_name() -> &'static str {
    #[cfg(target_os = "linux")]
    { "Linux" }
    #[cfg(target_os = "freebsd")]
    { "FreeBSD" }
    #[cfg(target_os = "openbsd")]
    { "OpenBSD" }
    #[cfg(target_os = "netbsd")]
    { "NetBSD" }
    #[cfg(target_os = "dragonfly")]
    { "DragonFly BSD" }
    #[cfg(target_os = "macos")]
    { "macOS" }
    #[cfg(not(any(
        target_os = "linux",
        target_os = "freebsd",
        target_os = "openbsd", 
        target_os = "netbsd",
        target_os = "dragonfly",
        target_os = "macos"
    )))]
    { "Unknown" }
}

/// Check if running on a Linux system
pub fn is_linux() -> bool {
    cfg!(target_os = "linux")
}

/// Check if running on a BSD system
pub fn is_bsd() -> bool {
    cfg!(any(
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))
}
