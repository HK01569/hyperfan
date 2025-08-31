#!/bin/bash

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Function to run a command with GUI sudo prompt
run_with_gui_sudo() {
    if command -v pkexec >/dev/null 2>&1; then
        # Use pkexec for graphical sudo prompt
        pkexec "$@"
    else
        # Fallback to sudo
        sudo "$@"
    fi
}

# Check if running as root, if not, restart with GUI sudo prompt
if [ "$EUID" -ne 0 ]; then
    echo -e "${YELLOW}This application requires root privileges to access hardware controls.${NC}"
    echo -e "${YELLOW}Please enter your password when prompted...${NC}"
    
    # Get the absolute path to this script
    SCRIPT_PATH="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"
    
    # Run with sudo, preserving necessary environment variables
    if ! run_with_gui_sudo env "PATH=$PATH" "DISPLAY=$DISPLAY" "XAUTHORITY=$XAUTHORITY" \
        "XDG_RUNTIME_DIR=$XDG_RUNTIME_DIR" "DBUS_SESSION_BUS_ADDRESS=$DBUS_SESSION_BUS_ADDRESS" \
        "$SCRIPT_PATH" "$@"; then
        echo -e "${RED}Failed to obtain root privileges. Please try again.${NC}" >&2
        exit 1
    fi
    exit 0
fi

# ALWAYS ensure frontend is built - this prevents white screen
echo -e "${YELLOW}Verifying frontend build...${NC}"
cd "$(dirname "$0")/src/hyperfan-gui" || { echo -e "${RED}Failed to enter frontend directory${NC}"; exit 1; }

# Check if dist exists and is recent
if [ ! -d "dist" ] || [ ! -f "dist/index.html" ]; then
    echo -e "${YELLOW}Frontend not built. Building now...${NC}"
    if [ ! -d "node_modules" ]; then
        npm install || { echo -e "${RED}Failed to install dependencies!${NC}"; exit 1; }
    fi
    npm run build || { echo -e "${RED}Frontend build failed!${NC}"; exit 1; }
fi

echo -e "${GREEN}✓ Frontend verified${NC}"
cd "$(dirname "$0")"

# Build backend if needed
if [ ! -f "./target/release/hyperfan-gui" ]; then
    echo -e "${YELLOW}Binary not found. Building backend...${NC}"
    /home/hck/.cargo/bin/cargo build --release || { echo -e "${RED}Backend build failed!${NC}"; exit 1; }
fi

# Launch the GUI application
echo -e "${GREEN}🚀 Launching Hyperfan GUI...${NC}"

# Set up environment for better compatibility (Wayland first)
# On some systems WebKit's DMABUF/GBM path crashes under Wayland; disable it.
export WEBKIT_DISABLE_DMABUF_RENDERER=1
export WEBKIT_DISABLE_COMPOSITING_MODE=1
# Prefer Wayland, but allow X11 fallback automatically
export GDK_BACKEND=${GDK_BACKEND:-wayland,x11}
# Prefer wayland for Qt too, with xcb fallback
export QT_QPA_PLATFORM=${QT_QPA_PLATFORM:-wayland;xcb}
export RUST_BACKTRACE=1
export TAURI_DEVTOOLS=1

# Run the application
exec ./target/release/hyperfan-gui
