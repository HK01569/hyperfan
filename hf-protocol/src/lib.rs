use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

/// Global request ID counter for correlation
static REQUEST_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Maximum message size for IPC (8KB)
pub const MAX_MESSAGE_SIZE: usize = 8 * 1024;

/// Maximum path length for security validation
const MAX_PATH_LENGTH: usize = 256;

/// Maximum EC register count per read operation
const MAX_EC_REGISTER_COUNT: u8 = 64;

const ALLOWED_PATH_PREFIXES: &[&str] = &["/sys/class/hwmon/", "/sys/devices/"];

const ALLOWED_VIRTUAL_PWM_PREFIXES: &[&str] = &["nvidia:", "amd:", "intel:"];

const FORBIDDEN_PATH_COMPONENTS: &[&str] = &[
    "..",      // Path traversal
    "//",      // Double slash (path normalization bypass)
    "\0",      // Null byte injection
    "\n",      // Newline injection
    "\r",      // Carriage return injection
    "$(",      // Command substitution
    "`",       // Command substitution (backtick)
    ";",       // Command chaining
    "|",       // Pipe
    "&",       // Background execution
    ">",       // Output redirection
    "<",       // Input redirection
    "\\",      // Backslash (escape sequences)
    "'",       // Single quote (shell injection)
    "\"",      // Double quote (shell injection)
];

/// Generate a unique request ID for correlation
pub fn generate_request_id() -> u64 {
    REQUEST_ID_COUNTER.fetch_add(1, Ordering::SeqCst)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestEnvelope {
    /// Unique request ID for correlation and debugging
    pub id: u64,
    /// The actual request
    #[serde(flatten)]
    pub request: Request,
}

impl RequestEnvelope {
    pub fn new(request: Request) -> Self {
        Self {
            id: generate_request_id(),
            request,
        }
    }
    
    pub fn with_id(request: Request, id: u64) -> Self {
        Self { id, request }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "data")]
pub enum Request {
    Ping,
    Version,
    ListHardware,
    /// Batch request: returns hardware + GPUs in single response (performance optimization)
    ListAll,
    ReadTemperature { path: String },
    ReadFanRpm { path: String },
    ReadPwm { path: String },
    SetPwm { path: String, value: u8 },
    EnableManualPwm { path: String },
    DisableManualPwm { path: String },
    SetPwmOverride { path: String, value: u8, ttl_ms: u32 },
    ClearPwmOverride { path: String },
    ListGpus,
    SetGpuFan { index: u32, fan_index: Option<u32>, percent: u32 },
    ResetGpuFanAuto { index: u32 },
    DetectFanMappings,
    ReloadConfig,
    GetManualPairings,
    SetManualPairing { 
        pwm_uuid: String,
        pwm_path: String, 
        fan_uuid: Option<String>,
        fan_path: Option<String>,
    },
    DeleteManualPairing { pwm_path: String },
    ListEcChips,
    ReadEcRegister { chip_path: String, register: u8 },
    WriteEcRegister { chip_path: String, register: u8, value: u8 },
    ReadEcRegisterRange { chip_path: String, start_register: u8, count: u8 },
    SetGlobalMode { mode: GlobalMode },
    GetGlobalMode,
    /// Get current daemon rate limit
    GetRateLimit,
    /// Set daemon rate limit (1500-9999 requests per 10s window)
    SetRateLimit { limit: u32 },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GlobalMode {
    /// Auto mode: fans follow control pairs
    Auto,
    /// Manual mode: all fans set to 30% for testing/pairing
    Manual,
}

impl Request {
    /// Validate request parameters before sending to daemon
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Request::Ping | Request::Version | Request::ListHardware 
            | Request::ListAll | Request::ListGpus | Request::DetectFanMappings 
            | Request::ReloadConfig | Request::GetManualPairings
            | Request::ListEcChips | Request::GetGlobalMode => Ok(()),
            
            Request::SetGlobalMode { mode: _ } => Ok(()),
            
            Request::GetRateLimit => Ok(()),
            Request::SetRateLimit { limit } => validate_rate_limit(*limit),
            
            Request::ReadTemperature { path } => validate_hwmon_path(path),
            Request::ReadFanRpm { path } => validate_hwmon_path(path),
            Request::ReadPwm { path } => validate_hwmon_path(path),
            
            Request::SetPwm { path, value } => {
                validate_pwm_target_path(path)?;
                validate_pwm_value(*value)?;
                Ok(())
            }
            
            Request::EnableManualPwm { path } => validate_pwm_target_path(path),
            Request::DisableManualPwm { path } => validate_pwm_target_path(path),
            
            Request::SetPwmOverride { path, value, ttl_ms } => {
                validate_pwm_target_path(path)?;
                validate_pwm_value(*value)?;
                validate_ttl_ms(*ttl_ms)?;
                Ok(())
            }
            
            Request::ClearPwmOverride { path } => validate_pwm_target_path(path),
            
            Request::SetGpuFan { index, fan_index, percent } => {
                validate_gpu_index(*index)?;
                if let Some(fi) = fan_index {
                    if *fi > 255 {
                        return Err("Fan index out of range (0-255)".into());
                    }
                }
                validate_percent(*percent)?;
                Ok(())
            }
            
            Request::ResetGpuFanAuto { index } => {
                validate_gpu_index(*index)?;
                Ok(())
            }
            
            Request::SetManualPairing { pwm_uuid: _, pwm_path, fan_uuid: _, fan_path } => {
                validate_pwm_target_path(pwm_path)?;
                if let Some(fp) = fan_path {
                    validate_hwmon_path(fp)?;
                }
                Ok(())
            }
            
            Request::DeleteManualPairing { pwm_path } => validate_pwm_target_path(pwm_path),
            
            Request::ReadEcRegister { chip_path, register: _ } => {
                validate_hwmon_path(chip_path)
            }
            
            Request::WriteEcRegister { chip_path, register: _, value: _ } => {
                validate_hwmon_path(chip_path)
            }
            
            Request::ReadEcRegisterRange { chip_path, start_register: _, count } => {
                validate_hwmon_path(chip_path)?;
                validate_ec_register_count(*count)?;
                Ok(())
            }
        }
    }
    
    pub fn type_name(&self) -> &'static str {
        match self {
            Request::Ping => "Ping",
            Request::Version => "Version",
            Request::ListHardware => "ListHardware",
            Request::ListAll => "ListAll",
            Request::ReadTemperature { .. } => "ReadTemperature",
            Request::ReadFanRpm { .. } => "ReadFanRpm",
            Request::ReadPwm { .. } => "ReadPwm",
            Request::SetPwm { .. } => "SetPwm",
            Request::EnableManualPwm { .. } => "EnableManualPwm",
            Request::DisableManualPwm { .. } => "DisableManualPwm",
            Request::SetPwmOverride { .. } => "SetPwmOverride",
            Request::ClearPwmOverride { .. } => "ClearPwmOverride",
            Request::ListGpus => "ListGpus",
            Request::SetGpuFan { .. } => "SetGpuFan",
            Request::ResetGpuFanAuto { .. } => "ResetGpuFanAuto",
            Request::DetectFanMappings => "DetectFanMappings",
            Request::ReloadConfig => "ReloadConfig",
            Request::GetManualPairings => "GetManualPairings",
            Request::SetManualPairing { .. } => "SetManualPairing",
            Request::DeleteManualPairing { .. } => "DeleteManualPairing",
            Request::ListEcChips => "ListEcChips",
            Request::ReadEcRegister { .. } => "ReadEcRegister",
            Request::WriteEcRegister { .. } => "WriteEcRegister",
            Request::ReadEcRegisterRange { .. } => "ReadEcRegisterRange",
            Request::SetGlobalMode { .. } => "SetGlobalMode",
            Request::GetGlobalMode => "GetGlobalMode",
            Request::GetRateLimit => "GetRateLimit",
            Request::SetRateLimit { .. } => "SetRateLimit",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseEnvelope {
    /// Request ID this response corresponds to
    pub id: u64,
    /// The actual response
    #[serde(flatten)]
    pub response: Response,
}

impl ResponseEnvelope {
    pub fn new(id: u64, response: Response) -> Self {
        Self { id, response }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum Response {
    #[serde(rename = "ok")]
    Ok(ResponseData),
    #[serde(rename = "error")]
    Error { message: String },
}

/// Response data - each variant has a unique structure that serde can distinguish
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub celsius: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rpm: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pwm: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hardware: Option<HardwareInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpus: Option<Vec<GpuInfo>>,
    /// Batched response: hardware + GPUs combined (for ListAll)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub all_data: Option<AllHardwareData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fan_mappings: Option<Vec<FanMapping>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manual_pairings: Option<Vec<ManualPwmFanPairing>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ec_chips: Option<Vec<EcChipInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ec_register: Option<EcRegisterValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ec_registers: Option<Vec<EcRegisterValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub global_mode: Option<GlobalMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<u32>,
}

impl Default for ResponseData {
    fn default() -> Self {
        Self {
            value: None,
            celsius: None,
            rpm: None,
            pwm: None,
            hardware: None,
            gpus: None,
            all_data: None,
            fan_mappings: None,
            manual_pairings: None,
            ec_chips: None,
            ec_register: None,
            ec_registers: None,
            global_mode: None,
            rate_limit: None,
        }
    }
}

impl ResponseData {
    pub fn none() -> Self { Self::default() }
    pub fn string(v: String) -> Self { Self { value: Some(v), ..Self::default() } }
    pub fn temperature(c: f32) -> Self { Self { celsius: Some(c), ..Self::default() } }
    pub fn fan_rpm(r: u32) -> Self { Self { rpm: Some(r), ..Self::default() } }
    pub fn pwm_value(p: u8) -> Self { Self { pwm: Some(p), ..Self::default() } }
    pub fn hw(h: HardwareInfo) -> Self { Self { hardware: Some(h), ..Self::default() } }
    pub fn gpu_list(g: Vec<GpuInfo>) -> Self { Self { gpus: Some(g), ..Self::default() } }
    pub fn all(data: AllHardwareData) -> Self { Self { all_data: Some(data), ..Self::default() } }
    pub fn mappings(m: Vec<FanMapping>) -> Self { Self { fan_mappings: Some(m), ..Self::default() } }
    pub fn pairings(p: Vec<ManualPwmFanPairing>) -> Self { Self { manual_pairings: Some(p), ..Self::default() } }
    pub fn chips(c: Vec<EcChipInfo>) -> Self { Self { ec_chips: Some(c), ..Self::default() } }
    pub fn register(r: EcRegisterValue) -> Self { Self { ec_register: Some(r), ..Self::default() } }
    pub fn registers(r: Vec<EcRegisterValue>) -> Self { Self { ec_registers: Some(r), ..Self::default() } }
    pub fn mode(m: GlobalMode) -> Self { Self { global_mode: Some(m), ..Self::default() } }
    pub fn rate_limit(r: u32) -> Self { Self { rate_limit: Some(r), ..Self::default() } }
}

/// Batched hardware data (hwmon + GPUs) for efficient polling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllHardwareData {
    pub hardware: HardwareInfo,
    pub gpus: Vec<GpuInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareInfo {
    pub chips: Vec<HwmonChip>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HwmonChip {
    pub name: String,
    pub path: String,
    pub temperatures: Vec<TempSensor>,
    pub fans: Vec<FanSensor>,
    pub pwms: Vec<PwmControl>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempSensor {
    pub name: String,
    pub label: Option<String>,
    pub path: String,
    pub value: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FanSensor {
    /// Unique identifier for this fan sensor (stable across reboots)
    pub uuid: String,
    pub name: String,
    pub label: Option<String>,
    pub path: String,
    pub rpm: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PwmControl {
    /// Unique identifier for this PWM control (stable across reboots)
    pub uuid: String,
    pub name: String,
    pub path: String,
    pub value: u8,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    pub index: u32,
    pub name: String,
    pub vendor: String,
    pub temp: Option<f32>,
    pub fan_percent: Option<u32>,
    pub fan_rpm: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FanMapping {
    /// UUID of the PWM control
    pub pwm_uuid: String,
    pub pwm_path: String,
    /// UUID of the fan sensor
    pub fan_uuid: String,
    pub fan_path: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualPwmFanPairing {
    /// UUID of the PWM control (primary key for matching)
    pub pwm_uuid: String,
    pub pwm_path: String,
    pub pwm_name: String,
    /// UUID of the paired fan sensor (None if unpaired)
    pub fan_uuid: Option<String>,
    pub fan_path: Option<String>,
    pub fan_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EcChipInfo {
    pub name: String,
    pub path: String,
    pub device_path: Option<String>,
    pub chip_class: String,
    pub register_count: Option<u16>,
    pub supports_direct_access: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EcRegisterValue {
    pub register: u8,
    pub value: u8,
    pub label: Option<String>,
    pub writable: bool,
}

impl Response {
    pub fn ok() -> Self {
        Response::Ok(ResponseData::none())
    }

    pub fn ok_string(s: impl Into<String>) -> Self {
        Response::Ok(ResponseData::string(s.into()))
    }

    pub fn ok_temp(t: f32) -> Self {
        Response::Ok(ResponseData::temperature(t))
    }

    pub fn ok_rpm(r: u32) -> Self {
        Response::Ok(ResponseData::fan_rpm(r))
    }

    pub fn ok_pwm(v: u8) -> Self {
        Response::Ok(ResponseData::pwm_value(v))
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Response::Error { message: msg.into() }
    }
}

pub fn validate_hwmon_path(path: &str) -> Result<(), String> {
    if path.len() > MAX_PATH_LENGTH {
        return Err(format!(
            "Path too long: {} > {} chars",
            path.len(),
            MAX_PATH_LENGTH
        ));
    }

    if path.is_empty() {
        return Err("Path cannot be empty".into());
    }

    if !path.starts_with('/') {
        return Err("Path must be absolute".into());
    }

    let allowed = ALLOWED_PATH_PREFIXES
        .iter()
        .any(|prefix| path.starts_with(prefix));
    if !allowed {
        return Err(format!("Path must be under one of: {:?}", ALLOWED_PATH_PREFIXES));
    }

    for forbidden in FORBIDDEN_PATH_COMPONENTS {
        if path.contains(forbidden) {
            return Err(format!("Path contains forbidden sequence: {:?}", forbidden));
        }
    }

    for c in path.chars() {
        if !c.is_ascii_alphanumeric() && c != '/' && c != '-' && c != '_' && c != '.' {
            return Err(format!("Path contains invalid character: {:?}", c));
        }
    }

    if let Ok(canonical) = std::fs::canonicalize(path) {
        let canonical_str = canonical.to_string_lossy();
        let canonical_allowed = ALLOWED_PATH_PREFIXES
            .iter()
            .any(|prefix| canonical_str.starts_with(prefix));

        if !canonical_allowed {
            return Err(format!(
                "Canonical path {} is outside allowed directories",
                canonical_str
            ));
        }
    }

    Ok(())
}

pub fn validate_pwm_target_path(path: &str) -> Result<(), String> {
    if ALLOWED_VIRTUAL_PWM_PREFIXES.iter().any(|p| path.starts_with(p)) {
        return validate_virtual_pwm_path(path);
    }

    validate_hwmon_path(path)
}

fn validate_virtual_pwm_path(path: &str) -> Result<(), String> {
    if path.len() > MAX_PATH_LENGTH {
        return Err(format!(
            "Path too long: {} > {} chars",
            path.len(),
            MAX_PATH_LENGTH
        ));
    }

    if path.is_empty() {
        return Err("Path cannot be empty".into());
    }

    for forbidden in FORBIDDEN_PATH_COMPONENTS {
        if path.contains(forbidden) {
            return Err(format!("Path contains forbidden sequence: {:?}", forbidden));
        }
    }

    if path.starts_with("nvidia:") {
        return validate_nvidia_pwm_path(path);
    }

    for c in path.chars() {
        if !c.is_ascii_alphanumeric() && c != ':' && c != '-' && c != '_' && c != '.' {
            return Err(format!("Path contains invalid character: {:?}", c));
        }
    }

    Ok(())
}

fn validate_nvidia_pwm_path(path: &str) -> Result<(), String> {
    let parts: Vec<&str> = path.split(':').collect();
    if parts.len() != 3 {
        return Err("Invalid NVIDIA PWM path format".into());
    }

    let gpu_index: u32 = parts[1]
        .parse()
        .map_err(|_| "Invalid NVIDIA GPU index".to_string())?;
    validate_gpu_index(gpu_index).map_err(|e| e.to_string())?;

    let fan_index: u32 = parts[2]
        .parse()
        .map_err(|_| "Invalid NVIDIA fan index".to_string())?;
    if fan_index > 255 {
        return Err("NVIDIA fan index out of range (0-255)".into());
    }

    Ok(())
}

pub fn validate_gpu_index(index: u32) -> Result<(), &'static str> {
    if index > 255 {
        return Err("GPU index out of range (0-255)");
    }
    Ok(())
}

pub fn validate_percent(percent: u32) -> Result<(), &'static str> {
    if percent > 100 {
        return Err("Percent must be 0-100");
    }
    Ok(())
}

pub fn validate_pwm_value(_value: u8) -> Result<(), &'static str> {
    // u8 type already constrains to 0-255
    Ok(())
}

pub fn validate_ttl_ms(ttl_ms: u32) -> Result<(), &'static str> {
    if ttl_ms < 50 {
        return Err("TTL too short (minimum 50ms)");
    }
    if ttl_ms > 30_000 {
        return Err("TTL too long (maximum 30000ms)");
    }
    Ok(())
}

pub fn validate_ec_register_count(count: u8) -> Result<(), &'static str> {
    if count == 0 {
        return Err("Register count must be at least 1");
    }
    if count > MAX_EC_REGISTER_COUNT {
        return Err("Register count exceeds maximum (64)");
    }
    Ok(())
}

/// Minimum rate limit (requests per 10s window)
pub const MIN_RATE_LIMIT: u32 = 1500;

/// Maximum rate limit (requests per 10s window)
pub const MAX_RATE_LIMIT: u32 = 9999;

pub fn validate_rate_limit(limit: u32) -> Result<(), String> {
    if limit < MIN_RATE_LIMIT {
        return Err(format!("Rate limit too low (minimum {})", MIN_RATE_LIMIT));
    }
    if limit > MAX_RATE_LIMIT {
        return Err(format!("Rate limit too high (maximum {})", MAX_RATE_LIMIT));
    }
    Ok(())
}
