# Hyperfan

Professional fan control platform for Linux enthusiasts who demand precise thermal management.

Hyperfan provides comprehensive control over system cooling with a modern GTK4 interface, robust hardware detection, and safety-first architecture. Whether you're building a silent workstation, optimizing a gaming rig, or managing server thermals, Hyperfan delivers the precision you need.

---

## Key Features

### Hardware Detection and Fingerprinting
- Automatic discovery of all temperature sensors, fans, and PWM controllers
- Advanced hardware fingerprinting survives reboots and hwmon reindexing
- Confidence-based matching algorithm for stable fan pairing
- Support for any hwmon-compatible device

### Visual Fan Curve Editor
- Interactive drag-and-drop curve editor
- Real-time preview with live temperature indicator
- Piecewise linear interpolation
- Configurable smoothing to prevent oscillation
- Multiple curves per profile

### Comprehensive GPU Support
- NVIDIA: Full control via nvidia-smi and nvidia-settings
- AMD: Complete amdgpu sysfs integration (VRAM, power, multi-fan)
- Intel: i915/xe hwmon monitoring and discrete GPU control
- Multi-GPU systems fully supported
- Per-GPU, per-fan control

### Privilege Separation Architecture
- Unprivileged GUI (hf-gtk) for user interface
- Privileged daemon (hf-daemon) for hardware control
- Secure Unix socket IPC
- systemd service integration
- Safety-first design with automatic fallbacks

### Real-Time Monitoring
- Live temperature and fan speed graphs
- GPU metrics: VRAM usage, power draw, utilization
- 100ms control loop for responsive adjustments
- 1-second GUI updates for efficiency
- Smooth exponential moving average filtering

### Profile System
- JSON-based configuration
- Multiple profiles for different scenarios
- Hardware fingerprint validation
- Automatic profile migration
- Curve library for reusable configurations

---

## Architecture

Hyperfan is built as a modular Rust workspace with clean separation of concerns:

| Crate | Description |
|-------|-------------|
| **hf-gtk** | GTK4/Libadwaita GUI application (unprivileged) |
| **hf-daemon** | System daemon for hardware control (privileged) |
| **hf-core** | Core library: hwmon detection, PWM control, fingerprinting |
| **hf-gpu** | GPU-specific library: NVIDIA, AMD, Intel implementations |
| **hf-protocol** | IPC protocol definitions for daemon communication |
| **hf-error** | Unified error types across all crates |

### Design Principles

- **Safety First**: Always fall back to 100% fan speed on errors
- **Privilege Separation**: Minimize attack surface with unprivileged GUI
- **Hardware Agnostic**: Support diverse hardware configurations
- **Modular Design**: Clean crate boundaries for maintainability
- **Type Safety**: Leverage Rust's type system for correctness

---

## Screenshots



---

## Quick Start

### Requirements

- Linux with sysfs hwmon support
- GTK4 4.12+ and libadwaita 1.5+
- Rust toolchain 1.70+ (for building from source)
- Optional: nvidia-smi and nvidia-settings (for NVIDIA GPU control)
- Optional: systemd (recommended for daemon service)

### Install Dependencies

**Debian/Ubuntu:**
```bash
sudo apt install libgtk-4-dev libadwaita-1-dev
```

**Fedora:**
```bash
sudo dnf install gtk4-devel libadwaita-devel
```

**Arch:**
```bash
sudo pacman -S gtk4 libadwaita
```

### Build and Run

```bash
# Clone the repository
git clone https://github.com/HK01569/hyperfan.git
cd hyperfan

# Build optimized release
cargo build --release

# Run the GUI
./target/release/hyperfan
```

### Install the Daemon

For persistent fan control, install the privileged daemon:

```bash
# Build the daemon
cargo build --release -p hf-daemon

# Install binary
sudo cp target/release/hyperfand /usr/local/bin/

# Install and enable systemd service
sudo cp hf-daemon/hyperfan.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now hyperfan.service

# Check status
sudo systemctl status hyperfan.service
## Configuration

Configuration files are stored in multiple locations:

### System Configuration (Daemon)
- `/etc/hyperfan/profile.json` - Active profile with fan mappings and curves
- `/run/hyperfan/daemon.sock` - Unix socket for IPC

### User Configuration (GUI)
- `~/.config/hyperfan/settings.json` - GUI preferences and window state
- `~/.config/hyperfan/curves/` - Saved fan curve library

### Profile Format

Profiles are JSON files containing fan-sensor pairings and curves:

```json
{
  "name": "Gaming Profile",
  "fan_mappings": [
    {
      "fan_path": "/sys/class/hwmon/hwmon3/pwm1",
      "sensor_path": "/sys/class/hwmon/hwmon0/temp1_input",
      "curve_name": "cpu_curve",
      "hardware_id": { /* fingerprint */ }
    }
  ],
  "curves": {
    "cpu_curve": {
      "points": [[30.0, 30.0], [60.0, 50.0], [80.0, 100.0]],
      "smoothing": 0.3
    }
  }
}
```

---

## Advanced Features

### Hardware Fingerprinting

Hyperfan uses stable hardware identifiers to maintain fan pairings across reboots:

- PCI bus ID (e.g., "0000:00:18.3")
- PCI vendor and device IDs
- Driver name and chip model
- PWM channel number

Confidence-based matching algorithm:
- **>0.90**: Safe to use automatically
- **>0.70**: Warn user but allow
- **>0.40**: Require manual confirmation
- **<0.40**: Refuse to use (unsafe)

### GPU Fan Control

**NVIDIA GPUs:**
- Requires nvidia-smi and nvidia-settings
- X11 session with Coolbits enabled
- Supports multiple GPUs and fans per GPU
- Manual and automatic control modes

**AMD GPUs:**
- Direct sysfs control via amdgpu driver
- VRAM usage monitoring
- Power consumption tracking
- Multi-fan GPU support

**Intel GPUs:**
- Temperature monitoring via i915/xe hwmon
- Fan control on discrete GPUs (Arc series)
- Integrated GPU monitoring

### Safety Mechanisms

- Input validation on all user inputs
- Path traversal protection
- File size limits on configurations
- Automatic fallback to 100% on errors
- Profile integrity validation
- Hardware state monitoring

---

## Documentation

Comprehensive documentation is available in the `docs/` directory:

- `app_flow.md` - Application flow and architecture diagrams
- `app_design.md` - Design decisions and technical details
- `app_detail.json` - Structured summary of key features and innovations

For detailed API documentation:
```bash
cargo doc --open
```

---

## Contributing

Contributions are welcome! Whether it's bug reports, feature requests, or pull requests, your input helps make Hyperfan better for everyone.

---

## Support Development

If Hyperfan helps keep your system cool, consider supporting development:

[![ko-fi](https://ko-fi.com/img/githubbutton_sm.svg)](https://ko-fi.com/henryk44801)

---

## License

GNU General Public License v3.0 or later. See [LICENSE.md](LICENSE.md).

---

## Author

**Henry Kleyn** - [GitHub](https://github.com/HK01569)

---

Built with Rust for safety, performance, and reliability.