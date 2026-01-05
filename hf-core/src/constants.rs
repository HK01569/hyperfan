//! Constants and configuration values for Hyperfan
//!
//! Centralizes all magic numbers, paths, and configuration defaults.
//! This is the SINGLE SOURCE OF TRUTH for all configuration values.
//! Never use magic numbers in other files - add them here first.

use std::time::Duration;

/// System paths - supports both Linux and BSD
pub mod paths {

    /// Base path for hwmon devices
    /// Linux: /sys/class/hwmon
    /// FreeBSD: uses sysctl (handled separately)
    #[cfg(target_os = "linux")]
    pub const HWMON_BASE: &str = "/sys/class/hwmon";
    
    #[cfg(target_os = "freebsd")]
    pub const HWMON_BASE: &str = "/dev/sysctl"; // BSD uses sysctl, not sysfs
    
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    pub const HWMON_BASE: &str = "/sys/class/hwmon"; // Default to Linux-style

    /// Configuration directory
    pub const CONFIG_DIR: &str = "/etc/hyperfan";

    /// Profile configuration file
    pub const PROFILE_FILE: &str = "profile.json";

    /// User configuration directory - works on both Linux and BSD
    /// Handles the case where daemon runs as root but needs to access user's config
    /// Uses SUDO_USER/PKEXEC_UID to find the original user when running elevated
    pub fn user_config_dir() -> Option<std::path::PathBuf> {
        // When running as root (daemon), we need to find the actual user's config
        // Check for SUDO_USER or PKEXEC_UID to get the original user
        let config_base = if let Ok(sudo_user) = std::env::var("SUDO_USER") {
            // Running via sudo - get the original user's home
            get_user_home(&sudo_user).map(|h| h.join(".config"))
        } else if let Ok(pkexec_uid) = std::env::var("PKEXEC_UID") {
            // Running via pkexec - get user by UID
            if let Ok(uid) = pkexec_uid.parse::<u32>() {
                get_home_by_uid(uid).map(|h| h.join(".config"))
            } else {
                None
            }
        // SAFETY: geteuid is always safe - it just returns the effective user ID of the process.
        } else if unsafe { libc::geteuid() } == 0 {
            // Running as root without SUDO_USER/PKEXEC_UID
            // Try to find a non-root user's config (first user with UID >= 1000)
            find_first_user_config()
        } else {
            None
        };
        
        // Fall back to standard methods if above didn't work
        let config_base = config_base.or_else(|| {
            if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
                Some(std::path::PathBuf::from(xdg))
            } else if let Ok(home) = std::env::var("HOME") {
                Some(std::path::PathBuf::from(home).join(".config"))
            } else {
                dirs::config_dir()
            }
        });
        
        config_base.map(|p| p.join("hyperfan"))
    }
    
    /// Get home directory for a username
    fn get_user_home(username: &str) -> Option<std::path::PathBuf> {
        // Read /etc/passwd to find user's home directory
        if let Ok(passwd) = std::fs::read_to_string("/etc/passwd") {
            for line in passwd.lines() {
                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() >= 6 && parts[0] == username {
                    return Some(std::path::PathBuf::from(parts[5]));
                }
            }
        }
        None
    }
    
    /// Get home directory by UID
    fn get_home_by_uid(uid: u32) -> Option<std::path::PathBuf> {
        if let Ok(passwd) = std::fs::read_to_string("/etc/passwd") {
            for line in passwd.lines() {
                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() >= 6 {
                    if let Ok(line_uid) = parts[2].parse::<u32>() {
                        if line_uid == uid {
                            return Some(std::path::PathBuf::from(parts[5]));
                        }
                    }
                }
            }
        }
        None
    }
    
    /// Find the first regular user's config directory (UID >= 1000)
    /// Returns the config base path (e.g., /home/user/.config) if hyperfan config exists
    /// 
    /// Strategy:
    /// 1. First, look for users with existing hyperfan config (settings.json OR curves.json)
    /// 2. If none found, look for the currently logged-in user via /run/user/<uid>
    /// 3. Fall back to first regular user with a home directory
    fn find_first_user_config() -> Option<std::path::PathBuf> {
        use tracing::{debug, info};
        
        let passwd = match std::fs::read_to_string("/etc/passwd") {
            Ok(p) => p,
            Err(e) => {
                debug!("Failed to read /etc/passwd: {}", e);
                return None;
            }
        };
        
        // Collect all regular users (UID >= 1000)
        let mut users: Vec<(String, u32, std::path::PathBuf)> = Vec::new();
        for line in passwd.lines() {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 6 {
                if let Ok(uid) = parts[2].parse::<u32>() {
                    if uid >= 1000 && uid < 65534 {
                        let username = parts[0].to_string();
                        let home = std::path::PathBuf::from(parts[5]);
                        if home.exists() {
                            users.push((username, uid, home));
                        }
                    }
                }
            }
        }
        
        if users.is_empty() {
            debug!("No regular users found in /etc/passwd");
            return None;
        }
        
        info!("Found {} regular users, searching for hyperfan config", users.len());
        
        // Strategy 1: HIGHEST PRIORITY - Find currently logged-in user via /run/user/<uid>
        // The active user's config takes precedence over all others
        for (username, uid, home) in &users {
            let run_user = std::path::PathBuf::from(format!("/run/user/{}", uid));
            if run_user.exists() {
                info!("Found logged-in user {} (UID {}) via /run/user - using their config (HIGHEST PRIORITY): {:?}", 
                      username, uid, home.join(".config"));
                return Some(home.join(".config"));
            }
        }
        
        // Strategy 2: Find user with existing hyperfan config (settings.json OR curves.json)
        for (username, uid, home) in &users {
            let hyperfan_dir = home.join(".config").join("hyperfan");
            let settings_path = hyperfan_dir.join("settings.json");
            let curves_path = hyperfan_dir.join("curves.json");
            
            debug!("Checking user {} (UID {}) for config at {:?}", username, uid, hyperfan_dir);
            
            // Check for either settings.json or curves.json (user may have created curves first)
            if settings_path.exists() || curves_path.exists() {
                info!("Found hyperfan config for user {} at {:?} (settings={}, curves={})", 
                      username, hyperfan_dir, settings_path.exists(), curves_path.exists());
                return Some(home.join(".config"));
            }
        }
        
        // Strategy 3: Fall back to first regular user
        if let Some((username, uid, home)) = users.first() {
            info!("No logged-in user found, falling back to first user {} (UID {}): {:?}", 
                  username, uid, home.join(".config"));
            return Some(home.join(".config"));
        }
        
        debug!("No suitable user config found");
        None
    }
    
    /// Get the resolved config directory path (for logging/debugging)
    pub fn get_resolved_config_path() -> Option<std::path::PathBuf> {
        user_config_dir()
    }

    /// DMI/SMBIOS paths for system information
    pub mod dmi {
        #[cfg(target_os = "linux")]
        pub const BOARD_VENDOR: &str = "/sys/devices/virtual/dmi/id/board_vendor";
        #[cfg(target_os = "linux")]
        pub const BOARD_NAME: &str = "/sys/devices/virtual/dmi/id/board_name";
        #[cfg(target_os = "linux")]
        pub const PRODUCT_NAME: &str = "/sys/devices/virtual/dmi/id/product_name";
        
        // BSD uses kenv or sysctl for DMI info
        #[cfg(target_os = "freebsd")]
        pub const BOARD_VENDOR: &str = "smbios.planar.maker";
        #[cfg(target_os = "freebsd")]
        pub const BOARD_NAME: &str = "smbios.planar.product";
        #[cfg(target_os = "freebsd")]
        pub const PRODUCT_NAME: &str = "smbios.system.product";
        
        #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
        pub const BOARD_VENDOR: &str = "";
        #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
        pub const BOARD_NAME: &str = "";
        #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
        pub const PRODUCT_NAME: &str = "";
    }

    /// System info paths
    pub mod proc {
        #[cfg(target_os = "linux")]
        pub const CPUINFO: &str = "/proc/cpuinfo";
        #[cfg(target_os = "linux")]
        pub const MEMINFO: &str = "/proc/meminfo";
        #[cfg(target_os = "linux")]
        pub const HOSTNAME: &str = "/proc/sys/kernel/hostname";
        #[cfg(target_os = "linux")]
        pub const KERNEL_RELEASE: &str = "/proc/sys/kernel/osrelease";
        #[cfg(target_os = "linux")]
        pub const VERSION: &str = "/proc/version";
        
        // BSD doesn't use procfs by default - use sysctl instead
        #[cfg(target_os = "freebsd")]
        pub const CPUINFO: &str = ""; // Use sysctl hw.model
        #[cfg(target_os = "freebsd")]
        pub const MEMINFO: &str = ""; // Use sysctl hw.physmem
        #[cfg(target_os = "freebsd")]
        pub const HOSTNAME: &str = ""; // Use sysctl kern.hostname
        #[cfg(target_os = "freebsd")]
        pub const KERNEL_RELEASE: &str = ""; // Use sysctl kern.osrelease
        #[cfg(target_os = "freebsd")]
        pub const VERSION: &str = ""; // Use sysctl kern.version
        
        #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
        pub const CPUINFO: &str = "";
        #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
        pub const MEMINFO: &str = "";
        #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
        pub const HOSTNAME: &str = "";
        #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
        pub const KERNEL_RELEASE: &str = "";
        #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
        pub const VERSION: &str = "";
    }
    
    /// Check if running on Linux
    pub fn is_linux() -> bool {
        cfg!(target_os = "linux")
    }
    
    /// Check if running on BSD
    pub fn is_bsd() -> bool {
        cfg!(any(target_os = "freebsd", target_os = "openbsd", target_os = "netbsd", target_os = "dragonfly"))
    }
}

/// PWM control constants
pub mod pwm {
    /// Minimum PWM value (fan off or minimum speed)
    pub const MIN_VALUE: u8 = 0;

    /// Maximum PWM value (full speed)
    pub const MAX_VALUE: u8 = 255;

    /// PWM enable values
    pub mod enable {
        /// PWM control disabled
        pub const DISABLED: u8 = 0;
        /// Manual PWM control
        pub const MANUAL: u8 = 1;
        /// Automatic/thermal control
        pub const AUTOMATIC: u8 = 2;
    }

    /// Convert percentage (0-100) to PWM value (0-255)
    #[inline]
    pub fn from_percent(percent: f32) -> u8 {
        ((percent.clamp(0.0, 100.0) / 100.0) * 255.0).round() as u8
    }

    /// Convert PWM value (0-255) to percentage (0-100)
    #[inline]
    pub fn to_percent(value: u8) -> f32 {
        (value as f32 / 255.0) * 100.0
    }
}

/// Temperature constants
pub mod temperature {
    /// Temperature readings are in millidegrees, divide by this to get Celsius
    pub const MILLIDEGREE_DIVISOR: f32 = 1000.0;

    /// Critical temperature threshold (Celsius)
    pub const CRITICAL_THRESHOLD: f32 = 95.0;

    /// High temperature threshold (Celsius)
    pub const HIGH_THRESHOLD: f32 = 80.0;

    /// Target temperature for balanced operation (Celsius)
    pub const TARGET_THRESHOLD: f32 = 70.0;

    /// Low temperature threshold (Celsius)
    pub const LOW_THRESHOLD: f32 = 45.0;
}

/// Timing constants for detection and control
pub mod timing {
    use super::*;

    /// Time to wait for fan to reach stable speed after PWM change
    pub const FAN_STABILIZATION: Duration = Duration::from_millis(3000);
    
    /// Fan stabilization time in milliseconds (for use in FanMapping)
    pub const FAN_STABILIZATION_MS: u32 = 3000;

    /// Time to wait for fan to spin up from stop
    pub const FAN_SPINUP: Duration = Duration::from_millis(4000);

    /// Brief delay between detection tests
    pub const DETECTION_DELAY: Duration = Duration::from_millis(1000);

    /// Polling interval for fan curve updates
    pub const CURVE_UPDATE_INTERVAL: Duration = Duration::from_millis(1000);

    /// Minimum interval between consecutive PWM writes
    pub const PWM_WRITE_DEBOUNCE: Duration = Duration::from_millis(100);
}

/// Detection algorithm parameters
pub mod detection {
    /// Minimum RPM drop to consider a fan affected by PWM change (absolute value)
    pub const MIN_RPM_DROP_ABSOLUTE: i32 = 50;
    
    /// Minimum RPM drop to consider a fan responsive during detection (higher threshold)
    pub const MIN_RPM_DROP: i32 = 800;

    /// Minimum confidence to consider a valid PWM-fan mapping
    pub const MIN_CONFIDENCE: f32 = 0.6;

    /// High confidence threshold
    pub const HIGH_CONFIDENCE: f32 = 0.8;
    
    /// Daemon startup delay for hardware module initialization (milliseconds)
    pub const DAEMON_STARTUP_DELAY_MS: u64 = 500;

    /// Confidence scores assigned based on RPM drop percentage
    pub mod confidence_scores {
        /// Fan dropped >80% RPM - very likely controlled by this PWM
        pub const VERY_HIGH_DROP: f32 = 0.95;
        /// Fan dropped >60% RPM - probably controlled by this PWM
        pub const HIGH_DROP: f32 = 0.85;
        /// Fan dropped >40% RPM - likely controlled by this PWM
        pub const MEDIUM_DROP: f32 = 0.75;
        /// Fan dropped >20% RPM - possibly controlled by this PWM
        pub const LOW_DROP: f32 = 0.65;
        /// Fan dropped >10% RPM - weak correlation
        pub const MINIMAL_DROP: f32 = 0.45;
        /// Fan dropped >50 RPM but <10% - very weak correlation
        pub const ABSOLUTE_DROP: f32 = 0.25;
    }

    /// RPM drop percentage thresholds for confidence calculation
    pub mod rpm_drop_thresholds {
        /// Percentage drop indicating very high confidence
        pub const VERY_HIGH: f32 = 80.0;
        /// Percentage drop indicating high confidence
        pub const HIGH: f32 = 60.0;
        /// Percentage drop indicating medium confidence
        pub const MEDIUM: f32 = 40.0;
        /// Percentage drop indicating low confidence
        pub const LOW: f32 = 20.0;
        /// Percentage drop indicating minimal confidence
        pub const MINIMAL: f32 = 10.0;
    }

    /// Heuristic matching confidence values
    pub mod heuristic {
        /// Confidence when fan/PWM indices match
        pub const INDEX_MATCH_BASE: f32 = 0.5;
        /// Confidence when doing positional matching (no index info)
        pub const POSITION_MATCH_BASE: f32 = 0.3;
        /// Maximum confidence cap for index matching
        pub const INDEX_MATCH_CAP: f32 = 0.9;
        /// Maximum confidence cap for position matching
        pub const POSITION_MATCH_CAP: f32 = 0.7;
        /// Bonus for labels with 3+ common prefix characters
        pub const LABEL_MATCH_STRONG: f32 = 0.15;
        /// Bonus for labels with 2 common prefix characters
        pub const LABEL_MATCH_WEAK: f32 = 0.08;
    }
}

/// File size limits for security
pub mod limits {
    /// Maximum profile config file size (1MB)
    pub const MAX_PROFILE_SIZE: u64 = 1024 * 1024;

    /// Maximum number of fan mappings
    pub const MAX_MAPPINGS: usize = 32;

    /// Maximum number of curve points
    pub const MAX_CURVE_POINTS: usize = 16;

    /// Maximum sensor name length
    pub const MAX_SENSOR_NAME_LEN: usize = 128;

    /// Maximum valid temperature for curve points (°C)
    pub const MAX_CURVE_TEMPERATURE: f32 = 150.0;

    /// Minimum label prefix length for "strong" match
    pub const LABEL_PREFIX_STRONG: usize = 3;

    /// Minimum label prefix length for "weak" match
    pub const LABEL_PREFIX_WEAK: usize = 2;
}

/// Fan curve algorithm parameters
pub mod curve {
    /// Default hysteresis in degrees Celsius
    /// Prevents rapid fan speed oscillation near threshold temperatures
    pub const DEFAULT_HYSTERESIS_CELSIUS: f32 = 2.0;

    /// Default smoothing factor (0.0 = no smoothing, 0.99 = very smooth)
    /// Higher values make fan speed changes more gradual
    pub const DEFAULT_SMOOTHING_FACTOR: f32 = 0.3;

    /// Minimum smoothing factor allowed
    pub const MIN_SMOOTHING: f32 = 0.0;

    /// Maximum smoothing factor allowed
    pub const MAX_SMOOTHING: f32 = 0.99;

    /// Epsilon for floating-point comparisons
    pub const FLOAT_EPSILON: f32 = 0.001;

    /// Fallback fan speed when curve is empty or invalid (100% for safety)
    pub const FALLBACK_FAN_PERCENT: f32 = 100.0;
    
    /// Default delay in milliseconds before responding to temperature changes
    /// Helps filter out brief temperature spikes
    pub const DEFAULT_DELAY_MS: u32 = 0;
    
    /// Default ramp up speed in percent per second
    /// How fast the fan speeds up when temperature rises (0 = instant)
    pub const DEFAULT_RAMP_UP_SPEED: f32 = 50.0;
    
    /// Default ramp down speed in percent per second
    /// How fast the fan slows down when temperature drops (0 = instant)
    pub const DEFAULT_RAMP_DOWN_SPEED: f32 = 25.0;
    
    /// Maximum hysteresis value in degrees Celsius
    pub const MAX_HYSTERESIS_CELSIUS: f32 = 10.0;
    
    /// Maximum delay in milliseconds
    pub const MAX_DELAY_MS: u32 = 10000;
    
    /// Maximum ramp speed in percent per second
    pub const MAX_RAMP_SPEED: f32 = 200.0;
    
    /// Minimum ramp speed in percent per second (0 = instant)
    pub const MIN_RAMP_SPEED: f32 = 0.0;
}

// GPU-related constants have been moved to hf-gpu crate

/// Fingerprint validation thresholds
pub mod fingerprint {
    /// Confidence threshold for ValidationState::Ok
    pub const CONFIDENCE_OK: f32 = 0.90;
    /// Confidence threshold for ValidationState::Degraded
    pub const CONFIDENCE_DEGRADED: f32 = 0.70;
    /// Confidence threshold for ValidationState::NeedsRebind
    pub const CONFIDENCE_NEEDS_REBIND: f32 = 0.40;
    /// Below this is ValidationState::Unsafe
    pub const CONFIDENCE_UNSAFE: f32 = 0.40;
    
    /// Weight for driver name match in chip validation
    pub const WEIGHT_DRIVER_NAME: f32 = 30.0;
    /// Weight for device symlink target match
    pub const WEIGHT_DEVICE_SYMLINK: f32 = 25.0;
    /// Weight for PCI address match
    pub const WEIGHT_PCI_ADDRESS: f32 = 20.0;
    /// Weight for PCI vendor ID match
    pub const WEIGHT_PCI_VENDOR: f32 = 10.0;
    /// Weight for PCI device ID match
    pub const WEIGHT_PCI_DEVICE: f32 = 10.0;
    /// Weight for I2C bus match
    pub const WEIGHT_I2C_BUS: f32 = 15.0;
    /// Weight for I2C address match
    pub const WEIGHT_I2C_ADDRESS: f32 = 15.0;
    
    /// Weight for channel file existence
    pub const WEIGHT_CHANNEL_EXISTS: f32 = 20.0;
    /// Weight for label match
    pub const WEIGHT_LABEL_MATCH: f32 = 30.0;
    /// Weight for attribute fingerprint match
    pub const WEIGHT_ATTR_FINGERPRINT: f32 = 20.0;
    
    /// Confidence boost for label-based rebind
    pub const LABEL_REBIND_CONFIDENCE: f32 = 0.80;
    
    /// Maximum age (in seconds) before fingerprint considered stale
    pub const MAX_FINGERPRINT_AGE_SECS: u64 = 86400 * 30; // 30 days
    
    /// Value range tolerance for expected_value_range guard
    pub const VALUE_RANGE_TOLERANCE: f32 = 0.20; // 20% outside learned range triggers warning
}

/// Default fan curve points
pub mod default_curve {
    use crate::data::CurvePoint;

    /// Returns the default "quiet" fan curve
    pub fn quiet() -> Vec<CurvePoint> {
        vec![
            CurvePoint { temperature: 30.0, fan_percent: 15.0 },
            CurvePoint { temperature: 50.0, fan_percent: 25.0 },
            CurvePoint { temperature: 65.0, fan_percent: 45.0 },
            CurvePoint { temperature: 75.0, fan_percent: 70.0 },
            CurvePoint { temperature: 85.0, fan_percent: 100.0 },
        ]
    }

    /// Returns the default "balanced" fan curve
    pub fn balanced() -> Vec<CurvePoint> {
        vec![
            CurvePoint { temperature: 30.0, fan_percent: 20.0 },
            CurvePoint { temperature: 50.0, fan_percent: 40.0 },
            CurvePoint { temperature: 70.0, fan_percent: 70.0 },
            CurvePoint { temperature: 85.0, fan_percent: 100.0 },
        ]
    }

    /// Returns the default "performance" fan curve
    pub fn performance() -> Vec<CurvePoint> {
        vec![
            CurvePoint { temperature: 30.0, fan_percent: 40.0 },
            CurvePoint { temperature: 50.0, fan_percent: 60.0 },
            CurvePoint { temperature: 65.0, fan_percent: 80.0 },
            CurvePoint { temperature: 75.0, fan_percent: 100.0 },
        ]
    }

    /// Returns the "full speed" curve (always 100%)
    pub fn full_speed() -> Vec<CurvePoint> {
        vec![
            CurvePoint { temperature: 0.0, fan_percent: 100.0 },
        ]
    }
}
