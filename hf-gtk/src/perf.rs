//! Performance Metrics Module
//!
//! Collects real system performance metrics for Linux and BSD.
//! Displays FPS, CPU usage, and memory usage in a status bar.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// Global flag to enable/disable performance metrics display
static PERF_ENABLED: AtomicBool = AtomicBool::new(false);

/// Check if performance metrics are enabled
pub fn is_enabled() -> bool {
    PERF_ENABLED.load(Ordering::Relaxed)
}

/// Enable performance metrics display
pub fn enable() {
    PERF_ENABLED.store(true, Ordering::Relaxed);
}

/// Performance metrics data
#[derive(Debug, Clone, Default)]
pub struct PerfMetrics {
    /// Frames per second (UI refresh rate)
    pub fps: f64,
    /// CPU usage percentage (0-100)
    pub cpu_percent: f64,
    /// Memory usage in bytes
    pub memory_used: u64,
    /// Total memory in bytes
    pub memory_total: u64,
}

impl PerfMetrics {
    /// Format memory as human-readable string
    pub fn memory_str(&self) -> String {
        let used_mb = self.memory_used as f64 / (1024.0 * 1024.0);
        let total_mb = self.memory_total as f64 / (1024.0 * 1024.0);
        format!("{:.0}/{:.0} MB", used_mb, total_mb)
    }
}

/// Performance metrics collector
/// 
/// PERFORMANCE: FPS is measured every frame, but CPU/memory are cached
/// and only updated every ~1 second to avoid blocking the UI thread with I/O.
pub struct PerfCollector {
    /// Last frame timestamp for FPS calculation
    last_frame: Instant,
    /// Frame count for averaging
    frame_count: u32,
    /// Total frame count (for throttling)
    total_frames: u32,
    /// Accumulated frame time
    frame_time_accum: f64,
    /// Current FPS
    fps: f64,
    /// Last CPU measurement time
    last_cpu_time: Instant,
    /// Previous CPU stats for delta calculation
    #[cfg(target_os = "linux")]
    prev_cpu_stats: Option<LinuxCpuStats>,
    #[cfg(any(target_os = "freebsd", target_os = "openbsd", target_os = "netbsd", target_os = "dragonfly"))]
    prev_cpu_stats: Option<BsdCpuStats>,
    #[cfg(not(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd", target_os = "netbsd", target_os = "dragonfly")))]
    prev_cpu_stats: Option<()>,
    /// Current CPU usage (cached)
    cpu_percent: f64,
    /// Cached memory values (updated every ~1sec, not every frame)
    cached_memory: (u64, u64),
    /// Last memory update time
    last_memory_time: Instant,
}

#[cfg(target_os = "linux")]
#[derive(Clone)]
struct LinuxCpuStats {
    user: u64,
    nice: u64,
    system: u64,
    idle: u64,
    iowait: u64,
    irq: u64,
    softirq: u64,
}

#[cfg(any(target_os = "freebsd", target_os = "openbsd", target_os = "netbsd", target_os = "dragonfly"))]
#[derive(Clone)]
struct BsdCpuStats {
    user: u64,
    nice: u64,
    system: u64,
    interrupt: u64,
    idle: u64,
}

impl PerfCollector {
    pub fn new() -> Self {
        Self {
            last_frame: Instant::now(),
            frame_count: 0,
            total_frames: 0,
            frame_time_accum: 0.0,
            fps: 0.0,
            last_cpu_time: Instant::now(),
            prev_cpu_stats: None,
            cpu_percent: 0.0,
            cached_memory: (0, 0),
            last_memory_time: Instant::now(),
        }
    }

    /// Record a frame tick and update FPS
    /// 
    /// PERFORMANCE: This is called every vsync. Only does lightweight
    /// timestamp math here. Heavy I/O (CPU/memory) is done infrequently.
    pub fn tick_frame(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f64();
        self.last_frame = now;

        self.frame_time_accum += dt;
        self.frame_count += 1;
        self.total_frames = self.total_frames.wrapping_add(1);

        // Update FPS every ~500ms for stable reading
        if self.frame_time_accum >= 0.5 {
            self.fps = self.frame_count as f64 / self.frame_time_accum;
            self.frame_count = 0;
            self.frame_time_accum = 0.0;
        }

        // Update CPU/memory only every ~1 second (avoid blocking UI with I/O)
        if now.duration_since(self.last_cpu_time).as_secs_f64() >= 1.0 {
            self.update_cpu();
            self.cached_memory = Self::read_memory();
            self.last_cpu_time = now;
            self.last_memory_time = now;
        }
    }

    /// Get current metrics (uses cached values - no I/O)
    pub fn get_metrics(&self) -> PerfMetrics {
        PerfMetrics {
            fps: self.fps,
            cpu_percent: self.cpu_percent,
            memory_used: self.cached_memory.0,
            memory_total: self.cached_memory.1,
        }
    }

    /// Get current frame count (for throttling label updates)
    pub fn frame_count(&self) -> u32 {
        self.total_frames
    }

    /// Update CPU usage calculation
    fn update_cpu(&mut self) {
        #[cfg(target_os = "linux")]
        {
            if let Some(stats) = Self::read_linux_cpu_stats() {
                if let Some(ref prev) = self.prev_cpu_stats {
                    let user_delta = stats.user.saturating_sub(prev.user);
                    let nice_delta = stats.nice.saturating_sub(prev.nice);
                    let system_delta = stats.system.saturating_sub(prev.system);
                    let idle_delta = stats.idle.saturating_sub(prev.idle);
                    let iowait_delta = stats.iowait.saturating_sub(prev.iowait);
                    let irq_delta = stats.irq.saturating_sub(prev.irq);
                    let softirq_delta = stats.softirq.saturating_sub(prev.softirq);

                    let total = user_delta + nice_delta + system_delta + idle_delta 
                              + iowait_delta + irq_delta + softirq_delta;
                    let active = total - idle_delta - iowait_delta;

                    if total > 0 {
                        self.cpu_percent = (active as f64 / total as f64) * 100.0;
                    }
                }
                self.prev_cpu_stats = Some(stats);
            }
        }

        #[cfg(any(target_os = "freebsd", target_os = "openbsd", target_os = "netbsd", target_os = "dragonfly"))]
        {
            if let Some(stats) = Self::read_bsd_cpu_stats() {
                if let Some(ref prev) = self.prev_cpu_stats {
                    let user_delta = stats.user.saturating_sub(prev.user);
                    let nice_delta = stats.nice.saturating_sub(prev.nice);
                    let system_delta = stats.system.saturating_sub(prev.system);
                    let interrupt_delta = stats.interrupt.saturating_sub(prev.interrupt);
                    let idle_delta = stats.idle.saturating_sub(prev.idle);

                    let total = user_delta + nice_delta + system_delta + interrupt_delta + idle_delta;
                    let active = total - idle_delta;

                    if total > 0 {
                        self.cpu_percent = (active as f64 / total as f64) * 100.0;
                    }
                }
                self.prev_cpu_stats = Some(stats);
            }
        }

        #[cfg(not(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd", target_os = "netbsd", target_os = "dragonfly")))]
        {
            self.cpu_percent = 0.0;
        }
    }

    #[cfg(target_os = "linux")]
    fn read_linux_cpu_stats() -> Option<LinuxCpuStats> {
        let content = std::fs::read_to_string("/proc/stat").ok()?;
        let line = content.lines().next()?;
        
        if !line.starts_with("cpu ") {
            return None;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 8 {
            return None;
        }

        Some(LinuxCpuStats {
            user: parts[1].parse().ok()?,
            nice: parts[2].parse().ok()?,
            system: parts[3].parse().ok()?,
            idle: parts[4].parse().ok()?,
            iowait: parts[5].parse().ok()?,
            irq: parts[6].parse().ok()?,
            softirq: parts[7].parse().ok()?,
        })
    }

    #[cfg(any(target_os = "freebsd", target_os = "openbsd", target_os = "netbsd", target_os = "dragonfly"))]
    fn read_bsd_cpu_stats() -> Option<BsdCpuStats> {
        // Use sysctl to get CPU times
        // kern.cp_time returns: user, nice, system, interrupt, idle
        use std::process::Command;

        let output = Command::new("sysctl")
            .args(["-n", "kern.cp_time"])
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let parts: Vec<&str> = stdout.trim().split_whitespace().collect();
        
        if parts.len() < 5 {
            return None;
        }

        Some(BsdCpuStats {
            user: parts[0].parse().ok()?,
            nice: parts[1].parse().ok()?,
            system: parts[2].parse().ok()?,
            interrupt: parts[3].parse().ok()?,
            idle: parts[4].parse().ok()?,
        })
    }

    /// Read memory usage
    fn read_memory() -> (u64, u64) {
        #[cfg(target_os = "linux")]
        {
            Self::read_linux_memory().unwrap_or((0, 0))
        }

        #[cfg(any(target_os = "freebsd", target_os = "openbsd", target_os = "netbsd", target_os = "dragonfly"))]
        {
            Self::read_bsd_memory().unwrap_or((0, 0))
        }

        #[cfg(not(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd", target_os = "netbsd", target_os = "dragonfly")))]
        {
            (0, 0)
        }
    }

    #[cfg(target_os = "linux")]
    fn read_linux_memory() -> Option<(u64, u64)> {
        let content = std::fs::read_to_string("/proc/meminfo").ok()?;
        
        let mut total: u64 = 0;
        let mut available: u64 = 0;

        for line in content.lines() {
            if line.starts_with("MemTotal:") {
                total = Self::parse_meminfo_line(line)?;
            } else if line.starts_with("MemAvailable:") {
                available = Self::parse_meminfo_line(line)?;
            }
        }

        if total > 0 {
            let used = total.saturating_sub(available);
            Some((used, total))
        } else {
            None
        }
    }

    #[cfg(target_os = "linux")]
    fn parse_meminfo_line(line: &str) -> Option<u64> {
        // Format: "MemTotal:       16384000 kB"
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            let kb: u64 = parts[1].parse().ok()?;
            Some(kb * 1024) // Convert to bytes
        } else {
            None
        }
    }

    #[cfg(any(target_os = "freebsd", target_os = "openbsd", target_os = "netbsd", target_os = "dragonfly"))]
    fn read_bsd_memory() -> Option<(u64, u64)> {
        use std::process::Command;

        // Get total physical memory
        let total_output = Command::new("sysctl")
            .args(["-n", "hw.physmem"])
            .output()
            .ok()?;

        let total: u64 = String::from_utf8_lossy(&total_output.stdout)
            .trim()
            .parse()
            .ok()?;

        // Get page size
        let pagesize_output = Command::new("sysctl")
            .args(["-n", "hw.pagesize"])
            .output()
            .ok()?;

        let pagesize: u64 = String::from_utf8_lossy(&pagesize_output.stdout)
            .trim()
            .parse()
            .ok()?;

        // Get free page count (method varies by BSD variant)
        #[cfg(target_os = "freebsd")]
        let free_pages_output = Command::new("sysctl")
            .args(["-n", "vm.stats.vm.v_free_count"])
            .output()
            .ok()?;

        #[cfg(any(target_os = "openbsd", target_os = "netbsd", target_os = "dragonfly"))]
        let free_pages_output = Command::new("sysctl")
            .args(["-n", "vm.uvmexp.free"])
            .output()
            .ok()?;

        let free_pages: u64 = String::from_utf8_lossy(&free_pages_output.stdout)
            .trim()
            .parse()
            .unwrap_or(0);

        let free = free_pages * pagesize;
        let used = total.saturating_sub(free);

        Some((used, total))
    }
}

impl Default for PerfCollector {
    fn default() -> Self {
        Self::new()
    }
}
