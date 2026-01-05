//! Command Line Interface
//!
//! Provides CLI access to all settings and core functionality.

use clap::{Parser, Subcommand, Args};

#[derive(Parser)]
#[command(name = "hyperfan")]
#[command(author = "Henry Kleyn")]
#[command(version)]
#[command(about = "Hyperfan - Modern fan control for Linux")]
#[command(long_about = "Hyperfan - Modern fan control for Linux

A GPU-accelerated fan control application with custom curves,
real-time temperature monitoring, and intelligent hardware detection.

EXAMPLES:
    hyperfan                           Launch GUI (default)
    hyperfan status                    Show system status summary
    hyperfan hardware temps            List all temperature sensors
    hyperfan hardware detect           Detect fan-to-PWM mappings
    hyperfan curves list               List all saved fan curves
    hyperfan curves create MyCurve --preset balanced
    hyperfan settings show             Show all settings as JSON
    hyperfan settings set display.temperature_unit fahrenheit
    hyperfan service status            Check daemon service status
    hyperfan fan set /sys/class/hwmon/hwmon3/pwm1 50

ENVIRONMENT VARIABLES:
    RUST_LOG=debug         Enable debug logging
    GSK_RENDERER=ngl       Force new OpenGL renderer (recommended)
    GSK_RENDERER=vulkan    Force Vulkan renderer
    GSK_RENDERER=cairo     Force Cairo software renderer
    GTK_DEBUG=renderer     Show which renderer is being used

FILES:
    ~/.config/hyperfan/settings.json      Application settings
    ~/.config/hyperfan/curves.json        Fan curve definitions
    ~/.config/hyperfan/temp_graphs.json   Temperature graph configs
    ~/.config/hyperfan/bindings.json      Hardware binding store")]
#[command(propagate_version = true)]
pub struct Cli {
    /// Show performance metrics overlay in GUI
    #[arg(long)]
    pub perf: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Launch the GUI application (default)
    Gui,

    /// Hardware discovery and information
    #[command(subcommand, about = "Discover and query hardware sensors")]
    Hardware(HardwareCommands),

    /// Fan curve management
    #[command(subcommand, about = "Create, list, and manage fan curves")]
    Curves(CurveCommands),

    /// Temperature graph management
    #[command(subcommand, about = "Manage persistent temperature graphs")]
    Graphs(GraphCommands),

    /// Active fan-curve pair management
    #[command(subcommand, about = "Manage active fan-curve bindings")]
    Pairs(PairCommands),

    /// Sensor friendly name management
    #[command(subcommand, about = "Manage sensor display names")]
    Sensors(SensorCommands),

    /// Hardware binding store management
    #[command(subcommand, about = "Manage hardware fingerprint bindings")]
    Bindings(BindingCommands),

    /// Settings management
    #[command(subcommand, about = "View and modify application settings")]
    Settings(SettingsCommands),

    /// Daemon service management
    #[command(subcommand, about = "Control the hyperfand privileged service")]
    Service(ServiceCommands),

    /// Direct fan control (requires root or daemon)
    #[command(subcommand, about = "Read and control fan speeds directly")]
    Fan(FanCommands),

    /// PWM-to-fan pairing management
    #[command(subcommand, about = "Manage PWM-to-fan pairings")]
    Pairings(PairingCommands),

    /// GPU fan control
    #[command(subcommand, about = "Control GPU fan speeds")]
    Gpu(GpuCommands),

    /// System information
    #[command(subcommand, about = "Show system and platform information")]
    System(SystemCommands),

    /// Show system status summary
    Status,
}

// ============================================================================
// Hardware Commands
// ============================================================================

#[derive(Subcommand)]
pub enum HardwareCommands {
    /// List all hwmon chips
    Chips,
    /// List all temperature sensors
    Temps,
    /// List all fans
    Fans,
    /// List all PWM controllers
    Pwm,
    /// List all GPUs
    Gpus,
    /// Show full hardware snapshot as JSON
    Snapshot,
    /// Detect fan-to-PWM mappings
    Detect {
        /// Use heuristic detection (faster, less accurate)
        #[arg(long)]
        heuristic: bool,
    },
    /// Show saved PWM-fan mappings
    Mappings,
    /// Clear saved PWM-fan mappings
    ClearMappings {
        /// Skip confirmation
        #[arg(long)]
        force: bool,
    },
    /// Check if detection has been completed
    DetectionStatus,
}

// ============================================================================
// Curve Commands
// ============================================================================

#[derive(Subcommand)]
pub enum CurveCommands {
    /// List all saved curves
    List,
    /// Show curve details
    Show {
        /// Curve ID or name
        id: String,
    },
    /// Create a new curve from preset
    Create {
        /// Curve name
        name: String,
        /// Preset: silent, balanced, performance, aggressive
        #[arg(long, default_value = "balanced")]
        preset: String,
    },
    /// Delete a curve
    Delete {
        /// Curve ID
        id: String,
    },

    Rename {
        /// Curve ID
        id: String,
        /// New curve name
        name: String,
    },

    SetPoints {
        /// Curve ID
        id: String,
        /// JSON file containing points as [[temp_c, percent], ...]
        path: String,
    },
    /// Export curves to JSON file
    Export {
        /// Output file path
        path: String,
    },
    /// Import curves from JSON file
    Import {
        /// Input file path
        path: String,
    },
}

// ============================================================================
// Graph Commands
// ============================================================================

#[derive(Subcommand)]
pub enum GraphCommands {
    /// List all saved temperature graphs
    List,
    /// Add a new temperature graph
    Add {
        /// Graph name
        name: String,
        /// Temperature source path (hwmon path or gpu:INDEX:SENSOR)
        source: String,
        /// Human-readable label for the source
        #[arg(long)]
        label: Option<String>,
    },
    /// Remove a temperature graph
    Remove {
        /// Graph ID
        id: String,
    },
}

// ============================================================================
// Pair Commands (Active fan-curve bindings)
// ============================================================================

#[derive(Subcommand)]
pub enum PairCommands {
    /// List all active fan-curve pairs
    List,
    /// Show details of a specific pair
    Show {
        /// Pair ID
        id: String,
    },
    /// Delete a pair
    Delete {
        /// Pair ID
        id: String,
    },
    /// Enable a pair
    Enable {
        /// Pair ID
        id: String,
    },
    /// Disable a pair
    Disable {
        /// Pair ID
        id: String,
    },

    /// Create a new pair
    Create {
        /// Pair name
        name: String,

        /// Curve ID to use
        curve_id: String,

        /// Temperature source path
        temp_source_path: String,

        /// PWM controller path
        fan_path: String,
    },
}

// =========================================================================
// Pairing Commands (PWM-to-fan pairing)
// =========================================================================

#[derive(Subcommand)]
pub enum PairingCommands {
    /// List all stored PWM-to-fan pairings (from settings.json)
    List,

    /// Set or update a PWM-to-fan pairing (stored in settings.json)
    Set {
        /// PWM path
        pwm_path: String,
        /// Fan input path (omit to unpair)
        fan_path: Option<String>,
        /// Fan display name (optional)
        #[arg(long)]
        fan_name: Option<String>,
        /// Friendly name for this pairing (optional)
        #[arg(long)]
        friendly_name: Option<String>,
    },

    /// Delete a PWM-to-fan pairing entry (stored in settings.json)
    Delete {
        /// PWM path
        pwm_path: String,
    },
}

// ============================================================================
// Sensor Commands (Friendly names)
// ============================================================================

#[derive(Subcommand)]
pub enum SensorCommands {
    /// List all sensor friendly names
    List,
    /// Get friendly name for a sensor
    Get {
        /// Sensor path
        path: String,
    },
    /// Set friendly name for a sensor
    Set {
        /// Sensor path
        path: String,
        /// Friendly name
        name: String,
    },
    /// Remove friendly name for a sensor
    Remove {
        /// Sensor path
        path: String,
    },
}

// ============================================================================
// Binding Commands (Hardware fingerprints)
// ============================================================================

#[derive(Subcommand)]
pub enum BindingCommands {
    /// Show binding store status
    Status,
    /// List all bindings
    List,
    /// Validate all bindings against current hardware
    Validate,
    /// Discover and fingerprint current system hardware
    Discover,
    /// Show binding store file path
    Path,
    /// Clear all bindings
    Clear {
        /// Skip confirmation
        #[arg(long)]
        force: bool,
    },
}

// ============================================================================
// GPU Commands
// ============================================================================

#[derive(Subcommand)]
pub enum GpuCommands {
    /// List all GPUs with details
    List,
    /// Show GPU details
    Show {
        /// GPU index
        index: u32,
    },
    /// Set GPU fan speed
    Set {
        /// GPU index
        index: u32,
        /// Fan index (optional, NVIDIA only)
        #[arg(long)]
        fan: Option<u32>,
        /// Fan speed percentage (0-100)
        percent: f32,
    },
    /// Reset GPU fan to automatic control
    Auto {
        /// GPU index
        index: u32,
        /// Fan index (optional, NVIDIA only)
        #[arg(long)]
        fan: Option<u32>,
    },
}

// ============================================================================
// System Commands
// ============================================================================

#[derive(Subcommand)]
pub enum SystemCommands {
    /// Show OS and platform information
    Info,
    /// Show system summary (hardware counts)
    Summary,
    /// Check if running on Linux
    IsLinux,
    /// Check if running on BSD
    IsBsd,
    /// Detect init system
    InitSystem,
    /// Detect desktop environment
    Desktop,
}

// ============================================================================
// Settings Commands
// ============================================================================

#[derive(Subcommand)]
pub enum SettingsCommands {
    /// Show all current settings as JSON
    Show,
    /// Get a specific setting value
    #[command(after_help = "AVAILABLE KEYS:\n  general.start_at_boot\n  general.poll_interval_ms\n  general.apply_curves_on_startup\n  general.default_page\n  display.temperature_unit\n  display.fan_control_metric\n  display.show_tray_icon\n  display.graph_style\n  display.color_scheme\n  display.display_backend\n  display.window_manager\n  advanced.ec_direct_control_enabled")]
    Get {
        /// Setting key (e.g., display.temperature_unit)
        key: String,
    },
    /// Set a setting value
    #[command(after_help = "EXAMPLES:\n  hyperfan settings set display.temperature_unit fahrenheit\n  hyperfan settings set general.poll_interval_ms 200\n  hyperfan settings set display.color_scheme dark")]
    Set(SetSettingArgs),
    /// Reset all settings to defaults
    Reset {
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// Show settings file path
    Path,
    /// Export all settings to JSON file
    Export {
        /// Output file path
        path: String,
    },
    /// Import settings from JSON file
    Import {
        /// Input file path
        path: String,
    },
}

#[derive(Args)]
pub struct SetSettingArgs {
    /// Setting key
    pub key: String,
    /// New value
    pub value: String,
}

// ============================================================================
// Service Commands
// ============================================================================

#[derive(Subcommand)]
pub enum ServiceCommands {
    /// Show service status
    Status,
    /// Install the hyperfand service
    Install,
    /// Uninstall the hyperfand service
    Uninstall,
    /// Reload daemon configuration
    Reload,
    /// List hardware via daemon
    ListHardware,
    /// Detect fan mappings via daemon
    DetectMappings,
    /// Start the service
    Start,
    /// Stop the service
    Stop,
    /// Restart the service
    Restart,
    /// Check if daemon is available
    Ping,
}

// ============================================================================
// Fan Commands
// ============================================================================

#[derive(Subcommand)]
pub enum FanCommands {
    /// Read current fan RPM
    Read {
        /// Fan sensor path
        path: String,
    },
    /// Read current PWM value
    ReadPwm {
        /// PWM control path
        path: String,
    },
    /// Set fan speed (requires root or daemon)
    Set {
        /// PWM control path
        path: String,
        /// Speed as percentage (0-100)
        percent: f32,
    },
    /// Enable manual PWM control
    Manual {
        /// PWM control path
        path: String,
    },
    /// Reset fan to automatic control
    Auto {
        /// PWM control path
        path: String,
    },

    /// Temporarily override a PWM value (0-255) for a short duration
    Override {
        /// PWM path (sysfs path or gpu virtual id)
        path: String,
        /// PWM value 0-255
        value: u8,
        /// Override TTL in milliseconds
        #[arg(long, default_value_t = 1500)]
        ttl_ms: u32,
    },

    /// Clear a previously set PWM override
    ClearOverride {
        /// PWM path (sysfs path or gpu virtual id)
        path: String,
    },
}

// ============================================================================
// CLI Execution
// ============================================================================

pub fn run_cli(cli: &Cli) -> Result<bool, Box<dyn std::error::Error>> {
    match &cli.command {
        None | Some(Commands::Gui) => Ok(false), // Continue to GUI
        Some(cmd) => {
            execute_command(cmd)?;
            Ok(true) // CLI handled, exit
        }
    }
}

fn execute_command(cmd: &Commands) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        Commands::Gui => Ok(()),
        Commands::Status => cmd_status(),
        Commands::Hardware(sub) => cmd_hardware(sub),
        Commands::Curves(sub) => cmd_curves(sub),
        Commands::Graphs(sub) => cmd_graphs(sub),
        Commands::Pairs(sub) => cmd_pairs(sub),
        Commands::Sensors(sub) => cmd_sensors(sub),
        Commands::Bindings(sub) => cmd_bindings(sub),
        Commands::Settings(sub) => cmd_settings(sub),
        Commands::Service(sub) => cmd_service(sub),
        Commands::Fan(sub) => cmd_fan(sub),
        Commands::Pairings(sub) => cmd_pairings(sub),
        Commands::Gpu(sub) => cmd_gpu(sub),
        Commands::System(sub) => cmd_system(sub),
    }
}

// ============================================================================
// Status Command
// ============================================================================

fn cmd_status() -> Result<(), Box<dyn std::error::Error>> {
    println!("Hyperfan Status");
    println!("===============");
    println!();

    // Service status
    let service_status = hf_core::get_service_status();
    let daemon_available = hf_core::is_daemon_available();
    println!("Service: {}", service_status);
    println!("Daemon:  {}", if daemon_available { "connected" } else { "not available" });
    println!();

    // Hardware summary (daemon authoritative)
    if daemon_available {
        if let Ok(hw) = hf_core::daemon_list_hardware() {
            let temp_count: usize = hw.chips.iter().map(|c| c.temperatures.len()).sum();
            let fan_count: usize = hw.chips.iter().map(|c| c.fans.len()).sum();
            let pwm_count: usize = hw.chips.iter().map(|c| c.pwms.len()).sum();
            println!("Hardware:");
            println!("  Chips: {}", hw.chips.len());
            println!("  Temps: {}", temp_count);
            println!("  Fans:  {}", fan_count);
            println!("  PWMs:  {}", pwm_count);
        }

        if let Ok(gpus) = hf_core::daemon_list_gpus() {
            println!("  GPUs:  {}", gpus.len());
        }
    }
    println!();

    // Curves and pairs
    if let Ok(curves) = hf_core::load_curves() {
        println!("Curves: {}", curves.all().len());
    }
    if let Ok(settings) = hf_core::load_settings() {
        let active = settings.active_pairs.iter().filter(|p| p.active).count();
        println!("Active pairs: {}", active);
    }

    Ok(())
}

// ============================================================================
// Hardware Commands
// ============================================================================

fn cmd_hardware(cmd: &HardwareCommands) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        HardwareCommands::Chips => {
            if !hf_core::is_daemon_available() {
                return Err("Daemon not available".into());
            }
            let hw = hf_core::daemon_list_hardware()?;
            println!("Hwmon Chips ({}):", hw.chips.len());
            for chip in &hw.chips {
                println!("  {} ({})", chip.name, chip.path);
                println!("    Temps: {}", chip.temperatures.len());
                println!("    Fans: {}", chip.fans.len());
                println!("    PWMs: {}", chip.pwms.len());
            }
        }
        HardwareCommands::Temps => {
            if !hf_core::is_daemon_available() {
                return Err("Daemon not available".into());
            }
            let hw = hf_core::daemon_list_hardware()?;
            println!("Temperature Sensors:");
            for chip in &hw.chips {
                for temp in &chip.temperatures {
                    let label = temp.label.as_deref().unwrap_or(&temp.name);
                    println!("  {} / {}: {:.1}°C ({})", chip.name, label, temp.value, temp.path);
                }
            }
        }
        HardwareCommands::Fans => {
            if !hf_core::is_daemon_available() {
                return Err("Daemon not available".into());
            }
            let hw = hf_core::daemon_list_hardware()?;
            println!("Fan Sensors:");
            for chip in &hw.chips {
                for fan in &chip.fans {
                    let label = fan.label.as_deref().unwrap_or(&fan.name);
                    let rpm_str = fan.rpm.map(|v| format!("{} RPM", v)).unwrap_or_else(|| "N/A".into());
                    println!("  {} / {}: {} ({})", chip.name, label, rpm_str, fan.path);
                }
            }
        }
        HardwareCommands::Pwm => {
            if !hf_core::is_daemon_available() {
                return Err("Daemon not available".into());
            }
            let hw = hf_core::daemon_list_hardware()?;
            println!("PWM Controllers:");
            for chip in &hw.chips {
                for pwm in &chip.pwms {
                    let label = pwm.name.as_str();
                    let pct = pwm.value as f32 / 255.0 * 100.0;
                    println!("  {} / {}: {:.0}% ({})", chip.name, label, pct, pwm.path);
                }
            }
        }
        HardwareCommands::Gpus => {
            if !hf_core::is_daemon_available() {
                return Err("Daemon not available".into());
            }
            let gpus = hf_core::daemon_list_gpus()?;
            println!("GPUs ({}):", gpus.len());
            for gpu in &gpus {
                println!("  [{}] {} ({})", gpu.index, gpu.name, gpu.vendor);
                let value_str = gpu.temp.map(|v| format!("{:.1}°C", v)).unwrap_or_else(|| "N/A".into());
                println!("      Temp: {}", value_str);
            }
        }
        HardwareCommands::Snapshot => {
            let snapshot = hf_core::capture_raw_snapshot()?;
            let json = hf_core::snapshot_to_json(&snapshot)?;
            println!("{}", json);
        }
        HardwareCommands::Detect { heuristic } => {
            let _ = heuristic; // daemon currently implements heuristic detection
            if !hf_core::is_daemon_available() {
                return Err("Daemon not available".into());
            }
            println!("Detecting fan mappings via daemon...");
            let mappings = hf_core::daemon_detect_fan_mappings()?;
            println!("Found {} mappings:", mappings.len());
            for m in &mappings {
                println!("  {} -> {} (confidence: {:.0}%)", m.pwm_path, m.fan_path, m.confidence * 100.0);
            }
            println!("Mappings persisted by daemon");
        }
        HardwareCommands::Mappings => {
            let mappings = hf_core::get_pwm_fan_mappings()?;
            println!("Saved PWM-Fan Mappings ({}):", mappings.len());
            for m in &mappings {
                println!("  {} -> {} (confidence: {:.0}%)", m.pwm_name, m.fan_name, m.confidence * 100.0);
            }
        }
        HardwareCommands::ClearMappings { force } => {
            if !force {
                println!("Warning: This will clear all saved PWM-fan mappings. Use --force to confirm.");
                return Ok(());
            }
            hf_core::clear_pwm_fan_mappings()?;
            println!("Cleared all PWM-fan mappings");
        }
        HardwareCommands::DetectionStatus => {
            let completed = hf_core::is_detection_completed()?;
            println!("Detection completed: {}", completed);
        }
    }
    Ok(())
}

// ============================================================================
// Curve Commands
// ============================================================================

fn cmd_curves(cmd: &CurveCommands) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        CurveCommands::List => {
            let store = hf_core::load_curves()?;
            let curves = store.all();
            println!("Fan Curves ({}):", curves.len());
            for curve in curves {
                println!("  [{}] {} ({} points)", curve.id, curve.name, curve.points.len());
            }
        }
        CurveCommands::Show { id } => {
            let store = hf_core::load_curves()?;
            let curve = store.all().into_iter()
                .find(|c| c.id == *id || c.name.to_lowercase() == id.to_lowercase())
                .ok_or_else(|| format!("Curve not found: {}", id))?;
            println!("Curve: {} ({})", curve.name, curve.id);
            println!("Points:");
            for (temp, pct) in &curve.points {
                println!("  {:.0}°C -> {:.0}%", temp, pct);
            }
        }
        CurveCommands::Create { name, preset } => {
            let curve = match preset.to_lowercase().as_str() {
                "quiet" | "silent" => hf_core::CurvePreset::Quiet,
                "balanced" => hf_core::CurvePreset::Balanced,
                "performance" => hf_core::CurvePreset::Performance,
                "full" | "fullspeed" => hf_core::CurvePreset::FullSpeed,
                _ => return Err(format!("Unknown preset: {}. Use: quiet, balanced, performance, full", preset).into()),
            };
            let id = format!("curve_{}", std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis());
            let points: Vec<(f32, f32)> = curve.points().iter().map(|p| (p.temperature, p.fan_percent)).collect();
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let persisted = hf_core::PersistedCurve {
                id: id.clone(),
                name: name.clone(),
                temp_source_path: String::new(),
                temp_source_label: String::new(),
                points,
                created_at: now,
                updated_at: now,
                hysteresis: hf_core::constants::curve::DEFAULT_HYSTERESIS_CELSIUS,
                delay_ms: hf_core::constants::curve::DEFAULT_DELAY_MS,
                ramp_up_speed: hf_core::constants::curve::DEFAULT_RAMP_UP_SPEED,
                ramp_down_speed: hf_core::constants::curve::DEFAULT_RAMP_DOWN_SPEED,
            };
            hf_core::save_curve(persisted)?;
            if hf_core::is_daemon_available() {
                if let Err(e) = hf_core::daemon_reload_config() {
                    eprintln!("Warning: Failed to signal daemon reload: {}", e);
                }
            }
            println!("Created curve '{}' with ID: {}", name, id);
        }
        CurveCommands::Delete { id } => {
            hf_core::delete_curve(id)?;
            if hf_core::is_daemon_available() {
                if let Err(e) = hf_core::daemon_reload_config() {
                    eprintln!("Warning: Failed to signal daemon reload: {}", e);
                }
            }
            println!("Deleted curve: {}", id);
        }
        CurveCommands::Rename { id, name } => {
            let store = hf_core::load_curves()?;
            let curves = store.all();

            let Some(curve) = curves.into_iter().find(|c| c.id == *id) else {
                return Err(format!("Curve not found: {}", id).into());
            };

            let mut curve = curve.clone();
            curve.name = name.clone();
            hf_core::save_curve(curve)?;

            if hf_core::is_daemon_available() {
                if let Err(e) = hf_core::daemon_reload_config() {
                    eprintln!("Warning: Failed to signal daemon reload: {}", e);
                }
            }
            println!("Renamed curve {} -> {}", id, name);
        }
        CurveCommands::SetPoints { id, path } => {
            let content = std::fs::read_to_string(path)?;
            let points: Vec<(f32, f32)> = serde_json::from_str(&content)?;
            let updated = hf_core::update_curve_points(id, points)?;
            if !updated {
                return Err(format!("Curve not found: {}", id).into());
            }
            if hf_core::is_daemon_available() {
                if let Err(e) = hf_core::daemon_reload_config() {
                    eprintln!("Warning: Failed to signal daemon reload: {}", e);
                }
            }
            println!("Updated curve points for {}", id);
        }
        CurveCommands::Export { path } => {
            let store = hf_core::load_curves()?;
            let json = serde_json::to_string_pretty(&store.all())?;
            std::fs::write(path, json)?;
            println!("Exported curves to: {}", path);
        }
        CurveCommands::Import { path } => {
            let content = std::fs::read_to_string(path)?;
            let curves: Vec<hf_core::PersistedCurve> = serde_json::from_str(&content)?;
            for curve in curves {
                hf_core::save_curve(curve)?;
            }
            if hf_core::is_daemon_available() {
                if let Err(e) = hf_core::daemon_reload_config() {
                    eprintln!("Warning: Failed to signal daemon reload: {}", e);
                }
            }
            println!("Imported curves from: {}", path);
        }
    }
    Ok(())
}

// ============================================================================
// Graph Commands
// ============================================================================

fn cmd_graphs(cmd: &GraphCommands) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        GraphCommands::List => {
            let graphs = hf_core::load_temp_graphs()?;
            println!("Temperature Graphs ({}):", graphs.len());
            for g in &graphs {
                println!("  [{}] {} -> {}", g.id, g.name, g.temp_source_path);
            }
        }
        GraphCommands::Add { name, source, label } => {
            let id = format!("graph_{}", std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis());
            let graph = hf_core::PersistedGraph {
                id: id.clone(),
                name: name.clone(),
                temp_source_path: source.clone(),
                temp_source_label: label.clone().unwrap_or_else(|| source.clone()),
            };
            hf_core::add_temp_graph(graph)?;
            println!("Added graph '{}' with ID: {}", name, id);
        }
        GraphCommands::Remove { id } => {
            hf_core::remove_temp_graph(id)?;
            println!("Removed graph: {}", id);
        }
    }
    Ok(())
}

// ============================================================================
// Settings Commands
// ============================================================================

fn cmd_settings(cmd: &SettingsCommands) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        SettingsCommands::Show => {
            let settings = hf_core::load_settings()?;
            let json = serde_json::to_string_pretty(&settings)?;
            println!("{}", json);
        }
        SettingsCommands::Get { key } => {
            let settings = hf_core::load_settings()?;
            let value = get_setting_value(&settings, key)?;
            println!("{}", value);
        }
        SettingsCommands::Set(args) => {
            set_setting_value(&args.key, &args.value)?;
            println!("Set {} = {}", args.key, args.value);
        }
        SettingsCommands::Reset { force } => {
            if !force {
                eprintln!("This will reset all settings to defaults. Use --force to confirm.");
                return Ok(());
            }
            let defaults = hf_core::AppSettings::default();
            hf_core::save_settings(&defaults)?;
            println!("Settings reset to defaults");
        }
        SettingsCommands::Path => {
            let path = hf_core::get_settings_path()?;
            println!("{}", path.display());
        }
        SettingsCommands::Export { path } => {
            let settings = hf_core::load_settings()?;
            let json = serde_json::to_string_pretty(&settings)?;
            std::fs::write(path, json)?;
            println!("Exported settings to: {}", path);
        }
        SettingsCommands::Import { path } => {
            let content = std::fs::read_to_string(path)?;
            let settings: hf_core::AppSettings = serde_json::from_str(&content)?;
            hf_core::save_settings(&settings)?;
            println!("Imported settings from: {}", path);
        }
    }
    Ok(())
}

fn get_setting_value(settings: &hf_core::AppSettings, key: &str) -> Result<String, Box<dyn std::error::Error>> {
    let parts: Vec<&str> = key.split('.').collect();
    match parts.as_slice() {
        ["general", "start_at_boot"] => Ok(settings.general.start_at_boot.to_string()),
        ["general", "poll_interval_ms"] => Ok(settings.general.poll_interval_ms.to_string()),
        ["general", "apply_curves_on_startup"] => Ok(settings.general.apply_curves_on_startup.to_string()),
        ["general", "default_page"] => Ok(settings.general.default_page.clone()),
        ["display", "temperature_unit"] => Ok(settings.display.temperature_unit.clone()),
        ["display", "fan_control_metric"] => Ok(settings.display.fan_control_metric.clone()),
        ["display", "show_tray_icon"] => Ok(settings.display.show_tray_icon.to_string()),
        ["display", "graph_style"] => Ok(settings.display.graph_style.clone()),
        ["display", "color_scheme"] => Ok(settings.display.color_scheme.clone()),
        ["display", "display_backend"] => Ok(settings.display.display_backend.clone()),
        ["display", "window_manager"] => Ok(settings.display.window_manager.clone()),
        ["advanced", "ec_direct_control_enabled"] => Ok(settings.advanced.ec_direct_control_enabled.to_string()),
        _ => Err(format!("Unknown setting: {}", key).into()),
    }
}

fn set_setting_value(key: &str, value: &str) -> Result<(), Box<dyn std::error::Error>> {
    hf_core::update_setting(|settings| {
        let parts: Vec<&str> = key.split('.').collect();
        match parts.as_slice() {
            ["general", "start_at_boot"] => settings.general.start_at_boot = value.parse().unwrap_or(false),
            ["general", "poll_interval_ms"] => settings.general.poll_interval_ms = value.parse().unwrap_or(100),
            ["general", "apply_curves_on_startup"] => settings.general.apply_curves_on_startup = value.parse().unwrap_or(true),
            ["general", "default_page"] => settings.general.default_page = value.to_string(),
            ["display", "temperature_unit"] => settings.display.temperature_unit = value.to_string(),
            ["display", "fan_control_metric"] => settings.display.fan_control_metric = value.to_string(),
            ["display", "show_tray_icon"] => settings.display.show_tray_icon = value.parse().unwrap_or(false),
            ["display", "graph_style"] => settings.display.graph_style = value.to_string(),
            ["display", "color_scheme"] => settings.display.color_scheme = value.to_string(),
            ["display", "display_backend"] => settings.display.display_backend = value.to_string(),
            ["display", "window_manager"] => settings.display.window_manager = value.to_string(),
            ["advanced", "ec_direct_control_enabled"] => settings.advanced.ec_direct_control_enabled = value.parse().unwrap_or(false),
            _ => eprintln!("Unknown setting: {}", key),
        }
    })?;
    Ok(())
}

// ============================================================================
// Service Commands
// ============================================================================

fn cmd_service(cmd: &ServiceCommands) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        ServiceCommands::Status => {
            let status = hf_core::get_service_status();
            let installed = hf_core::is_service_installed();
            let running = hf_core::is_service_running();
            let init = hf_core::detect_init_system();
            println!("Init system: {:?}", init);
            println!("Installed:   {}", installed);
            println!("Running:     {}", running);
            println!("Status:      {}", status);
        }
        ServiceCommands::Install => {
            hf_core::install_service()?;
            println!("Service installed");
        }
        ServiceCommands::Uninstall => {
            hf_core::uninstall_service()?;
            println!("Service uninstalled");
        }
        ServiceCommands::Start => {
            hf_core::start_service()?;
            println!("Service started");
        }
        ServiceCommands::Stop => {
            hf_core::stop_service()?;
            println!("Service stopped");
        }
        ServiceCommands::Restart => {
            hf_core::restart_service()?;
            println!("Service restarted");
        }
        ServiceCommands::Ping => {
            if hf_core::is_daemon_available() {
                if let Ok(version) = hf_core::get_daemon_version() {
                    println!("Daemon available: v{}", version);
                } else {
                    println!("Daemon available");
                }
            } else {
                println!("Daemon not available");
            }
        }
        ServiceCommands::Reload => {
            if !hf_core::is_daemon_available() {
                return Err("Daemon not available".into());
            }
            hf_core::daemon_reload_config()?;
            println!("Daemon configuration reloaded");
        }
        ServiceCommands::ListHardware => {
            if !hf_core::is_daemon_available() {
                return Err("Daemon not available".into());
            }
            let info = hf_core::daemon_list_hardware()?;
            println!("Hardware via daemon:");
            println!("  Chips: {}", info.chips.len());
            for chip in &info.chips {
                println!("    {} - temps: {}, fans: {}, pwms: {}", 
                    chip.name, chip.temperatures.len(), chip.fans.len(), chip.pwms.len());
            }
        }
        ServiceCommands::DetectMappings => {
            if !hf_core::is_daemon_available() {
                return Err("Daemon not available".into());
            }
            println!("Detecting fan mappings via daemon...");
            let mappings = hf_core::daemon_detect_fan_mappings()?;
            println!("Found {} mappings:", mappings.len());
            for m in &mappings {
                println!("  {} -> {} (confidence: {:.0}%)", m.pwm_path, m.fan_path, m.confidence * 100.0);
            }
        }
    }
    Ok(())
}

// ============================================================================
// Fan Commands
// ============================================================================

fn cmd_fan(cmd: &FanCommands) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        FanCommands::Read { path } => {
            if !hf_core::is_daemon_available() {
                return Err("Daemon not available".into());
            }
            let rpm = hf_core::daemon_read_fan_rpm(path)?;
            println!("{} RPM", rpm);
        }
        FanCommands::ReadPwm { path } => {
            if !hf_core::is_daemon_available() {
                return Err("Daemon not available".into());
            }
            let value = hf_core::daemon_read_pwm(path)?;
            let percent = value as f32 / 255.0 * 100.0;
            println!("{} ({:.1}%)", value, percent);
        }
        FanCommands::Set { path, percent } => {
            if !hf_core::is_daemon_available() {
                return Err("Daemon not available".into());
            }
            let pwm_value = (*percent / 100.0 * 255.0) as u8;
            hf_core::daemon_set_pwm(path, pwm_value)?;
            println!("Set {} to {:.1}%", path, percent);
        }
        FanCommands::Manual { path } => {
            if !hf_core::is_daemon_available() {
                return Err("Daemon not available".into());
            }
            hf_core::daemon_enable_manual_pwm(path)?;
            println!("Enabled manual control for {}", path);
        }
        FanCommands::Auto { path } => {
            if !hf_core::is_daemon_available() {
                return Err("Daemon not available".into());
            }
            hf_core::daemon_disable_manual_pwm(path)?;
            println!("Reset {} to automatic control", path);
        }

        FanCommands::Override { path, value, ttl_ms } => {
            if !hf_core::is_daemon_available() {
                return Err("Daemon not available".into());
            }
            hf_core::daemon_set_pwm_override(path, *value, *ttl_ms)?;
            println!("Override set for {} = {} (ttl_ms={})", path, value, ttl_ms);
        }

        FanCommands::ClearOverride { path } => {
            if !hf_core::is_daemon_available() {
                return Err("Daemon not available".into());
            }
            hf_core::daemon_clear_pwm_override(path)?;
            println!("Override cleared for {}", path);
        }
    }
    Ok(())
}

// ============================================================================
// Pair Commands
// ============================================================================

fn cmd_pairs(cmd: &PairCommands) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        PairCommands::List => {
            let settings = hf_core::load_settings()?;
            println!("Fan-Curve Pairs ({}):", settings.active_pairs.len());
            for pair in &settings.active_pairs {
                let status = if pair.active { "active" } else { "disabled" };
                println!("  [{}] {} ({})", pair.id, pair.name, status);
                println!("      Curve: {}", pair.curve_id);
                println!("      Temp:  {}", pair.temp_source_path);
                println!("      Fan:   {}", pair.fan_path);
            }
        }
        PairCommands::Show { id } => {
            let settings = hf_core::load_settings()?;
            let pair = settings.active_pairs.iter()
                .find(|p| p.id == *id)
                .ok_or_else(|| format!("Pair not found: {}", id))?;
            println!("Pair: {} ({})", pair.name, pair.id);
            println!("Active: {}", pair.active);
            println!("Curve ID: {}", pair.curve_id);
            println!("Temperature source: {}", pair.temp_source_path);
            println!("Fan path: {}", pair.fan_path);
        }
        PairCommands::Delete { id } => {
            hf_core::delete_pair(id)?;
            if hf_core::is_daemon_available() {
                if let Err(e) = hf_core::daemon_reload_config() {
                    eprintln!("Warning: Failed to signal daemon reload: {}", e);
                }
            }
            println!("Deleted pair: {}", id);
        }
        PairCommands::Enable { id } => {
            hf_core::update_setting(|s| {
                if let Some(pair) = s.active_pairs.iter_mut().find(|p| p.id == *id) {
                    pair.active = true;
                }
            })?;
            if hf_core::is_daemon_available() {
                if let Err(e) = hf_core::daemon_reload_config() {
                    eprintln!("Warning: Failed to signal daemon reload: {}", e);
                }
            }
            println!("Enabled pair: {}", id);
        }
        PairCommands::Disable { id } => {
            hf_core::update_setting(|s| {
                if let Some(pair) = s.active_pairs.iter_mut().find(|p| p.id == *id) {
                    pair.active = false;
                }
            })?;
            if hf_core::is_daemon_available() {
                if let Err(e) = hf_core::daemon_reload_config() {
                    eprintln!("Warning: Failed to signal daemon reload: {}", e);
                }
            }
            println!("Disabled pair: {}", id);
        }

        PairCommands::Create {
            name,
            curve_id,
            temp_source_path,
            fan_path,
        } => {
            let id = format!(
                "pair_{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
            );

            let pair = hf_core::FanCurvePair {
                id: id.clone(),
                name: name.clone(),
                curve_id: curve_id.clone(),
                temp_source_path: temp_source_path.clone(),
                fan_path: fan_path.clone(),
                fan_paths: vec![fan_path.clone()],
                hysteresis_ms: 0,
                active: true,
            };

            hf_core::save_pair(pair)?;

            if hf_core::is_daemon_available() {
                if let Err(e) = hf_core::daemon_reload_config() {
                    eprintln!("Warning: Failed to signal daemon reload: {}", e);
                }
            }

            println!("Created pair '{}' with ID: {}", name, id);
        }
    }
    Ok(())
}

fn cmd_pairings(cmd: &PairingCommands) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        PairingCommands::List => {
            let settings = hf_core::load_settings()?;
            println!("PWM-Fan Pairings ({}):", settings.pwm_fan_pairings.len());
            for p in &settings.pwm_fan_pairings {
                println!("  PWM: {}", p.pwm_path);
                if let Some(ref name) = p.friendly_name {
                    println!("    Name: {}", name);
                }
                println!("    Fan:  {:?}", p.fan_path);
            }
        }

        PairingCommands::Set {
            pwm_path,
            fan_path,
            fan_name,
            friendly_name,
        } => {
            let mut settings = hf_core::load_settings()?;
            settings.pwm_fan_pairings.retain(|p| p.pwm_path != *pwm_path);

            let pairing = hf_core::create_fingerprinted_pairing(
                pwm_path,
                fan_path.as_deref(),
                fan_name.as_deref(),
                friendly_name.as_deref(),
            );
            settings.pwm_fan_pairings.push(pairing);
            hf_core::save_settings(&settings)?;

            if hf_core::is_daemon_available() {
                if let Err(e) = hf_core::daemon_reload_config() {
                    eprintln!("Warning: Failed to signal daemon reload: {}", e);
                }
            }

            println!("Saved pairing for {}", pwm_path);
        }

        PairingCommands::Delete { pwm_path } => {
            let mut settings = hf_core::load_settings()?;
            let before = settings.pwm_fan_pairings.len();
            settings.pwm_fan_pairings.retain(|p| p.pwm_path != *pwm_path);
            if settings.pwm_fan_pairings.len() == before {
                return Err(format!("No pairing found for {}", pwm_path).into());
            }
            hf_core::save_settings(&settings)?;

            if hf_core::is_daemon_available() {
                if let Err(e) = hf_core::daemon_reload_config() {
                    eprintln!("Warning: Failed to signal daemon reload: {}", e);
                }
            }

            println!("Deleted pairing for {}", pwm_path);
        }
    }

    Ok(())
}

// ============================================================================
// Sensor Commands
// ============================================================================

fn cmd_sensors(cmd: &SensorCommands) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        SensorCommands::List => {
            let names = hf_core::get_all_sensor_friendly_names()?;
            println!("Sensor Friendly Names ({}):", names.len());
            for name in &names {
                println!("  {} -> {}", name.path, name.friendly_name);
            }
        }
        SensorCommands::Get { path } => {
            match hf_core::get_sensor_friendly_name(path)? {
                Some(name) => println!("{}", name),
                None => println!("(no friendly name set)"),
            }
        }
        SensorCommands::Set { path, name } => {
            hf_core::set_sensor_friendly_name(path, name)?;
            println!("Set friendly name for {} -> {}", path, name);
        }
        SensorCommands::Remove { path } => {
            hf_core::set_sensor_friendly_name(path, "")?;
            println!("Removed friendly name for {}", path);
        }
    }
    Ok(())
}

// ============================================================================
// Binding Commands
// ============================================================================

fn cmd_bindings(cmd: &BindingCommands) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        BindingCommands::Status => {
            let exists = hf_core::binding_store_exists();
            println!("Binding store exists: {}", exists);
            if exists {
                if let Ok(store) = hf_core::load_binding_store() {
                    println!("Chips: {}", store.chips.len());
                    println!("PWM channels: {}", store.pwm_channels.len());
                    println!("Fan channels: {}", store.fan_channels.len());
                    println!("Temp channels: {}", store.temp_channels.len());
                    println!("Bindings: {}", store.bindings.len());
                    println!("Last validated: {:?}", store.last_validated_at);
                }
            }
        }
        BindingCommands::List => {
            let store = hf_core::load_binding_store()?;
            println!("Hardware Bindings ({}):", store.bindings.len());
            for (pwm_id, binding) in &store.bindings {
                println!("  [{}]", pwm_id);
                println!("      PWM: {}", binding.pwm_fingerprint.channel.original_name);
                if let Some(ref fan_fp) = binding.fan_fingerprint {
                    println!("      Fan: {}", fan_fp.original_name);
                }
                println!("      State: {:?}", binding.validation_state);
                println!("      Confidence: {:.0}%", binding.confidence_score * 100.0);
            }
        }
        BindingCommands::Validate => {
            println!("Validating bindings against current hardware...");
            let mut store = hf_core::load_binding_store()?;
            let report = hf_core::validate_all_bindings(&mut store);
            hf_core::save_binding_store(&store)?;
            println!("Validation complete:");
            println!("  OK:          {}", report.ok_count);
            println!("  Degraded:    {}", report.degraded_count);
            println!("  Needs rebind: {}", report.needs_rebind_count);
            println!("  Unsafe:      {}", report.unsafe_count);
        }
        BindingCommands::Discover => {
            println!("Discovering and fingerprinting system hardware...");
            let mut store = hf_core::load_binding_store().unwrap_or_default();
            hf_core::discover_and_fingerprint_system(&mut store)?;
            hf_core::save_binding_store(&store)?;
            println!("Discovered {} chips, {} bindings", store.chips.len(), store.bindings.len());
        }
        BindingCommands::Path => {
            match hf_core::get_binding_store_path() {
                Ok(path) => println!("{}", path.display()),
                Err(e) => println!("Error: {}", e),
            }
        }
        BindingCommands::Clear { force } => {
            if !force {
                eprintln!("This will clear all hardware bindings. Use --force to confirm.");
                return Ok(());
            }
            let store = hf_core::BindingStore::default();
            hf_core::save_binding_store(&store)?;
            println!("Cleared all bindings");
        }
    }
    Ok(())
}

// ============================================================================
// GPU Commands
// ============================================================================

fn cmd_gpu(cmd: &GpuCommands) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        GpuCommands::List => {
            if !hf_core::is_daemon_available() {
                return Err("Daemon not available".into());
            }
            let gpus = hf_core::daemon_list_gpus()?;
            println!("GPUs ({}):", gpus.len());
            for gpu in &gpus {
                println!("  [{}] {} ({})", gpu.index, gpu.name, gpu.vendor);
                let value = gpu.temp.map(|v| format!("{:.1}°C", v)).unwrap_or_else(|| "N/A".into());
                println!("      Temp: {}", value);
                let rpm = gpu.fan_rpm.map(|v| format!("{} RPM", v)).unwrap_or_else(|| "N/A".into());
                let pct = gpu.fan_percent.map(|v| format!("{}%", v)).unwrap_or_else(|| "".into());
                println!("      Fan:  {} {}", rpm, pct);
            }
        }
        GpuCommands::Show { index } => {
            if !hf_core::is_daemon_available() {
                return Err("Daemon not available".into());
            }
            let gpus = hf_core::daemon_list_gpus()?;
            let gpu = gpus.iter().find(|g| g.index == *index)
                .ok_or_else(|| anyhow::anyhow!("GPU {} not found", index))?;
            println!("GPU {}: {}", gpu.index, gpu.name);
            println!("Vendor: {}", gpu.vendor);
            let value = gpu.temp.map(|v| format!("{:.1}°C", v)).unwrap_or_else(|| "N/A".into());
            println!("Temp: {}", value);
            let rpm = gpu.fan_rpm.map(|v| format!("{} RPM", v)).unwrap_or_else(|| "N/A".into());
            let pct = gpu.fan_percent.map(|v| format!("{}%", v)).unwrap_or_else(|| "".into());
            println!("Fan:  {} {}", rpm, pct);
        }
        GpuCommands::Set { index, fan, percent } => {
            let gpus = hf_core::enumerate_gpus().unwrap_or_default();
            let gpu = gpus
                .iter()
                .find(|g| g.index == *index)
                .ok_or_else(|| format!("GPU {} not found", index))?;

            match gpu.vendor {
                hf_core::GpuVendor::Nvidia => {
                    let fan_index = fan.unwrap_or(0);
                    hf_core::set_nvidia_fan_speed(*index, fan_index, *percent as u32)?;
                    println!("Set NVIDIA GPU {} fan {} to {:.0}%", index, fan_index, percent);
                }
                hf_core::GpuVendor::Amd => {
                    return Err("AMD GPU fan control via CLI is not implemented yet".into())
                }
                hf_core::GpuVendor::Intel => {
                    return Err("Intel GPU fan control is not supported".into())
                }
            }
        }

        GpuCommands::Auto { index, fan: _ } => {
            let gpus = hf_core::enumerate_gpus().unwrap_or_default();
            let gpu = gpus
                .iter()
                .find(|g| g.index == *index)
                .ok_or_else(|| format!("GPU {} not found", index))?;

            match gpu.vendor {
                hf_core::GpuVendor::Nvidia => {
                    hf_core::reset_nvidia_fan_auto(*index)?;
                    println!("Reset NVIDIA GPU {} to automatic fan control", index);
                }
                hf_core::GpuVendor::Amd => {
                    return Err("AMD GPU fan auto reset via CLI is not implemented yet".into())
                }
                hf_core::GpuVendor::Intel => {
                    return Err("Intel GPU fan auto control is not supported".into())
                }
            }
        }
    }
    Ok(())
}

// ============================================================================
// System Commands
// ============================================================================

fn cmd_system(cmd: &SystemCommands) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        SystemCommands::Info => {
            println!("OS: {}", hf_core::get_os_name());
            println!("Linux: {}", hf_core::is_linux());
            println!("BSD: {}", hf_core::is_bsd());
            println!("Init system: {:?}", hf_core::detect_init_system());
            println!("Desktop: {}", hf_core::detect_desktop_environment());
        }
        SystemCommands::Summary => {
            let summary = hf_core::get_system_summary()?;
            println!("System Summary:");
            println!("  Hostname: {}", summary.hostname);
            println!("  Kernel: {}", summary.kernel_version);
            println!("  CPU: {} ({} cores)", summary.cpu_model, summary.cpu_cores);
            println!("  Memory: {} MB total, {} MB available", 
                summary.memory_total_mb, summary.memory_available_mb);
            println!("  Motherboard: {}", summary.motherboard_name);
            
            // Also show hardware counts (daemon authoritative)
            if hf_core::is_daemon_available() {
                if let Ok(hw) = hf_core::daemon_list_hardware() {
                    let temp_count: usize = hw.chips.iter().map(|c| c.temperatures.len()).sum();
                    let fan_count: usize = hw.chips.iter().map(|c| c.fans.len()).sum();
                    let pwm_count: usize = hw.chips.iter().map(|c| c.pwms.len()).sum();
                    println!("  Hwmon chips: {}", hw.chips.len());
                    println!("  Temperature sensors: {}", temp_count);
                    println!("  Fan sensors: {}", fan_count);
                    println!("  PWM controllers: {}", pwm_count);
                }
                if let Ok(gpus) = hf_core::daemon_list_gpus() {
                    println!("  GPUs: {}", gpus.len());
                }
            }
        }
        SystemCommands::IsLinux => {
            if hf_core::is_linux() {
                println!("true");
            } else {
                println!("false");
            }
        }
        SystemCommands::IsBsd => {
            if hf_core::is_bsd() {
                println!("true");
            } else {
                println!("false");
            }
        }
        SystemCommands::InitSystem => {
            println!("{:?}", hf_core::detect_init_system());
        }
        SystemCommands::Desktop => {
            println!("{}", hf_core::detect_desktop_environment());
        }
    }
    Ok(())
}
