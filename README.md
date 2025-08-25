# WARNING
hyperfan is in alpha at this time. Make sure you take precautions to avoid damage to your hardware. Feedback, bug reports and feature requests are welcome thank you. If you run --ec-dump and want to contribute that file, please open an issue.

Cheers!

# hyperfan

Hyperfan is a Linux fan control TUI powered by the `hwmon` subsystem. It enumerates temperature sensors, fan RPMs, and PWM controllers, lets you map fans to PWMs, adjust PWM output, and run a guided auto-detect routine to discover fan↔PWM pairings. A headless service mode applies saved mappings and curves.

Status: active development. Requires root privileges for hardware access.

## Author

**Henry Kleyn**
- GitHub: [HK01569](https://github.com/HK01569)

## Features

- Detects `hwmon` chips under `/sys/class/hwmon`
- Reads temperatures (°C/°F/K), fan RPMs, and PWM raw values
- TUI with live refresh and keyboard-driven workflow
- Set PWM as a percentage with safety handling
- Auto-detect fan↔PWM pairings with progress and confidence scores
- Replace existing mappings with detected results in one step
- Headless service mode using saved config and curves
- Optional EC profile dump for sharing/debugging

## Safety notes

- Hyperfan runs with root and writes to `pwmN`/`pwmN_enable` under `hwmon`.
- Auto-detect temporarily ramps PWMs; original states are captured and restored.
- Ensure adequate cooling while testing; laptops/desktops vary in sensor update rate.

## Requirements

- Linux with `sysfs` `hwmon` (most distros)
- For broader sensor support, install `lm-sensors` and consider running `sudo sensors-detect` to load kernel modules

## Install Rust

Recommended via rustup:

```bash
curl https://sh.rustup.rs -sSf | sh
source "$HOME/.cargo/env"
rustup toolchain install stable
```

Fish shell users:

```fish
fish_add_path ~/.cargo/bin
```

Or use your distro packages (may be older):

- Debian/Ubuntu: `sudo apt-get update && sudo apt-get install -y cargo rustc`
- Fedora: `sudo dnf install rust cargo`
- Arch: `sudo pacman -S rust`

## Build and Run

```bash
cargo build --release

# Run TUI (root required for hardware access)
sudo ./target/release/hyperfan
```

## CLI modes

- TUI (default): `sudo hyperfan`
- Save system config: `sudo hyperfan save` → writes `/etc/hyperfan/profile.json`
- Service mode: `sudo hyperfan --service` → headless loop using `/etc/hyperfan/profile.json`
- Dump EC profile: `sudo hyperfan --dump-ec` → writes `/etc/hyperfan/profiles/{ecName}.json`
  - Includes chips, enumerated `fan*`, `pwm*`, `temp*`, and detected pairings with confidence

## Configuration

- User mappings are saved in the app; “Save system config” writes to `/etc/hyperfan/profile.json`.
- Curve configuration is written to `/etc/hyperfan/curves.json` and used by service mode.

## Support

If you find this project helpful, consider supporting development:

[![ko-fi](https://ko-fi.com/img/githubbutton_sm.svg)](https://ko-fi.com/henryk44801)

## License

This project is licensed under the GNU General Public License, version 3 or (at your option) any later version. See `LICENSE.md`.

All Rust source files include a GPL-3.0-or-later header.

## Roadmap

- Configurable auto-detect thresholds and timing
- More robust curve editor UX and visualization
- Additional chipset-specific helpers and safeguards
- GPU support for AMD, nVIDIA and Intel cards. Legacy cards may be supported in time.
- Optional cross-platform support (Windows/macOS) as a stretch goal
