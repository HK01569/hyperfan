//! Unix Socket Server (Hardened)
//!
//! A **security-hardened** async server for handling fan control requests.
//!
//! # Security Features
//! - **Socket permissions**: Restrictive mode with symlink attack prevention
//! - **Peer credentials**: Full audit logging of UID/GID/PID for every request
//! - **Path validation**: Strict allowlist prevents traversal and injection
//! - **Connection limits**: Maximum concurrent connections enforced
//! - **Rate limiting**: Per-client request rate limiting
//! - **Timeouts**: Read/write timeouts prevent resource exhaustion
//! - **Message limits**: Maximum message size prevents memory exhaustion
//! - **Input validation**: All parameters sanitized before processing
//!
//! # Performance
//! - Single-threaded async I/O with Tokio (minimal attack surface)
//! - Zero-copy parsing where possible
//! - Bounded buffers prevent memory exhaustion

use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::{UnixListener, UnixStream};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;
use tokio::time::timeout;
use tracing::{info, warn, error, debug, trace};

use hf_protocol::{
    Request, Response, ResponseData, HardwareInfo, HwmonChip, TempSensor,
    FanSensor, PwmControl, GpuInfo, ManualPwmFanPairing, validate_hwmon_path,
    validate_pwm_target_path, AllHardwareData,
    EcChipInfo, EcRegisterValue,
};

// ============================================================================
// Security Constants
// ============================================================================

/// Maximum concurrent client connections
const MAX_CONNECTIONS: usize = 64;

/// Maximum message size in bytes
const MAX_MESSAGE_SIZE: usize = hf_protocol::MAX_MESSAGE_SIZE;

/// Read timeout per message
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Write timeout per message
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);

/// Rate limit: default maximum requests per window (matches client default)
const DEFAULT_RATE_LIMIT_REQUESTS: u32 = 1500;

/// Minimum rate limit (cannot go below this)
pub const MIN_RATE_LIMIT: u32 = 1500;

/// Maximum rate limit (cannot exceed this)
pub const MAX_RATE_LIMIT: u32 = 9999;

/// Rate limit window duration
const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(10);

/// Socket permissions (0666 = world read/write)
/// Client validation via executable path check provides security
const SOCKET_MODE: u32 = 0o666;

/// Default TTL for SetPwm override to prevent control loop fighting (3 seconds)
const DEFAULT_PWM_OVERRIDE_TTL_MS: u32 = 3000;

/// How often to refresh the hwmon chip structure cache (seconds)
/// The chip structure (paths, names) rarely changes - only values need refreshing
const CHIP_CACHE_TTL_SECS: u64 = 30;

// ============================================================================
// Connection Tracking
// ============================================================================

/// Global connection counter
static ACTIVE_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);

// ============================================================================
// Hwmon Chip Cache (PERF: avoid re-enumerating filesystem on every request)
// ============================================================================

use std::sync::OnceLock;
use std::sync::RwLock as StdRwLock;

/// Cached hwmon chip structure with timestamp
struct ChipCache {
    chips: Vec<hf_core::HwmonChip>,
    cached_at: Instant,
}

static CHIP_CACHE: OnceLock<StdRwLock<Option<ChipCache>>> = OnceLock::new();

fn get_chip_cache() -> &'static StdRwLock<Option<ChipCache>> {
    CHIP_CACHE.get_or_init(|| StdRwLock::new(None))
}

/// Get hwmon chips with caching - avoids filesystem enumeration on every poll
fn get_cached_chips() -> Result<Vec<hf_core::HwmonChip>, String> {
    let cache = get_chip_cache();
    
    // Fast path: check if cache is valid
    {
        let guard = cache.read().map_err(|_| "Cache lock poisoned")?;
        if let Some(ref cached) = *guard {
            if cached.cached_at.elapsed().as_secs() < CHIP_CACHE_TTL_SECS {
                return Ok(cached.chips.clone());
            }
        }
    }
    
    // Slow path: refresh cache
    let chips = hf_core::enumerate_hwmon_chips()
        .map_err(|e| format!("Failed to enumerate hardware: {}", e))?;
    
    {
        let mut guard = cache.write().map_err(|_| "Cache lock poisoned")?;
        *guard = Some(ChipCache {
            chips: chips.clone(),
            cached_at: Instant::now(),
        });
    }
    
    Ok(chips)
}

async fn read_line_bounded<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut R,
    out: &mut Vec<u8>,
    max_len: usize,
) -> std::io::Result<usize> {
    out.clear();

    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(0);
        }

        let mut take_len = available.len();
        let mut found_newline = false;
        if let Some(pos) = available.iter().position(|b| *b == b'\n') {
            take_len = pos + 1;
            found_newline = true;
        }

        let remaining = max_len.saturating_sub(out.len());
        if take_len > remaining {
            // Consume enough to make forward progress, but don't buffer beyond max_len.
            let consume_len = remaining.min(available.len());
            reader.consume(consume_len);
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Message too large",
            ));
        }

        out.extend_from_slice(&available[..take_len]);
        reader.consume(take_len);

        if found_newline {
            return Ok(out.len());
        }
    }
}

/// Rate limiter state per client (keyed by UID)
struct RateLimiter {
    clients: HashMap<u32, ClientState>,
    /// Current rate limit (configurable at runtime)
    max_requests: u32,
}

fn pwm_enable_path_from_pwm_path(pwm_path: &str) -> Result<String, String> {
    let path = std::path::Path::new(pwm_path);
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "Invalid PWM path".to_string())?;

    if !file_name.starts_with("pwm") {
        return Err("PWM path does not point to a pwmN control file".to_string());
    }

    let suffix = &file_name[3..];
    if suffix.is_empty() || !suffix.chars().all(|c| c.is_ascii_digit()) {
        return Err("PWM path does not point to a pwmN control file".to_string());
    }

    Ok(path
        .with_file_name(format!("{}_enable", file_name))
        .to_string_lossy()
        .to_string())
}

fn apply_pwm_value_immediately(path: &str, value: u8) -> Response {
    if let Err(e) = validate_pwm_target_path(path) {
        return Response::error(e);
    }

    if path.starts_with("nvidia:") {
        let parts: Vec<&str> = path.split(':').collect();
        if parts.len() != 3 {
            return Response::error("Invalid NVIDIA PWM path format");
        }
        let Ok(gpu_idx) = parts[1].parse::<u32>() else {
            return Response::error("Invalid NVIDIA GPU index");
        };
        let Ok(fan_idx) = parts[2].parse::<u32>() else {
            return Response::error("Invalid NVIDIA fan index");
        };
        let percent = ((value as f32 / 255.0) * 100.0).round() as u32;
        return match hf_core::set_nvidia_fan_speed(gpu_idx, fan_idx, percent) {
            Ok(()) => Response::ok(),
            Err(e) => Response::error(format!("NVIDIA fan control failed: {}", e)),
        };
    }

    if path.starts_with("amd:") || path.starts_with("intel:") {
        let percent = ((value as f32 / 255.0) * 100.0).round() as u32;
        return match hf_core::set_gpu_fan_speed_by_id(path, percent) {
            Ok(()) => Response::ok(),
            Err(e) => Response::error(format!("GPU fan control failed: {}", e)),
        };
    }

    let enable_path = match pwm_enable_path_from_pwm_path(path) {
        Ok(p) => p,
        Err(e) => return Response::error(e),
    };

    if std::path::Path::new(&enable_path).exists() {
        if let Err(e) = validate_hwmon_path(&enable_path) {
            return Response::error(e);
        }
        if let Err(e) = std::fs::write(&enable_path, "1") {
            return Response::error(format!("Failed to enable manual PWM: {}", e));
        }
    }

    match std::fs::write(path, value.to_string()) {
        Ok(_) => Response::ok(),
        Err(e) => Response::error(format!("Failed to set PWM: {}", e)),
    }
}

struct ClientState {
    request_count: u32,
    window_start: Instant,
}

impl RateLimiter {
    fn new() -> Self {
        Self {
            clients: HashMap::new(),
            max_requests: DEFAULT_RATE_LIMIT_REQUESTS,
        }
    }
    
    /// Check if a client is rate limited. Returns true if allowed, false if limited.
    fn check_and_increment(&mut self, uid: u32) -> bool {
        let now = Instant::now();
        
        let state = self.clients.entry(uid).or_insert(ClientState {
            request_count: 0,
            window_start: now,
        });
        
        // Reset window if expired
        if now.duration_since(state.window_start) > RATE_LIMIT_WINDOW {
            state.request_count = 0;
            state.window_start = now;
        }
        
        if state.request_count >= self.max_requests {
            return false;
        }
        
        state.request_count += 1;
        true
    }
    
    /// Set the rate limit (clamped to valid range)
    fn set_rate_limit(&mut self, limit: u32) -> u32 {
        self.max_requests = limit.clamp(MIN_RATE_LIMIT, MAX_RATE_LIMIT);
        self.max_requests
    }
    
    /// Get current rate limit
    fn get_rate_limit(&self) -> u32 {
        self.max_requests
    }
    
    /// Cleanup old entries to prevent memory growth
    fn cleanup(&mut self) {
        let now = Instant::now();
        self.clients.retain(|_, state| {
            now.duration_since(state.window_start) < RATE_LIMIT_WINDOW * 2
        });
    }
}

// ============================================================================
// Server
// ============================================================================

/// Run the Unix socket server with full security hardening
pub async fn run_server(socket_path: &str, fan_control_state: Arc<crate::fan_control::FanControlState>) -> Result<(), Box<dyn std::error::Error>> {
    let path = Path::new(socket_path);
    
    // SECURITY: Remove existing socket only if it's actually a socket
    if path.exists() {
        let metadata = path.symlink_metadata()?;
        
        // Refuse to remove symlinks (prevent symlink attacks)
        if metadata.file_type().is_symlink() {
            return Err("Socket path is a symlink - refusing for security".into());
        }
        
        std::fs::remove_file(path)?;
        debug!("Removed existing socket file");
    }
    
    // Create listener
    let listener = UnixListener::bind(socket_path)?;
    
    // Set socket permissions
    std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(SOCKET_MODE))?;
    
    info!("Listening on {} (mode {:o})", socket_path, SOCKET_MODE);
    info!("Security: max_conn={}, max_msg={}, rate_limit={}/{:?}", 
          MAX_CONNECTIONS, MAX_MESSAGE_SIZE, DEFAULT_RATE_LIMIT_REQUESTS, RATE_LIMIT_WINDOW);
    
    // Shared rate limiter
    let rate_limiter = Arc::new(Mutex::new(RateLimiter::new()));
    
    // Periodic cleanup task for rate limiter
    let rate_limiter_cleanup = rate_limiter.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(RATE_LIMIT_WINDOW).await;
            rate_limiter_cleanup.lock().await.cleanup();
        }
    });
    
    // Handle shutdown signal
    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);
    
    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, _addr)) => {
                        // Check connection limit
                        let current = ACTIVE_CONNECTIONS.load(Ordering::SeqCst);
                        if current >= MAX_CONNECTIONS {
                            warn!("Connection limit reached ({}), rejecting new connection", current);
                            drop(stream);
                            continue;
                        }
                        
                        ACTIVE_CONNECTIONS.fetch_add(1, Ordering::SeqCst);
                        let rate_limiter = rate_limiter.clone();
                        let fan_state = fan_control_state.clone();
                        
                        tokio::spawn(async move {
                            handle_client(stream, rate_limiter, fan_state).await;
                            ACTIVE_CONNECTIONS.fetch_sub(1, Ordering::SeqCst);
                        });
                    }
                    Err(e) => {
                        error!("Accept error: {}", e);
                    }
                }
            }
            _ = &mut shutdown => {
                info!("Shutdown signal received");
                break;
            }
        }
    }
    
    // Cleanup socket
    let _ = std::fs::remove_file(socket_path);
    info!("Server stopped (handled {} total connections)", 
          ACTIVE_CONNECTIONS.load(Ordering::SeqCst));
    
    Ok(())
}

/// Client credentials from Unix socket peer
#[derive(Debug, Clone, Copy)]
struct PeerCredentials {
    uid: u32,
    gid: u32,
    pid: i32,
}

/// Handle a single client connection with full security enforcement
async fn handle_client(
    stream: UnixStream, 
    rate_limiter: Arc<Mutex<RateLimiter>>,
    fan_control_state: Arc<crate::fan_control::FanControlState>,
) {
    // Get peer credentials for audit logging
    let cred = match get_peer_credentials(&stream) {
        Some(c) => c,
        None => {
            error!("Failed to get peer credentials, rejecting connection");
            return;
        }
    };
    
    // Validate that the client is a legitimate Hyperfan binary
    if let Err(e) = validate_client(&cred) {
        error!("Client validation failed: {}", e);
        // Send error response and close connection
        let mut writer = stream;
        let error_response = hf_protocol::ResponseEnvelope::new(
            0,
            hf_protocol::Response::error("Unauthorized: Only Hyperfan GUI/CLI clients are allowed")
        );
        let _ = send_response_sync(&mut writer, &error_response).await;
        return;
    }
    
    info!(
        "Validated connection from uid={}, gid={}, pid={}",
        cred.uid, cred.gid, cred.pid
    );
    
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line: Vec<u8> = Vec::with_capacity(MAX_MESSAGE_SIZE);
    let mut request_count: u64 = 0;
    let connection_start = Instant::now();
    
    loop {
        // Apply read timeout
        let read_result = timeout(READ_TIMEOUT, read_line_bounded(&mut reader, &mut line, MAX_MESSAGE_SIZE)).await;
        
        match read_result {
            Ok(Ok(0)) => {
                // EOF - client disconnected gracefully
                debug!("Client disconnected: uid={}, pid={}, requests={}, duration={:?}", 
                       cred.uid, cred.pid, request_count, connection_start.elapsed());
                break;
            }
            Ok(Ok(n)) => {
                // read_line_bounded enforces MAX_MESSAGE_SIZE before buffering the full line.
                
                // Check rate limit
                {
                    let mut limiter = rate_limiter.lock().await;
                    if !limiter.check_and_increment(cred.uid) {
                        warn!("Rate limit exceeded for uid={}, pid={}", cred.uid, cred.pid);
                        let response_envelope = hf_protocol::ResponseEnvelope::new(
                            0,
                            Response::error("Rate limit exceeded")
                        );
                        let _ = send_response(&mut writer, &response_envelope).await;
                        // Don't break - just reject this request
                        continue;
                    }
                }
                
                request_count += 1;
                trace!("Request #{} from uid={}: {} bytes", request_count, cred.uid, n);

                let line_str = match std::str::from_utf8(&line) {
                    Ok(s) => s,
                    Err(e) => {
                        debug!("Non-UTF8 request from uid={}: {}", cred.uid, e);
                        let response_envelope = hf_protocol::ResponseEnvelope::new(
                            0,
                            Response::error("Invalid request encoding")
                        );
                        let _ = send_response(&mut writer, &response_envelope).await;
                        break;
                    }
                };

                // Process request with audit logging
                let response_envelope = process_request(line_str, &cred, &fan_control_state, &rate_limiter).await;
                
                // Send response with timeout
                if send_response(&mut writer, &response_envelope).await.is_err() {
                    break;
                }
            }
            Ok(Err(e)) => {
                if e.kind() == std::io::ErrorKind::InvalidData
                    && e.to_string().contains("Message too large")
                {
                    warn!(
                        "Message too large (>{} bytes) from uid={}, pid={}",
                        MAX_MESSAGE_SIZE,
                        cred.uid,
                        cred.pid
                    );
                    let response_envelope = hf_protocol::ResponseEnvelope::new(
                        0,
                        Response::error("Message too large")
                    );
                    let _ = send_response(&mut writer, &response_envelope).await;
                } else {
                    error!("Read error from uid={}, pid={}: {}", cred.uid, cred.pid, e);
                }
                break;
            }
            Err(_) => {
                debug!("Read timeout for uid={}, pid={}", cred.uid, cred.pid);
                let response_envelope = hf_protocol::ResponseEnvelope::new(
                    0,
                    Response::error("Read timeout")
                );
                let _ = send_response(&mut writer, &response_envelope).await;
                break;
            }
        }
    }
}

/// Send response with timeout
async fn send_response(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    response_envelope: &hf_protocol::ResponseEnvelope,
) -> Result<(), ()> {
    let response_json = serde_json::to_string(response_envelope).unwrap_or_else(|_| {
        r#"{"id":0,"status":"Error","data":{"message":"Serialization error"}}"#.to_string()
    });
    
    let write_result = timeout(WRITE_TIMEOUT, async {
        writer.write_all(response_json.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        Ok::<_, std::io::Error>(())
    }).await;
    
    match write_result {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => {
            error!("Write error: {}", e);
            Err(())
        }
        Err(_) => {
            error!("Write timeout");
            Err(())
        }
    }
}

/// Send response synchronously (for early rejection before async setup)
async fn send_response_sync(
    writer: &mut UnixStream,
    response_envelope: &hf_protocol::ResponseEnvelope,
) -> Result<(), ()> {
    let response_json = serde_json::to_string(response_envelope).unwrap_or_else(|_| {
        r#"{"id":0,"status":"Error","data":{"message":"Serialization error"}}"#.to_string()
    });
    
    writer.write_all(response_json.as_bytes()).await.map_err(|_| ())?;
    writer.write_all(b"\n").await.map_err(|_| ())?;
    writer.flush().await.map_err(|_| ())?;
    
    Ok(())
}

/// Validate that the connecting client is a legitimate Hyperfan binary
fn validate_client(cred: &PeerCredentials) -> Result<(), String> {
    // On Linux, we can check the executable path via /proc
    #[cfg(target_os = "linux")]
    {
        let exe_path = format!("/proc/{}/exe", cred.pid);
        if let Ok(exe) = std::fs::read_link(&exe_path) {
            let exe_str = exe.to_string_lossy();
            
            // Allow hyperfan GUI binary
            if exe_str.contains("/hyperfan") && !exe_str.contains("hyperfand") {
                debug!("Validated client: {} (pid={})", exe_str, cred.pid);
                return Ok(());
            }
            
            // Allow if running from development build directory
            if exe_str.contains("/target/") {
                debug!("Validated dev client: {} (pid={})", exe_str, cred.pid);
                return Ok(());
            }
            
            warn!("Rejected unauthorized client: {} (pid={}, uid={})", exe_str, cred.pid, cred.uid);
            return Err(format!("Unauthorized client: {}", exe_str));
        } else {
            // If we can't read the executable path, allow the connection
            // This can happen if the process exits quickly or due to permission issues
            debug!("Could not read executable path for pid={}, allowing connection", cred.pid);
            return Ok(());
        }
    }
    
    // On BSD, we can't easily check executable path, so rely on socket permissions
    #[cfg(not(target_os = "linux"))]
    {
        debug!("Client validation skipped on BSD (relying on socket permissions)");
        Ok(())
    }
}

/// Get peer credentials (uid, gid, pid) from Unix socket
fn get_peer_credentials(stream: &UnixStream) -> Option<PeerCredentials> {
    use std::os::unix::io::AsRawFd;
    
    let fd = stream.as_raw_fd();
    
    // Linux uses SO_PEERCRED with ucred struct
    #[cfg(target_os = "linux")]
    {
        // SAFETY: ucred is a simple C struct with no pointers. Zeroing it is safe and creates a valid initial state.
        let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        
        // SAFETY: getsockopt is safe when:
        // 1. fd is a valid socket file descriptor (guaranteed by caller)
        // 2. cred is properly initialized (zeroed above)
        // 3. len is set to the correct size of ucred struct
        let result = unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                &mut cred as *mut _ as *mut libc::c_void,
                &mut len,
            )
        };
        
        if result == 0 {
            return Some(PeerCredentials {
                uid: cred.uid,
                gid: cred.gid,
                pid: cred.pid,
            });
        }
    }
    
    // BSD uses getpeereid (simpler, no PID)
    #[cfg(any(target_os = "freebsd", target_os = "openbsd", target_os = "netbsd", target_os = "dragonfly", target_os = "macos"))]
    {
        let mut uid: libc::uid_t = 0;
        let mut gid: libc::gid_t = 0;
        
        // SAFETY: getpeereid is safe when:
        // 1. fd is a valid socket file descriptor (guaranteed by caller)
        // 2. uid and gid are valid mutable references to initialized variables
        let result = unsafe { libc::getpeereid(fd, &mut uid, &mut gid) };
        
        if result == 0 {
            return Some(PeerCredentials {
                uid,
                gid,
                pid: 0, // BSD doesn't provide PID via getpeereid
            });
        }
    }
    
    None
}

/// Process a single request and return response with audit logging
async fn process_request(
    line: &str, 
    cred: &PeerCredentials,
    fan_control_state: &Arc<crate::fan_control::FanControlState>,
    rate_limiter: &Arc<Mutex<RateLimiter>>,
) -> hf_protocol::ResponseEnvelope {
    // Parse request envelope with strict validation
    let envelope: hf_protocol::RequestEnvelope = match serde_json::from_str(line.trim()) {
        Ok(e) => e,
        Err(e) => {
            debug!("Invalid JSON from uid={}: {}", cred.uid, e);
            return hf_protocol::ResponseEnvelope::new(
                0,
                Response::error("Invalid request format")
            );
        }
    };
    
    let request_id = envelope.id;
    let request = envelope.request;
    
    // Double validation: validate request parameters server-side
    if let Err(e) = request.validate() {
        warn!("Request validation failed from uid={}: {}", cred.uid, e);
        // Sanitize error message to prevent path leakage
        return hf_protocol::ResponseEnvelope::new(
            request_id,
            Response::error(sanitize_validation_error(&e))
        );
    };
    
    // Log the request type for audit trail
    let request_type = request.type_name();
    debug!("Processing {} (id={}) from uid={}, pid={}", request_type, request_id, cred.uid, cred.pid);
    
    let response = match request {
        Request::Ping => Response::ok_string("pong"),
        
        Request::Version => Response::ok_string(env!("CARGO_PKG_VERSION")),
        
        Request::ListHardware => list_hardware(),
        
        // Batched request: hardware + GPUs in single response (performance optimization)
        Request::ListAll => list_all(),
        
        Request::ReadTemperature { path } => read_temperature(&path),
        
        Request::ReadFanRpm { path } => read_fan_rpm(&path),
        
        Request::ReadPwm { path } => read_pwm(&path),
        
        // Write operations get extra logging
        Request::SetPwm { path, value } => {
            info!("AUDIT: SetPwm path={} value={} by uid={}, pid={}", 
                  path, value, cred.uid, cred.pid);
            let resp = set_pwm(&path, value);
            if matches!(resp, Response::Ok(_)) {
                // Prevent the control loop from immediately fighting a manual set.
                // Keep it short so curves re-take control automatically.
                fan_control_state.set_pwm_override(path, value, DEFAULT_PWM_OVERRIDE_TTL_MS).await;
            }
            resp
        }
        
        Request::EnableManualPwm { path } => {
            info!("AUDIT: EnableManualPwm path={} by uid={}, pid={}", 
                  path, cred.uid, cred.pid);
            enable_manual_pwm(&path)
        }
        
        Request::DisableManualPwm { path } => {
            info!("AUDIT: DisableManualPwm path={} by uid={}, pid={}", 
                  path, cred.uid, cred.pid);
            disable_manual_pwm(&path)
        }

        Request::SetPwmOverride { path, value, ttl_ms } => {
            info!(
                "AUDIT: SetPwmOverride path={} value={} ttl_ms={} by uid={}, pid={}",
                path,
                value,
                ttl_ms,
                cred.uid,
                cred.pid
            );
            set_pwm_override(&path, value, ttl_ms, fan_control_state).await;
            Response::ok()
        }

        Request::ClearPwmOverride { path } => {
            info!("AUDIT: ClearPwmOverride path={} by uid={}, pid={}", path, cred.uid, cred.pid);
            clear_pwm_override(&path, fan_control_state).await;
            Response::ok()
        }
        
        Request::ListGpus => list_gpus(),
        
        Request::SetGpuFan { index, fan_index, percent } => {
            info!(
                "AUDIT: SetGpuFan index={} fan_index={:?} percent={} by uid={}, pid={}",
                index,
                fan_index,
                percent,
                cred.uid,
                cred.pid
            );
            set_gpu_fan(index, fan_index, percent)
        }

        Request::ResetGpuFanAuto { index } => {
            info!(
                "AUDIT: ResetGpuFanAuto index={} by uid={}, pid={}",
                index,
                cred.uid,
                cred.pid
            );
            reset_gpu_fan_auto(index)
        }
        
        Request::DetectFanMappings => {
            warn!("AUDIT: DetectFanMappings by uid={}, pid={}", cred.uid, cred.pid);
            detect_fan_mappings().await
        }
        
        Request::GetManualPairings => {
            debug!("GetManualPairings by uid={}, pid={}", cred.uid, cred.pid);
            get_manual_pairings()
        }
        
        Request::SetManualPairing { pwm_uuid, pwm_path, fan_uuid, fan_path } => {
            info!("AUDIT: SetManualPairing pwm_uuid={} pwm={} fan_uuid={:?} fan={:?} by uid={}, pid={}", 
                  pwm_uuid, pwm_path, fan_uuid, fan_path, cred.uid, cred.pid);
            let resp = set_manual_pairing(&pwm_uuid, &pwm_path, fan_uuid.as_deref(), fan_path.as_deref());
            if matches!(resp, Response::Ok(_)) {
                fan_control_state.signal_reload();
            }
            resp
        }
        
        Request::DeleteManualPairing { pwm_path } => {
            info!("AUDIT: DeleteManualPairing pwm={} by uid={}, pid={}", 
                  pwm_path, cred.uid, cred.pid);
            let resp = delete_manual_pairing(&pwm_path);
            if matches!(resp, Response::Ok(_)) {
                fan_control_state.signal_reload();
            }
            resp
        }
        
        Request::ReloadConfig => {
            info!("AUDIT: ReloadConfig by uid={}, pid={}", cred.uid, cred.pid);
            // Signal the fan control loop to reload its configuration
            fan_control_state.signal_reload();
            Response::ok_string("Configuration reload signaled")
        }
        
        // ====================================================================
        // EC Direct Control (DANGEROUS)
        // ====================================================================
        
        Request::ListEcChips => {
            debug!("ListEcChips by uid={}, pid={}", cred.uid, cred.pid);
            list_ec_chips()
        }
        
        Request::ReadEcRegister { chip_path, register } => {
            debug!("ReadEcRegister chip={} reg=0x{:02X} by uid={}, pid={}", 
                   chip_path, register, cred.uid, cred.pid);
            read_ec_register(&chip_path, register)
        }
        
        Request::WriteEcRegister { chip_path, register, value } => {
            // CRITICAL: This is extremely dangerous - full audit logging
            warn!("DANGER AUDIT: WriteEcRegister chip={} reg=0x{:02X} val=0x{:02X} by uid={}, pid={}", 
                  chip_path, register, value, cred.uid, cred.pid);
            write_ec_register(&chip_path, register, value)
        }
        
        Request::ReadEcRegisterRange { chip_path, start_register, count } => {
            debug!("ReadEcRegisterRange chip={} start=0x{:02X} count={} by uid={}, pid={}", 
                   chip_path, start_register, count, cred.uid, cred.pid);
            read_ec_register_range(&chip_path, start_register, count)
        }
        
        Request::SetGlobalMode { mode: _ } => {
            // Not implemented - manual mode is handled via PWM overrides
            Response::error("SetGlobalMode not implemented")
        }
        
        Request::GetGlobalMode => {
            // Not implemented - manual mode is handled via PWM overrides
            Response::error("GetGlobalMode not implemented")
        }
        
        Request::GetRateLimit => {
            let limiter = rate_limiter.lock().await;
            let limit = limiter.get_rate_limit();
            Response::Ok(ResponseData::rate_limit(limit))
        }
        
        Request::SetRateLimit { limit } => {
            let mut limiter = rate_limiter.lock().await;
            let actual_limit = limiter.set_rate_limit(limit);
            info!("Rate limit changed to {} by uid={}", actual_limit, cred.uid);
            Response::Ok(ResponseData::rate_limit(actual_limit))
        }
    };
    
    // Log errors for audit
    if let Response::Error { ref message } = response {
        warn!("Request {} (id={}) failed for uid={}: {}", request_type, request_id, cred.uid, message);
    }
    
    hf_protocol::ResponseEnvelope::new(request_id, response)
}

/// Generate a stable UUID for a sensor based on chip name, sensor name, and type
/// This UUID is deterministic and will be the same across reboots for the same hardware
fn generate_sensor_uuid(chip_name: &str, sensor_name: &str, sensor_type: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    
    let mut hasher = DefaultHasher::new();
    chip_name.hash(&mut hasher);
    sensor_name.hash(&mut hasher);
    sensor_type.hash(&mut hasher);
    let hash = hasher.finish();
    
    // Format as UUID-like string (not a real UUID, but stable and unique)
    format!("{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        (hash >> 32) as u32,
        ((hash >> 16) & 0xFFFF) as u16,
        (hash & 0xFFFF) as u16,
        ((hash >> 48) & 0xFFFF) as u16,
        hash & 0xFFFFFFFFFFFF
    )
}

/// Sanitize validation error messages to prevent information leakage
fn sanitize_validation_error(error: &str) -> String {
    // Remove specific paths from error messages
    if error.contains("/sys/") || error.contains("/dev/") {
        return "Invalid path parameter".to_string();
    }
    
    // Generic messages for common validation failures
    if error.contains("too long") {
        return "Parameter exceeds maximum length".to_string();
    }
    if error.contains("forbidden") {
        return "Invalid parameter format".to_string();
    }
    if error.contains("out of range") {
        return "Parameter out of valid range".to_string();
    }
    
    // Default safe message
    "Invalid request parameter".to_string()
}

/// Convert hwmon chips to protocol format (shared by list_hardware and list_all)
fn chips_to_protocol(chips: &[hf_core::HwmonChip]) -> Vec<HwmonChip> {
    chips.iter().map(|c| {
        HwmonChip {
            name: c.name.clone(),
            path: c.path.to_string_lossy().to_string(),
            temperatures: c.temperatures.iter().map(|t| {
                let value = hf_core::read_temperature(&t.input_path).unwrap_or(f32::NAN);
                TempSensor {
                    name: t.name.clone(),
                    label: t.label.clone(),
                    path: t.input_path.to_string_lossy().to_string(),
                    value,
                }
            }).collect(),
            fans: c.fans.iter().map(|f| {
                let rpm = hf_core::read_fan_rpm(&f.input_path).ok();
                let uuid = generate_sensor_uuid(&c.name, &f.name, "fan");
                FanSensor {
                    uuid,
                    name: f.name.clone(),
                    label: f.label.clone(),
                    path: f.input_path.to_string_lossy().to_string(),
                    rpm,
                }
            }).collect(),
            pwms: c.pwms.iter().map(|p| {
                let value = std::fs::read_to_string(&p.pwm_path)
                    .ok()
                    .and_then(|s| s.trim().parse().ok())
                    .unwrap_or(0);
                let enabled = std::fs::read_to_string(&p.enable_path)
                    .ok()
                    .and_then(|s| s.trim().parse::<u8>().ok())
                    .map(|v| v == 1)
                    .unwrap_or(false);
                let uuid = generate_sensor_uuid(&c.name, &p.name, "pwm");
                PwmControl {
                    uuid,
                    name: p.name.clone(),
                    path: p.pwm_path.to_string_lossy().to_string(),
                    value,
                    enabled,
                }
            }).collect(),
        }
    }).collect()
}

/// Convert GPUs to protocol format (shared by list_gpus and list_all)
fn gpus_to_protocol(gpus: &[hf_gpu::GpuDevice]) -> Vec<GpuInfo> {
    gpus.iter().map(|g| {
        GpuInfo {
            index: g.index,
            name: g.name.clone(),
            vendor: g.vendor.to_string(),
            temp: g.temperatures.first().and_then(|t| t.current_temp),
            fan_percent: g.fans.first().and_then(|f| f.speed_percent),
            fan_rpm: g.fans.first().and_then(|f| f.rpm),
        }
    }).collect()
}

fn list_hardware() -> Response {
    match get_cached_chips() {
        Ok(chips) => {
            Response::Ok(ResponseData::hw(HardwareInfo { chips: chips_to_protocol(&chips) }))
        }
        Err(e) => Response::error(e),
    }
}

/// Batched hardware + GPU enumeration (single IPC call for polling)
fn list_all() -> Response {
    let hardware = match get_cached_chips() {
        Ok(chips) => HardwareInfo { chips: chips_to_protocol(&chips) },
        Err(e) => return Response::error(e),
    };
    
    let gpus = match hf_core::enumerate_gpus() {
        Ok(gpus) => gpus_to_protocol(&gpus),
        Err(_) => Vec::new(), // GPUs are optional, don't fail the whole request
    };
    
    Response::Ok(ResponseData::all(AllHardwareData { hardware, gpus }))
}

fn read_temperature(path: &str) -> Response {
    if let Err(e) = validate_hwmon_path(path) {
        return Response::error(e);
    }
    
    match hf_core::read_temperature(std::path::Path::new(path)) {
        Ok(temp) => Response::ok_temp(temp),
        Err(e) => Response::error(format!("Failed to read temperature: {}", e)),
    }
}

fn read_fan_rpm(path: &str) -> Response {
    if let Err(e) = validate_hwmon_path(path) {
        return Response::error(e);
    }
    
    match hf_core::read_fan_rpm(std::path::Path::new(path)) {
        Ok(rpm) => Response::ok_rpm(rpm),
        Err(e) => Response::error(format!("Failed to read fan RPM: {}", e)),
    }
}

fn read_pwm(path: &str) -> Response {
    if let Err(e) = validate_hwmon_path(path) {
        return Response::error(e);
    }
    
    match std::fs::read_to_string(path) {
        Ok(content) => {
            match content.trim().parse::<u8>() {
                Ok(value) => Response::ok_pwm(value),
                Err(_) => Response::error("Invalid PWM value"),
            }
        }
        Err(e) => Response::error(format!("Failed to read PWM: {}", e)),
    }
}

fn set_pwm(path: &str, value: u8) -> Response {
    debug!("Setting PWM {} to {}", path, value);

    apply_pwm_value_immediately(path, value)
}

fn enable_manual_pwm(path: &str) -> Response {
    if let Err(e) = validate_pwm_target_path(path) {
        return Response::error(e);
    }

    if path.starts_with("nvidia:") || path.starts_with("amd:") || path.starts_with("intel:") {
        return Response::ok();
    }
    
    let enable_path = match pwm_enable_path_from_pwm_path(path) {
        Ok(p) => p,
        Err(e) => return Response::error(e),
    };

    if !std::path::Path::new(&enable_path).exists() {
        return Response::ok();
    }

    if let Err(e) = validate_hwmon_path(&enable_path) {
        return Response::error(e);
    }
    
    debug!("Enabling manual PWM control: {}", enable_path);
    
    match std::fs::write(&enable_path, "1") {
        Ok(_) => Response::ok(),
        Err(e) => Response::error(format!("Failed to enable manual PWM: {}", e)),
    }
}

fn disable_manual_pwm(path: &str) -> Response {
    if let Err(e) = validate_pwm_target_path(path) {
        return Response::error(e);
    }

    if path.starts_with("nvidia:") || path.starts_with("amd:") || path.starts_with("intel:") {
        return Response::ok();
    }
    
    let enable_path = match pwm_enable_path_from_pwm_path(path) {
        Ok(p) => p,
        Err(e) => return Response::error(e),
    };

    if !std::path::Path::new(&enable_path).exists() {
        return Response::ok();
    }

    if let Err(e) = validate_hwmon_path(&enable_path) {
        return Response::error(e);
    }
    
    debug!("Disabling manual PWM control: {}", enable_path);
    
    // Set to 2 for automatic control
    match std::fs::write(&enable_path, "2") {
        Ok(_) => Response::ok(),
        Err(e) => Response::error(format!("Failed to disable manual PWM: {}", e)),
    }
}

async fn set_pwm_override(
    path: &str,
    value: u8,
    ttl_ms: u32,
    fan_control_state: &Arc<crate::fan_control::FanControlState>,
) {
    // Conservative TTL bounds: long enough for live drag, short enough to be safe.
    let ttl_ms = ttl_ms.clamp(50, 30_000);

    // Install override in fan control state
    fan_control_state.set_pwm_override(path.to_string(), value, ttl_ms).await;
}

fn reset_gpu_fan_auto(index: u32) -> Response {
    let controllers = hf_core::enumerate_gpu_pwm_controllers();
    let mut matched = false;

    for controller in controllers.iter().filter(|c| c.gpu_index == index) {
        matched = true;

        match controller.vendor {
            hf_core::GpuVendor::Nvidia => {
                if let Err(e) = hf_core::reset_nvidia_fan_auto(index) {
                    return Response::error(format!("Failed to reset NVIDIA fan auto: {}", e));
                }
            }
            hf_core::GpuVendor::Amd => {
                // For AMD we currently reset using the resolved hwmon path to pwm1_enable.
                // The controller pwm_path is a file path (e.g. .../pwm1), so use its parent directory.
                let Some(hwmon_path) = std::path::Path::new(&controller.pwm_path).parent() else {
                    return Response::error("Invalid AMD PWM path");
                };
                if let Err(e) = hf_core::reset_amd_fan_auto(hwmon_path) {
                    return Response::error(format!("Failed to reset AMD fan auto: {}", e));
                }
            }
            hf_core::GpuVendor::Intel => {}
        }
    }

    if !matched {
        return Response::error("GPU not found or has no controllable fans");
    }

    Response::ok()
}

async fn clear_pwm_override(
    path: &str,
    fan_control_state: &Arc<crate::fan_control::FanControlState>,
) {
    fan_control_state.clear_pwm_override(path).await;
}

fn list_gpus() -> Response {
    match hf_core::enumerate_gpus() {
        Ok(gpus) => Response::Ok(ResponseData::gpu_list(gpus_to_protocol(&gpus))),
        Err(e) => Response::error(format!("Failed to enumerate GPUs: {}", e)),
    }
}

fn set_gpu_fan(index: u32, fan_index: Option<u32>, percent: u32) -> Response {
    if percent > 100 {
        return Response::error("Fan speed must be 0-100%");
    }

    let controllers = hf_core::enumerate_gpu_pwm_controllers();
    let mut matched = false;

    for controller in controllers.iter().filter(|c| c.gpu_index == index) {
        if let Some(fi) = fan_index {
            if controller.fan_index != fi {
                continue;
            }
        }
        matched = true;
        if let Err(e) = hf_core::set_gpu_fan_speed_by_id(&controller.id, percent) {
            return Response::error(format!("GPU fan control failed: {}", e));
        }
    }

    if !matched {
        return Response::error("GPU not found or has no controllable fans");
    }

    Response::ok()
}

async fn detect_fan_mappings() -> Response {
    // Use hf_core's detection logic
    // This is a blocking operation that may take several seconds
    warn!("Fan mapping detection requested - this may take a while");

    // Run blocking operation in spawn_blocking to avoid blocking executor
    let mappings = match tokio::task::spawn_blocking(|| {
        hf_core::autodetect_fan_pwm_mappings_heuristic()
    }).await {
        Ok(Ok(m)) => m,
        Ok(Err(e)) => return Response::error(format!("Detection failed: {}", e)),
        Err(e) => return Response::error(format!("Task panicked: {}", e)),
    };

    // Original match removed - now using spawn_blocking above
    /*let mappings = match hf_core::autodetect_fan_pwm_mappings_heuristic() {
        Ok(m) => m,
        Err(e) => return Response::error(format!("Detection failed: {}", e)),
    };*/

    if let Err(e) = hf_core::save_pwm_fan_mappings(mappings.clone()) {
        warn!("Failed to persist detected mappings to settings: {}", e);
    }

    // Build lookup maps from chip/pwm names to sysfs paths.
    let chips = match hf_core::enumerate_hwmon_chips() {
        Ok(c) => c,
        Err(e) => return Response::error(format!("Failed to enumerate hardware for mapping resolution: {}", e)),
    };

    // Build lookup maps: name -> (path, uuid)
    let mut pwm_name_to_info: std::collections::HashMap<String, (String, String)> = std::collections::HashMap::new();
    let mut fan_name_to_info: std::collections::HashMap<String, (String, String)> = std::collections::HashMap::new();
    
    for chip in &chips {
        for pwm in &chip.pwms {
            let key = format!("{}/{}", chip.name, pwm.name);
            let path = pwm.pwm_path.to_string_lossy().to_string();
            let uuid = generate_sensor_uuid(&chip.name, &pwm.name, "pwm");
            pwm_name_to_info.insert(key, (path, uuid));
        }
        for fan in &chip.fans {
            let key = format!("{}/{}", chip.name, fan.name);
            let path = fan.input_path.to_string_lossy().to_string();
            let uuid = generate_sensor_uuid(&chip.name, &fan.name, "fan");
            fan_name_to_info.insert(key, (path, uuid));
        }
    }

    let mut resolved: Vec<hf_protocol::FanMapping> = Vec::new();
    for m in mappings {
        let Some((pwm_path, pwm_uuid)) = pwm_name_to_info.get(&m.pwm_name).cloned() else {
            debug!("Could not resolve PWM '{}' to a sysfs path", m.pwm_name);
            continue;
        };
        let Some((fan_path, fan_uuid)) = fan_name_to_info.get(&m.fan_name).cloned() else {
            debug!("Could not resolve Fan '{}' to a sysfs path", m.fan_name);
            continue;
        };

        resolved.push(hf_protocol::FanMapping {
            pwm_uuid,
            pwm_path,
            fan_uuid,
            fan_path,
            confidence: m.confidence,
        });
    }

    Response::Ok(ResponseData::mappings(resolved))
}

// ============================================================================
// Manual PWM-Fan Pairing Persistence
// ============================================================================

fn get_manual_pairings() -> Response {
    let settings = match hf_core::load_settings() {
        Ok(s) => s,
        Err(e) => return Response::error(format!("Failed to load settings: {}", e)),
    };

    // Build map: pwm_path -> (fan_path, fan_name, fan_uuid)
    // We need to look up fan UUIDs from the saved pairings
    let mut pairings_by_pwm: std::collections::HashMap<String, (Option<String>, Option<String>, Option<String>)> =
        std::collections::HashMap::new();
    for p in &settings.pwm_fan_pairings {
        pairings_by_pwm.insert(p.pwm_path.clone(), (p.fan_path.clone(), p.fan_name.clone(), p.fan_uuid.clone()));
    }

    let chips = match hf_core::enumerate_hwmon_chips() {
        Ok(c) => c,
        Err(e) => return Response::error(format!("Failed to enumerate hardware: {}", e)),
    };

    // Build fan path -> uuid lookup for resolving fan UUIDs
    let mut fan_path_to_uuid: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for chip in &chips {
        for fan in &chip.fans {
            let path = fan.input_path.to_string_lossy().to_string();
            let uuid = generate_sensor_uuid(&chip.name, &fan.name, "fan");
            fan_path_to_uuid.insert(path, uuid);
        }
    }

    let mut pairings: Vec<ManualPwmFanPairing> = Vec::new();
    for chip in &chips {
        for pwm in &chip.pwms {
            let pwm_path = pwm.pwm_path.to_string_lossy().to_string();
            let pwm_name = format!("{} - {}", chip.name, pwm.name);
            let pwm_uuid = generate_sensor_uuid(&chip.name, &pwm.name, "pwm");
            
            let (fan_path, fan_name, saved_fan_uuid) = pairings_by_pwm
                .get(&pwm_path)
                .cloned()
                .unwrap_or((None, None, None));
            
            // Use saved fan_uuid if available, otherwise look up from current hardware
            let fan_uuid = saved_fan_uuid.or_else(|| {
                fan_path.as_ref().and_then(|fp| fan_path_to_uuid.get(fp).cloned())
            });
            
            pairings.push(ManualPwmFanPairing {
                pwm_uuid,
                pwm_path,
                pwm_name,
                fan_uuid,
                fan_path,
                fan_name,
            });
        }
    }

    for gpu in hf_core::enumerate_gpu_pwm_controllers() {
        let pwm_path = gpu.pwm_path.clone();
        let pwm_name = gpu.name.clone();
        // Generate UUID for GPU PWM controls
        let pwm_uuid = generate_sensor_uuid("gpu", &pwm_name, "pwm");
        
        let (fan_path, fan_name, saved_fan_uuid) = pairings_by_pwm
            .get(&pwm_path)
            .cloned()
            .unwrap_or((None, None, None));
        
        let fan_uuid = saved_fan_uuid.or_else(|| {
            fan_path.as_ref().and_then(|fp| fan_path_to_uuid.get(fp).cloned())
        });
        
        pairings.push(ManualPwmFanPairing {
            pwm_uuid,
            pwm_path,
            pwm_name,
            fan_uuid,
            fan_path,
            fan_name,
        });
    }

    Response::Ok(ResponseData::pairings(pairings))
}

fn set_manual_pairing(pwm_uuid: &str, pwm_path: &str, fan_uuid: Option<&str>, fan_path: Option<&str>) -> Response {
    // Validate PWM path
    if let Err(e) = validate_pwm_target_path(pwm_path) {
        return Response::error(format!("Invalid PWM path: {}", e));
    }
    
    // Validate fan path if provided
    if let Some(fp) = fan_path {
        if let Err(e) = validate_hwmon_path(fp) {
            return Response::error(format!("Invalid fan path: {}", e));
        }
    }
    
    let fan_name = fan_path.and_then(|fp| {
        let label_path = fp.replace("_input", "_label");
        std::fs::read_to_string(&label_path)
            .ok()
            .map(|s| s.trim().to_string())
            .or_else(|| {
                std::path::Path::new(fp)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
            })
    });

    // Create pairing with UUIDs for stable identification
    let mut pairing = hf_core::create_fingerprinted_pairing(pwm_path, fan_path, fan_name.as_deref(), None);
    pairing.pwm_uuid = Some(pwm_uuid.to_string());
    pairing.fan_uuid = fan_uuid.map(String::from);
    
    if let Err(e) = hf_core::update_setting(|s| {
        // Remove by UUID first (primary key), then by path (fallback)
        s.pwm_fan_pairings.retain(|p| {
            p.pwm_uuid.as_deref() != Some(pwm_uuid) && p.pwm_path != pwm_path
        });
        s.pwm_fan_pairings.push(pairing);
    }) {
        return Response::error(format!("Failed to save pairing: {}", e));
    }

    Response::ok()
}

fn delete_manual_pairing(pwm_path: &str) -> Response {
    let mut removed = false;
    let update = hf_core::update_setting(|s| {
        let before = s.pwm_fan_pairings.len();
        s.pwm_fan_pairings.retain(|p| p.pwm_path != pwm_path);
        removed = s.pwm_fan_pairings.len() != before;
    });

    if let Err(e) = update {
        return Response::error(format!("Failed to save: {}", e));
    }
    if !removed {
        return Response::error("No pairing found for this PWM");
    }

    Response::ok()
}

// ============================================================================
// EC Direct Control Functions (DANGEROUS)
// ============================================================================

/// EC chip classes that support direct register access
const EC_CHIP_CLASSES: &[&str] = &[
    "it87",      // ITE SuperIO
    "nct6775",   // Nuvoton SuperIO
    "nct6776",
    "nct6779",
    "nct6791",
    "nct6792",
    "nct6793",
    "nct6795",
    "nct6796",
    "nct6797",
    "nct6798",
    "w83627",    // Winbond SuperIO
    "w83667",
    "w83795",
    "f71882",    // Fintek SuperIO
    "f71889",
    "asus-ec",   // ASUS EC
    "dell-smm",  // Dell SMM
    "hp-wmi",    // HP WMI
    "thinkpad",  // ThinkPad EC
    "applesmc",  // Apple SMC
];

/// List all EC-capable chips
fn list_ec_chips() -> Response {
    match hf_core::enumerate_hwmon_chips() {
        Ok(chips) => {
            let ec_chips: Vec<EcChipInfo> = chips.iter()
                .filter(|c| {
                    // Check if chip name matches known EC/SuperIO drivers
                    let name_lower = c.name.to_lowercase();
                    EC_CHIP_CLASSES.iter().any(|ec| name_lower.contains(ec))
                })
                .map(|c| {
                    let device_path = c.path.join("device")
                        .canonicalize()
                        .ok()
                        .map(|p| p.to_string_lossy().to_string());
                    
                    let chip_class = classify_ec_chip(&c.name);
                    
                    EcChipInfo {
                        name: c.name.clone(),
                        path: c.path.to_string_lossy().to_string(),
                        device_path,
                        chip_class,
                        register_count: Some(256), // Most EC chips have 256 registers
                        supports_direct_access: true,
                    }
                })
                .collect();
            
            Response::Ok(ResponseData::chips(ec_chips))
        }
        Err(e) => Response::error(format!("Failed to enumerate EC chips: {}", e)),
    }
}

/// Classify EC chip type
fn classify_ec_chip(name: &str) -> String {
    let name_lower = name.to_lowercase();
    if name_lower.contains("it87") {
        "ITE SuperIO".to_string()
    } else if name_lower.contains("nct6") {
        "Nuvoton SuperIO".to_string()
    } else if name_lower.contains("w83") {
        "Winbond SuperIO".to_string()
    } else if name_lower.contains("f71") {
        "Fintek SuperIO".to_string()
    } else if name_lower.contains("asus") {
        "ASUS Embedded Controller".to_string()
    } else if name_lower.contains("dell") {
        "Dell SMM".to_string()
    } else if name_lower.contains("thinkpad") {
        "ThinkPad Embedded Controller".to_string()
    } else if name_lower.contains("applesmc") {
        "Apple SMC".to_string()
    } else {
        "Unknown EC/SuperIO".to_string()
    }
}

/// Read a single EC register
fn read_ec_register(chip_path: &str, register: u8) -> Response {
    // Validate path
    if let Err(e) = validate_hwmon_path(chip_path) {
        return Response::error(format!("Invalid chip path: {}", e));
    }
    
    // Construct the register file path
    // Most EC chips expose registers via /sys/class/hwmon/hwmonX/device/
    let chip_dir = std::path::Path::new(chip_path);
    
    // Try different register access methods
    let value = read_ec_register_value(chip_dir, register);
    
    match value {
        Ok(val) => {
            Response::Ok(ResponseData::register(EcRegisterValue {
                register,
                value: val,
                label: get_register_label(register),
                writable: is_register_writable(register),
            }))
        }
        Err(e) => Response::error(format!("Failed to read register 0x{:02X}: {}", register, e)),
    }
}

/// Write to an EC register (EXTREMELY DANGEROUS)
fn write_ec_register(chip_path: &str, register: u8, value: u8) -> Response {
    // Validate path
    if let Err(e) = validate_hwmon_path(chip_path) {
        return Response::error(format!("Invalid chip path: {}", e));
    }
    
    // Check if EC control is enabled in settings
    match hf_core::load_settings() {
        Ok(settings) => {
            if !settings.advanced.ec_direct_control_enabled {
                return Response::error("EC direct control is not enabled in settings");
            }
            if !settings.advanced.ec_danger_acknowledged {
                return Response::error("EC danger warning has not been acknowledged");
            }
        }
        Err(e) => {
            return Response::error(format!("Failed to load settings: {}", e));
        }
    }
    
    let chip_dir = std::path::Path::new(chip_path);
    
    match write_ec_register_value(chip_dir, register, value) {
        Ok(()) => {
            info!("EC register 0x{:02X} written with value 0x{:02X}", register, value);
            Response::ok()
        }
        Err(e) => Response::error(format!("Failed to write register 0x{:02X}: {}", register, e)),
    }
}

/// Read a range of EC registers
fn read_ec_register_range(chip_path: &str, start: u8, count: u8) -> Response {
    // Validate path
    if let Err(e) = validate_hwmon_path(chip_path) {
        return Response::error(format!("Invalid chip path: {}", e));
    }
    
    // Limit count to prevent abuse
    let count = count.min(64);
    
    let chip_dir = std::path::Path::new(chip_path);
    let mut registers = Vec::with_capacity(count as usize);
    
    for i in 0..count {
        let reg = start.wrapping_add(i);
        match read_ec_register_value(chip_dir, reg) {
            Ok(val) => {
                registers.push(EcRegisterValue {
                    register: reg,
                    value: val,
                    label: get_register_label(reg),
                    writable: is_register_writable(reg),
                });
            }
            Err(_) => {
                // Skip unreadable registers
                registers.push(EcRegisterValue {
                    register: reg,
                    value: 0xFF,
                    label: Some("(unreadable)".to_string()),
                    writable: false,
                });
            }
        }
    }
    
    Response::Ok(ResponseData::registers(registers))
}

/// Read EC register value using available methods
fn read_ec_register_value(chip_dir: &std::path::Path, register: u8) -> Result<u8, String> {
    // Method 1: Try device/ec_read interface (if available)
    let ec_read_path = chip_dir.join("device/ec_read");
    if ec_read_path.exists() {
        // Write register address, then read value
        if std::fs::write(&ec_read_path, format!("{}", register)).is_ok() {
            if let Ok(content) = std::fs::read_to_string(&ec_read_path) {
                if let Ok(val) = content.trim().parse::<u8>() {
                    return Ok(val);
                }
            }
        }
    }
    
    // Method 2: Try /dev/ec or /dev/port (requires root)
    // This is the most direct but also most dangerous method
    let ec_dev = std::path::Path::new("/dev/ec");
    if ec_dev.exists() {
        use std::io::{Read, Seek, SeekFrom};
        if let Ok(mut file) = std::fs::File::open(ec_dev) {
            if file.seek(SeekFrom::Start(register as u64)).is_ok() {
                let mut buf = [0u8; 1];
                if file.read_exact(&mut buf).is_ok() {
                    return Ok(buf[0]);
                }
            }
        }
    }
    
    Err("No supported EC access method available".to_string())
}

/// Write EC register value
fn write_ec_register_value(chip_dir: &std::path::Path, register: u8, value: u8) -> Result<(), String> {
    // Method 1: Try device/ec_write interface
    let ec_write_path = chip_dir.join("device/ec_write");
    if ec_write_path.exists() {
        let write_str = format!("{} {}", register, value);
        return std::fs::write(&ec_write_path, write_str)
            .map_err(|e| format!("Failed to write to ec_write: {}", e));
    }
    
    // Method 2: Try /dev/ec (requires root)
    let ec_dev = std::path::Path::new("/dev/ec");
    if ec_dev.exists() {
        use std::io::{Seek, SeekFrom, Write};
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .open(ec_dev)
            .map_err(|e| format!("Failed to open /dev/ec: {}", e))?;
        
        file.seek(SeekFrom::Start(register as u64))
            .map_err(|e| format!("Failed to seek: {}", e))?;
        
        file.write_all(&[value])
            .map_err(|e| format!("Failed to write: {}", e))?;
        
        return Ok(());
    }
    
    Err("No supported EC write method available".to_string())
}

/// Get human-readable label for common EC registers
fn get_register_label(register: u8) -> Option<String> {
    // Common EC register meanings (varies by chip)
    match register {
        0x00..=0x0F => Some(format!("Config Register 0x{:02X}", register)),
        0x10..=0x1F => Some(format!("Temperature Register 0x{:02X}", register)),
        0x20..=0x2F => Some(format!("Fan Speed Register 0x{:02X}", register)),
        0x30..=0x3F => Some(format!("Fan PWM Register 0x{:02X}", register)),
        0x40..=0x4F => Some(format!("Voltage Register 0x{:02X}", register)),
        0x50..=0x5F => Some(format!("GPIO Register 0x{:02X}", register)),
        _ => None,
    }
}

/// Check if a register is typically writable
fn is_register_writable(register: u8) -> bool {
    // PWM and config registers are typically writable
    matches!(register, 0x00..=0x0F | 0x30..=0x3F | 0x50..=0x5F)
}

