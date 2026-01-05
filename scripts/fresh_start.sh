#!/bin/bash
#
# Hyperfan Fresh Start Script
# 
# This script removes all Hyperfan service files, config files, and cached data
# to simulate a fresh install experience.
#
# Usage: ./scripts/fresh_start.sh
#

set -e

echo "=== Hyperfan Fresh Start Script ==="
echo ""

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Check if running as root for service removal
NEED_SUDO=false
if [ -f /etc/systemd/system/hyperfan.service ] || \
   [ -f /etc/init.d/hyperfand ] || \
   [ -d /etc/sv/hyperfand ] || \
   [ -f /etc/rc.d/hyperfand ] || \
   [ -f /usr/local/etc/rc.d/hyperfand ]; then
    NEED_SUDO=true
fi

# Stop and disable systemd service
if systemctl is-active --quiet hyperfan.service 2>/dev/null; then
    echo -e "${YELLOW}Stopping hyperfan.service...${NC}"
    sudo systemctl stop hyperfan.service
fi

if systemctl is-enabled --quiet hyperfan.service 2>/dev/null; then
    echo -e "${YELLOW}Disabling hyperfan.service...${NC}"
    sudo systemctl disable hyperfan.service
fi

# Kill any running daemon processes
if pgrep -x hyperfand > /dev/null 2>&1; then
    echo -e "${YELLOW}Killing hyperfand processes...${NC}"
    sudo pkill -9 hyperfand 2>/dev/null || true
fi

echo ""
echo "=== Removing Service Files ==="

# Systemd service files
if [ -f /etc/systemd/system/hyperfan.service ]; then
    echo -e "${RED}Removing /etc/systemd/system/hyperfan.service${NC}"
    sudo rm -f /etc/systemd/system/hyperfan.service
    sudo systemctl daemon-reload
fi

# OpenRC service files
if [ -f /etc/init.d/hyperfand ]; then
    echo -e "${RED}Removing /etc/init.d/hyperfand${NC}"
    sudo rm -f /etc/init.d/hyperfand
fi

# Runit service files
if [ -d /etc/sv/hyperfand ]; then
    echo -e "${RED}Removing /etc/sv/hyperfand${NC}"
    sudo rm -rf /etc/sv/hyperfand
fi
if [ -L /var/service/hyperfand ]; then
    echo -e "${RED}Removing /var/service/hyperfand symlink${NC}"
    sudo rm -f /var/service/hyperfand
fi

# BSD rc.d service files
if [ -f /etc/rc.d/hyperfand ]; then
    echo -e "${RED}Removing /etc/rc.d/hyperfand${NC}"
    sudo rm -f /etc/rc.d/hyperfand
fi
if [ -f /usr/local/etc/rc.d/hyperfand ]; then
    echo -e "${RED}Removing /usr/local/etc/rc.d/hyperfand${NC}"
    sudo rm -f /usr/local/etc/rc.d/hyperfand
fi

# Remove daemon binary from system paths
if [ -f /usr/local/bin/hyperfand ]; then
    echo -e "${RED}Removing /usr/local/bin/hyperfand${NC}"
    sudo rm -f /usr/local/bin/hyperfand
fi
if [ -f /usr/bin/hyperfand ]; then
    echo -e "${RED}Removing /usr/bin/hyperfand${NC}"
    sudo rm -f /usr/bin/hyperfand
fi

# Remove socket file
if [ -S /run/hyperfan.sock ]; then
    echo -e "${RED}Removing /run/hyperfan.sock${NC}"
    sudo rm -f /run/hyperfan.sock
fi
if [ -S /var/run/hyperfan.sock ]; then
    echo -e "${RED}Removing /var/run/hyperfan.sock${NC}"
    sudo rm -f /var/run/hyperfan.sock
fi

echo ""
echo "=== Removing User Config Files ==="

# User config directory
CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/hyperfan"
if [ -d "$CONFIG_DIR" ]; then
    echo -e "${RED}Removing $CONFIG_DIR${NC}"
    rm -rf "$CONFIG_DIR"
fi

# Legacy config locations
if [ -d "$HOME/.hyperfan" ]; then
    echo -e "${RED}Removing $HOME/.hyperfan${NC}"
    rm -rf "$HOME/.hyperfan"
fi

echo ""
echo "=== Removing Cache Files ==="

# User cache directory
CACHE_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/hyperfan"
if [ -d "$CACHE_DIR" ]; then
    echo -e "${RED}Removing $CACHE_DIR${NC}"
    rm -rf "$CACHE_DIR"
fi

echo ""
echo "=== Removing System Config Files ==="

# System-wide config (requires sudo)
if [ -d /etc/hyperfan ]; then
    echo -e "${RED}Removing /etc/hyperfan${NC}"
    sudo rm -rf /etc/hyperfan
fi

echo ""
echo -e "${GREEN}=== Fresh Start Complete ===${NC}"
echo ""
echo "Hyperfan has been completely removed. You can now:"
echo "  1. Run 'cargo run --package hf-gtk' to test fresh start experience"
echo "  2. Or install from packages for a production test"
echo ""
