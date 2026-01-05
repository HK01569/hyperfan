//! Daemon Client
//!
//! Communicates with the privileged hyperfand daemon via Unix socket.
//! Provides a safe interface for hardware operations without requiring root.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::cell::RefCell;

use crate::service::get_socket_path;

const TIMEOUT_MS: u64 = 5000;

const MAX_MESSAGE_SIZE: usize = hf_protocol::MAX_MESSAGE_SIZE;

/// Initial buffer size for responses (ListAll responses are ~4KB typically)
const INITIAL_BUFFER_SIZE: usize = 4096;

/// Default client-side rate limit: maximum requests per window
/// At 100ms poll interval, we need ~100 req/10s just for sensor polling
/// Plus additional requests for UI interactions, so allow 1500 req/10s
const DEFAULT_CLIENT_RATE_LIMIT: u32 = 1500;

/// Minimum rate limit (cannot go below this)
pub const MIN_RATE_LIMIT: u32 = hf_protocol::MIN_RATE_LIMIT;

/// Maximum rate limit (cannot exceed this)  
pub const MAX_RATE_LIMIT: u32 = hf_protocol::MAX_RATE_LIMIT;

/// Current client-side rate limit (configurable at runtime)
static CLIENT_RATE_LIMIT: AtomicU32 = AtomicU32::new(DEFAULT_CLIENT_RATE_LIMIT);

/// Client-side rate limit window duration
const CLIENT_RATE_LIMIT_WINDOW: Duration = Duration::from_secs(10);

/// Global rate limiter state
static RATE_LIMITER: Mutex<Option<ClientRateLimiter>> = Mutex::new(None);

/// Thread-local connection pool for reusing daemon connections
/// This eliminates the overhead of creating a new connection for every request
thread_local! {
    static CONNECTION_POOL: RefCell<Option<DaemonClient>> = RefCell::new(None);
}

/// Client-side rate limiter to prevent overwhelming the daemon
struct ClientRateLimiter {
    request_count: u32,
    window_start: Instant,
}

impl ClientRateLimiter {
    fn new() -> Self {
        Self {
            request_count: 0,
            window_start: Instant::now(),
        }
    }
    
    fn check_and_increment(&mut self) -> Result<(), String> {
        let now = Instant::now();
        
        // Reset window if expired
        if now.duration_since(self.window_start) > CLIENT_RATE_LIMIT_WINDOW {
            self.request_count = 0;
            self.window_start = now;
        }
        
        let current_limit = CLIENT_RATE_LIMIT.load(Ordering::Relaxed);
        if self.request_count >= current_limit {
            let wait_time = CLIENT_RATE_LIMIT_WINDOW
                .checked_sub(now.duration_since(self.window_start))
                .unwrap_or(Duration::from_secs(0));
            return Err(format!(
                "Client rate limit exceeded ({} req/{}s). Retry in {:.1}s",
                current_limit,
                CLIENT_RATE_LIMIT_WINDOW.as_secs(),
                wait_time.as_secs_f32()
            ));
        }
        
        self.request_count += 1;
        Ok(())
    }
}

/// Check client-side rate limit before sending request
fn check_rate_limit() -> Result<(), String> {
    let mut limiter_guard = RATE_LIMITER.lock()
        .map_err(|e| format!("Rate limiter mutex poisoned: {}. This indicates a previous panic in the rate limiter.", e))?;
    let limiter = limiter_guard.get_or_insert_with(ClientRateLimiter::new);
    limiter.check_and_increment()
}

pub type DaemonRequest = hf_protocol::Request;
pub type DaemonResponse = hf_protocol::Response;
pub type DaemonResponseData = hf_protocol::ResponseData;

pub type DaemonHardwareInfo = hf_protocol::HardwareInfo;
pub type DaemonHwmonChip = hf_protocol::HwmonChip;
pub type DaemonTempSensor = hf_protocol::TempSensor;
pub type DaemonFanSensor = hf_protocol::FanSensor;
pub type DaemonPwmControl = hf_protocol::PwmControl;
pub type DaemonGpuInfo = hf_protocol::GpuInfo;
pub type DaemonFanMapping = hf_protocol::FanMapping;
pub type DaemonManualPwmFanPairing = hf_protocol::ManualPwmFanPairing;
pub type DaemonEcChipInfo = hf_protocol::EcChipInfo;
pub type DaemonEcRegisterValue = hf_protocol::EcRegisterValue;
pub type DaemonAllHardwareData = hf_protocol::AllHardwareData;

/// Daemon client for making requests
pub struct DaemonClient {
    writer: UnixStream,
    reader: BufReader<UnixStream>,
}

impl DaemonClient {
    /// Get or create a pooled connection (S-tier optimization)
    /// Reuses existing connection if healthy, otherwise creates new one
    pub fn get_pooled() -> Result<Self, String> {
        CONNECTION_POOL.with(|pool| {
            let mut pool_ref = pool.borrow_mut();
            
            // Check if we have a cached connection
            if let Some(client) = pool_ref.take() {
                // Verify connection is still healthy with a quick ping
                if client.is_healthy() {
                    // Connection is good, return it
                    return Ok(client);
                }
                // Connection is dead, will create new one below
            }
            
            // No cached connection or it's dead - create new one
            Self::connect()
        })
    }
    
    /// Return connection to pool for reuse (S-tier optimization)
    pub fn return_to_pool(self) {
        CONNECTION_POOL.with(|pool| {
            *pool.borrow_mut() = Some(self);
        });
    }
    
    /// Check if connection is still healthy
    /// Returns false if socket has errors OR if there's stale data in the buffer
    fn is_healthy(&self) -> bool {
        use std::os::unix::io::AsRawFd;
        let fd = self.writer.as_raw_fd();
        
        // Check if socket is still connected using SO_ERROR
        let mut error: libc::c_int = 0;
        let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
        
        // SAFETY: getsockopt is safe when:
        // 1. fd is a valid socket file descriptor - guaranteed by UnixStream ownership
        // 2. error is a properly initialized c_int - initialized to 0 above
        // 3. len is set to the correct size of the error variable
        // 4. SOL_SOCKET and SO_ERROR are valid socket option constants
        // The function only reads the socket error state without modifying socket behavior.
        let socket_ok = unsafe {
            let result = libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_ERROR,
                &mut error as *mut _ as *mut libc::c_void,
                &mut len,
            );
            result == 0 && error == 0
        };
        
        if !socket_ok {
            return false;
        }
        
        // Check if there's stale data in the read buffer
        // If the buffer has data, the connection is "dirty" and should be discarded
        // This prevents reading stale responses from previous requests
        self.reader.buffer().is_empty()
    }
    
    /// Connect to the daemon (internal)
    fn connect() -> Result<Self, String> {
        let socket_path = get_socket_path();
        let stream = UnixStream::connect(socket_path)
            .map_err(|e| format!("Failed to connect to daemon at {}: {}", socket_path, e))?;

        let reader_stream = stream
            .try_clone()
            .map_err(|e| format!("Failed to clone daemon socket for reader: {}", e))?;

        stream
            .set_read_timeout(Some(Duration::from_millis(TIMEOUT_MS)))
            .map_err(|e| format!("Failed to set read timeout: {}", e))?;
        stream
            .set_write_timeout(Some(Duration::from_millis(TIMEOUT_MS)))
            .map_err(|e| format!("Failed to set write timeout: {}", e))?;

        reader_stream
            .set_read_timeout(Some(Duration::from_millis(TIMEOUT_MS)))
            .map_err(|e| format!("Failed to set read timeout: {}", e))?;
        reader_stream
            .set_write_timeout(Some(Duration::from_millis(TIMEOUT_MS)))
            .map_err(|e| format!("Failed to set write timeout: {}", e))?;

        Ok(Self {
            writer: stream,
            reader: BufReader::new(reader_stream),
        })
    }

    /// Send a request and get response (with automatic retry on connection failure)
    pub fn request(&mut self, req: DaemonRequest) -> Result<DaemonResponse, String> {
        self.request_with_retry(req, true)
    }
    
    /// Internal request with retry logic
    fn request_with_retry(&mut self, req: DaemonRequest, allow_retry: bool) -> Result<DaemonResponse, String> {
        // Check client-side rate limit
        check_rate_limit()?;
        
        // Validate request parameters before sending
        req.validate()
            .map_err(|e| format!("Request validation failed: {}", e))?;
        
        // Wrap request in envelope with unique ID
        // PERF: Avoid clone by moving req into envelope, get ID first
        let request_id = hf_protocol::generate_request_id();
        let envelope = hf_protocol::RequestEnvelope::with_id(req.clone(), request_id);
        
        // Serialize request envelope
        // PERF: Use to_vec to avoid intermediate String allocation
        let mut json = serde_json::to_vec(&envelope)
            .map_err(|e| format!("Failed to serialize request: {}", e))?;
        json.push(b'\n');

        if json.len() > MAX_MESSAGE_SIZE {
            return Err(crate::error::HyperfanError::MessageTooLarge {
                size: json.len(),
                max_size: MAX_MESSAGE_SIZE
            }.to_string());
        }

        // Send (with automatic reconnect on failure)
        if let Err(e) = self.writer.write_all(&json) {
            if allow_retry {
                // Connection failed - try to reconnect and retry once
                *self = Self::connect()
                    .map_err(|e2| format!("Failed to reconnect after send error: {}", e2))?;
                return self.request_with_retry(req, false);
            }
            return Err(format!("Failed to send request: {}", e));
        }

        // Read response with efficient buffer allocation (with automatic reconnect on failure)
        let mut response_buf: Vec<u8> = Vec::with_capacity(INITIAL_BUFFER_SIZE);
        if let Err(e) = self.reader.read_until(b'\n', &mut response_buf) {
            if allow_retry {
                // Connection failed - try to reconnect and retry once
                *self = Self::connect()
                    .map_err(|e2| format!("Failed to reconnect after read error: {}", e2))?;
                return self.request_with_retry(req, false);
            }
            return Err(format!("Failed to read response: {}", e));
        }

        if response_buf.is_empty() {
            return Err(crate::error::HyperfanError::DaemonConnection("Daemon closed connection".to_string()).to_string());
        }

        if response_buf.len() > MAX_MESSAGE_SIZE {
            return Err(crate::error::HyperfanError::MessageTooLarge {
                size: response_buf.len(),
                max_size: MAX_MESSAGE_SIZE
            }.to_string());
        }

        // PERF: Parse directly from bytes, skip UTF-8 string conversion
        // Trim trailing newline in-place
        if response_buf.last() == Some(&b'\n') {
            response_buf.pop();
        }
        
        // Parse response envelope directly from bytes
        let response_envelope: hf_protocol::ResponseEnvelope = serde_json::from_slice(&response_buf)
            .map_err(|e| format!("Failed to parse response: {}", e))?;
        
        // Verify response ID matches request ID
        if response_envelope.id != request_id {
            return Err(format!(
                "Response ID mismatch: expected {}, got {}",
                request_id, response_envelope.id
            ));
        }
        
        // Verify response type matches request expectations
        Self::verify_response_type(&req, &response_envelope.response)?;
        
        Ok(response_envelope.response)
    }
    
    /// Verify that response type matches the request
    fn verify_response_type(req: &DaemonRequest, resp: &DaemonResponse) -> Result<(), String> {
        match resp {
            DaemonResponse::Error { .. } => Ok(()), // Errors are always valid
            DaemonResponse::Ok(data) => {
                // ResponseData is now a flat struct - validation is simpler
                let valid = match req {
                    DaemonRequest::Ping | DaemonRequest::Version => data.value.is_some(),
                    DaemonRequest::ListHardware => data.hardware.is_some(),
                    DaemonRequest::ListAll => data.all_data.is_some(),
                    DaemonRequest::ReadTemperature { .. } => data.celsius.is_some(),
                    DaemonRequest::ReadFanRpm { .. } => data.rpm.is_some(),
                    DaemonRequest::ReadPwm { .. } => data.pwm.is_some(),
                    DaemonRequest::ListGpus => data.gpus.is_some(),
                    DaemonRequest::DetectFanMappings => data.fan_mappings.is_some(),
                    DaemonRequest::GetManualPairings => data.manual_pairings.is_some(),
                    DaemonRequest::ListEcChips => data.ec_chips.is_some(),
                    DaemonRequest::ReadEcRegister { .. } => data.ec_register.is_some(),
                    DaemonRequest::ReadEcRegisterRange { .. } => data.ec_registers.is_some(),
                    // Commands that return empty response
                    _ => true,
                };
                
                if !valid {
                    return Err(format!(
                        "Response type mismatch: got {:?} for request {}",
                        data,
                        req.type_name()
                    ));
                }
                
                Ok(())
            }
        }
    }
}

/// Check if daemon is available (socket exists)
pub fn is_daemon_available() -> bool {
    std::path::Path::new(get_socket_path()).exists()
}

/// Ping the daemon to check connectivity
pub fn ping_daemon() -> Result<(), String> {
    let mut client = DaemonClient::get_pooled()?;
    let result = match client.request(DaemonRequest::Ping)? {
        DaemonResponse::Ok(_) => Ok(()),
        DaemonResponse::Error { message } => Err(crate::error::HyperfanError::DaemonResponse(message).to_string()),
    };
    client.return_to_pool();
    result
}

/// Get daemon version
pub fn get_daemon_version() -> Result<String, String> {
    let mut client = DaemonClient::get_pooled()?;
    let result = match client.request(DaemonRequest::Version)? {
        DaemonResponse::Ok(data) if data.value.is_some() => Ok(data.value.unwrap()),
        DaemonResponse::Ok(_) => Err(crate::error::HyperfanError::IpcProtocol("Unexpected response type".to_string()).to_string()),
        DaemonResponse::Error { message } => Err(crate::error::HyperfanError::DaemonResponse(message).to_string()),
    };
    client.return_to_pool();
    result
}

/// Read temperature via daemon
pub fn daemon_read_temperature(path: &str) -> Result<f32, String> {
    let mut client = DaemonClient::get_pooled()?;
    let result = match client.request(DaemonRequest::ReadTemperature {
        path: path.to_string(),
    })? {
        DaemonResponse::Ok(data) if data.celsius.is_some() => Ok(data.celsius.unwrap()),
        DaemonResponse::Ok(_) => Err(crate::error::HyperfanError::IpcProtocol("Unexpected response type".to_string()).to_string()),
        DaemonResponse::Error { message } => Err(crate::error::HyperfanError::DaemonResponse(message).to_string()),
    };
    client.return_to_pool();
    result
}

/// Read fan RPM via daemon
pub fn daemon_read_fan_rpm(path: &str) -> Result<u32, String> {
    let mut client = DaemonClient::get_pooled()?;
    let result = match client.request(DaemonRequest::ReadFanRpm {
        path: path.to_string(),
    })? {
        DaemonResponse::Ok(data) if data.rpm.is_some() => Ok(data.rpm.unwrap()),
        DaemonResponse::Ok(_) => Err(crate::error::HyperfanError::IpcProtocol("Unexpected response type".to_string()).to_string()),
        DaemonResponse::Error { message } => Err(crate::error::HyperfanError::DaemonResponse(message).to_string()),
    };
    client.return_to_pool();
    result
}

/// Read PWM value via daemon
pub fn daemon_read_pwm(path: &str) -> Result<u8, String> {
    let mut client = DaemonClient::get_pooled()?;
    let result = match client.request(DaemonRequest::ReadPwm {
        path: path.to_string(),
    })? {
        DaemonResponse::Ok(data) if data.pwm.is_some() => Ok(data.pwm.unwrap()),
        DaemonResponse::Ok(_) => Err(crate::error::HyperfanError::IpcProtocol("Unexpected response type".to_string()).to_string()),
        DaemonResponse::Error { message } => Err(crate::error::HyperfanError::DaemonResponse(message).to_string()),
    };
    client.return_to_pool();
    result
}

/// Set PWM value via daemon
pub fn daemon_set_pwm(path: &str, value: u8) -> Result<(), String> {
    let mut client = DaemonClient::get_pooled()?;
    let result = match client.request(DaemonRequest::SetPwm {
        path: path.to_string(),
        value,
    })? {
        DaemonResponse::Ok(_) => Ok(()),
        DaemonResponse::Error { message } => Err(crate::error::HyperfanError::DaemonResponse(message).to_string()),
    };
    client.return_to_pool();
    result
}

/// Enable manual PWM control via daemon
pub fn daemon_enable_manual_pwm(path: &str) -> Result<(), String> {
    let mut client = DaemonClient::get_pooled()?;
    let result = match client.request(DaemonRequest::EnableManualPwm {
        path: path.to_string(),
    })? {
        DaemonResponse::Ok(_) => Ok(()),
        DaemonResponse::Error { message } => Err(crate::error::HyperfanError::DaemonResponse(message).to_string()),
    };
    client.return_to_pool();
    result
}

/// Disable manual PWM control via daemon (return to automatic)
pub fn daemon_disable_manual_pwm(path: &str) -> Result<(), String> {
    let mut client = DaemonClient::get_pooled()?;
    let result = match client.request(DaemonRequest::DisableManualPwm {
        path: path.to_string(),
    })? {
        DaemonResponse::Ok(_) => Ok(()),
        DaemonResponse::Error { message } => Err(crate::error::HyperfanError::DaemonResponse(message).to_string()),
    };
    client.return_to_pool();
    result
}

/// Set a temporary PWM override via daemon (0-255) for live preview.
/// ttl_ms controls how long the override is respected by the daemon loop.
pub fn daemon_set_pwm_override(path: &str, value: u8, ttl_ms: u32) -> Result<(), String> {
    let mut client = DaemonClient::get_pooled()?;
    let result = match client.request(DaemonRequest::SetPwmOverride {
        path: path.to_string(),
        value,
        ttl_ms,
    })? {
        DaemonResponse::Ok(_) => Ok(()),
        DaemonResponse::Error { message } => Err(crate::error::HyperfanError::DaemonResponse(message).to_string()),
    };
    client.return_to_pool();
    result
}

/// Clear a PWM override via daemon.
pub fn daemon_clear_pwm_override(path: &str) -> Result<(), String> {
    let mut client = DaemonClient::get_pooled()?;
    let result = match client.request(DaemonRequest::ClearPwmOverride {
        path: path.to_string(),
    })? {
        DaemonResponse::Ok(_) => Ok(()),
        DaemonResponse::Error { message } => Err(crate::error::HyperfanError::DaemonResponse(message).to_string()),
    };
    client.return_to_pool();
    result
}

/// List all hardware via daemon
pub fn daemon_list_hardware() -> Result<DaemonHardwareInfo, String> {
    let mut client = DaemonClient::get_pooled()?;
    let result = match client.request(DaemonRequest::ListHardware)? {
        DaemonResponse::Ok(data) if data.hardware.is_some() => Ok(data.hardware.unwrap()),
        DaemonResponse::Ok(_) => Err(crate::error::HyperfanError::IpcProtocol("Unexpected response type".to_string()).to_string()),
        DaemonResponse::Error { message } => Err(crate::error::HyperfanError::DaemonResponse(message).to_string()),
    };
    client.return_to_pool();
    result
}

/// List all hardware + GPUs in single request (performance optimization)
/// Reduces IPC round-trips from 2 to 1 for polling
pub fn daemon_list_all() -> Result<DaemonAllHardwareData, String> {
    let mut client = DaemonClient::get_pooled()?;
    let result = match client.request(DaemonRequest::ListAll)? {
        DaemonResponse::Ok(data) if data.all_data.is_some() => Ok(data.all_data.unwrap()),
        DaemonResponse::Ok(_) => Err(crate::error::HyperfanError::IpcProtocol("Unexpected response type".to_string()).to_string()),
        DaemonResponse::Error { message } => Err(crate::error::HyperfanError::DaemonResponse(message).to_string()),
    };
    client.return_to_pool();
    result
}

/// List GPUs via daemon
pub fn daemon_list_gpus() -> Result<Vec<DaemonGpuInfo>, String> {
    let mut client = DaemonClient::get_pooled()?;
    let result = match client.request(DaemonRequest::ListGpus)? {
        DaemonResponse::Ok(data) if data.gpus.is_some() => Ok(data.gpus.unwrap()),
        DaemonResponse::Ok(_) => Err(crate::error::HyperfanError::IpcProtocol("Unexpected response type".to_string()).to_string()),
        DaemonResponse::Error { message } => Err(crate::error::HyperfanError::DaemonResponse(message).to_string()),
    };
    client.return_to_pool();
    result
}

/// Set GPU fan speed via daemon
pub fn daemon_set_gpu_fan(index: u32, percent: u32) -> Result<(), String> {
    let mut client = DaemonClient::get_pooled()?;
    let result = match client.request(DaemonRequest::SetGpuFan {
        index,
        fan_index: None,
        percent,
    })? {
        DaemonResponse::Ok(_) => Ok(()),
        DaemonResponse::Error { message } => Err(crate::error::HyperfanError::DaemonResponse(message).to_string()),
    };
    client.return_to_pool();
    result
}

/// Set a specific GPU fan speed via daemon
pub fn daemon_set_gpu_fan_for_fan(index: u32, fan_index: u32, percent: u32) -> Result<(), String> {
    let mut client = DaemonClient::get_pooled()?;
    let result = match client.request(DaemonRequest::SetGpuFan {
        index,
        fan_index: Some(fan_index),
        percent,
    })? {
        DaemonResponse::Ok(_) => Ok(()),
        DaemonResponse::Error { message } => Err(crate::error::HyperfanError::DaemonResponse(message).to_string()),
    };
    client.return_to_pool();
    result
}

/// Reset GPU fan control back to automatic mode via daemon
pub fn daemon_reset_gpu_fan_auto(index: u32) -> Result<(), String> {
    let mut client = DaemonClient::get_pooled()?;
    let result = match client.request(DaemonRequest::ResetGpuFanAuto { index })? {
        DaemonResponse::Ok(_) => Ok(()),
        DaemonResponse::Error { message } => Err(crate::error::HyperfanError::DaemonResponse(message).to_string()),
    };
    client.return_to_pool();
    result
}

/// Detect fan mappings via daemon
pub fn daemon_detect_fan_mappings() -> Result<Vec<DaemonFanMapping>, String> {
    let mut client = DaemonClient::get_pooled()?;
    let result = match client.request(DaemonRequest::DetectFanMappings)? {
        DaemonResponse::Ok(data) if data.fan_mappings.is_some() => Ok(data.fan_mappings.unwrap()),
        DaemonResponse::Ok(_) => Err(crate::error::HyperfanError::IpcProtocol("Unexpected response type".to_string()).to_string()),
        DaemonResponse::Error { message } => Err(crate::error::HyperfanError::DaemonResponse(message).to_string()),
    };
    client.return_to_pool();
    result
}

/// Tell daemon to reload its configuration
pub fn daemon_reload_config() -> Result<(), String> {
    let mut client = DaemonClient::get_pooled()?;
    let result = match client.request(DaemonRequest::ReloadConfig)? {
        DaemonResponse::Ok(_) => Ok(()),
        DaemonResponse::Error { message } => Err(crate::error::HyperfanError::DaemonResponse(message).to_string()),
    };
    client.return_to_pool();
    result
}

pub fn daemon_get_manual_pairings() -> Result<Vec<DaemonManualPwmFanPairing>, String> {
    let mut client = DaemonClient::get_pooled()?;
    let result = match client.request(DaemonRequest::GetManualPairings)? {
        DaemonResponse::Ok(data) if data.manual_pairings.is_some() => Ok(data.manual_pairings.unwrap()),
        DaemonResponse::Ok(_) => Err(crate::error::HyperfanError::IpcProtocol("Unexpected response type".to_string()).to_string()),
        DaemonResponse::Error { message } => Err(crate::error::HyperfanError::DaemonResponse(message).to_string()),
    };
    client.return_to_pool();
    result
}

pub fn daemon_set_manual_pairing(
    pwm_uuid: &str,
    pwm_path: &str, 
    fan_uuid: Option<&str>,
    fan_path: Option<&str>,
) -> Result<(), String> {
    let mut client = DaemonClient::get_pooled()?;
    let result = match client.request(DaemonRequest::SetManualPairing {
        pwm_uuid: pwm_uuid.to_string(),
        pwm_path: pwm_path.to_string(),
        fan_uuid: fan_uuid.map(|s| s.to_string()),
        fan_path: fan_path.map(|s| s.to_string()),
    })? {
        DaemonResponse::Ok(_) => Ok(()),
        DaemonResponse::Error { message } => Err(crate::error::HyperfanError::DaemonResponse(message).to_string()),
    };
    client.return_to_pool();
    result
}

pub fn daemon_delete_manual_pairing(pwm_path: &str) -> Result<(), String> {
    let mut client = DaemonClient::get_pooled()?;
    let result = match client.request(DaemonRequest::DeleteManualPairing {
        pwm_path: pwm_path.to_string(),
    })? {
        DaemonResponse::Ok(_) => Ok(()),
        DaemonResponse::Error { message } => Err(crate::error::HyperfanError::DaemonResponse(message).to_string()),
    };
    client.return_to_pool();
    result
}

pub fn daemon_list_ec_chips() -> Result<Vec<DaemonEcChipInfo>, String> {
    let mut client = DaemonClient::get_pooled()?;
    let result = match client.request(DaemonRequest::ListEcChips)? {
        DaemonResponse::Ok(data) if data.ec_chips.is_some() => Ok(data.ec_chips.unwrap()),
        DaemonResponse::Ok(_) => Err(crate::error::HyperfanError::IpcProtocol("Unexpected response type".to_string()).to_string()),
        DaemonResponse::Error { message } => Err(crate::error::HyperfanError::DaemonResponse(message).to_string()),
    };
    client.return_to_pool();
    result
}

pub fn daemon_read_ec_register(chip_path: &str, register: u8) -> Result<DaemonEcRegisterValue, String> {
    let mut client = DaemonClient::get_pooled()?;
    let result = match client.request(DaemonRequest::ReadEcRegister {
        chip_path: chip_path.to_string(),
        register,
    })? {
        DaemonResponse::Ok(data) if data.ec_register.is_some() => Ok(data.ec_register.unwrap()),
        DaemonResponse::Ok(_) => Err(crate::error::HyperfanError::IpcProtocol("Unexpected response type".to_string()).to_string()),
        DaemonResponse::Error { message } => Err(crate::error::HyperfanError::DaemonResponse(message).to_string()),
    };
    client.return_to_pool();
    result
}

pub fn daemon_write_ec_register(chip_path: &str, register: u8, value: u8) -> Result<(), String> {
    let mut client = DaemonClient::get_pooled()?;
    let result = match client.request(DaemonRequest::WriteEcRegister {
        chip_path: chip_path.to_string(),
        register,
        value,
    })? {
        DaemonResponse::Ok(_) => Ok(()),
        DaemonResponse::Error { message } => Err(crate::error::HyperfanError::DaemonResponse(message).to_string()),
    };
    client.return_to_pool();
    result
}

pub fn daemon_read_ec_register_range(
    chip_path: &str,
    start_register: u8,
    count: u8,
) -> Result<Vec<DaemonEcRegisterValue>, String> {
    let mut client = DaemonClient::get_pooled()?;
    let result = match client.request(DaemonRequest::ReadEcRegisterRange {
        chip_path: chip_path.to_string(),
        start_register,
        count,
    })? {
        DaemonResponse::Ok(data) if data.ec_registers.is_some() => Ok(data.ec_registers.unwrap()),
        DaemonResponse::Ok(_) => Err(crate::error::HyperfanError::IpcProtocol("Unexpected response type".to_string()).to_string()),
        DaemonResponse::Error { message } => Err(crate::error::HyperfanError::DaemonResponse(message).to_string()),
    };
    client.return_to_pool();
    result
}

// ============================================================================
// Rate Limit Configuration
// ============================================================================

/// Get the current client-side rate limit
pub fn get_client_rate_limit() -> u32 {
    CLIENT_RATE_LIMIT.load(Ordering::Relaxed)
}

/// Set the client-side rate limit (clamped to valid range)
/// Takes effect immediately for all subsequent requests
pub fn set_client_rate_limit(limit: u32) -> u32 {
    let clamped = limit.clamp(MIN_RATE_LIMIT, MAX_RATE_LIMIT);
    CLIENT_RATE_LIMIT.store(clamped, Ordering::Relaxed);
    clamped
}

/// Get the current daemon-side rate limit
pub fn daemon_get_rate_limit() -> Result<u32, String> {
    let mut client = DaemonClient::get_pooled()?;
    let result = match client.request(DaemonRequest::GetRateLimit)? {
        DaemonResponse::Ok(data) if data.rate_limit.is_some() => Ok(data.rate_limit.unwrap()),
        DaemonResponse::Ok(_) => Err(crate::error::HyperfanError::IpcProtocol("Unexpected response type".to_string()).to_string()),
        DaemonResponse::Error { message } => Err(crate::error::HyperfanError::DaemonResponse(message).to_string()),
    };
    client.return_to_pool();
    result
}

/// Set the daemon-side rate limit (clamped to valid range by daemon)
/// Returns the actual limit that was set
pub fn daemon_set_rate_limit(limit: u32) -> Result<u32, String> {
    let mut client = DaemonClient::get_pooled()?;
    let result = match client.request(DaemonRequest::SetRateLimit { limit })? {
        DaemonResponse::Ok(data) if data.rate_limit.is_some() => Ok(data.rate_limit.unwrap()),
        DaemonResponse::Ok(_) => Err(crate::error::HyperfanError::IpcProtocol("Unexpected response type".to_string()).to_string()),
        DaemonResponse::Error { message } => Err(crate::error::HyperfanError::DaemonResponse(message).to_string()),
    };
    client.return_to_pool();
    result
}

/// Set both client and daemon rate limits simultaneously
/// Returns (client_limit, daemon_limit) - the actual limits that were set
pub fn set_rate_limits(limit: u32) -> Result<(u32, u32), String> {
    let client_limit = set_client_rate_limit(limit);
    let daemon_limit = daemon_set_rate_limit(limit)?;
    Ok((client_limit, daemon_limit))
}
