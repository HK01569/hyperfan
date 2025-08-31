#!/bin/bash

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Clear caches
echo -e "${YELLOW}Cleaning up...${NC}"
rm -rf ./target/release/.fingerprint/*
rm -rf ./target/release/build/*
rm -rf ./src/hyperfan-gui/node_modules/.cache

# Clear Rust build cache if needed
# cargo clean

echo -e "${GREEN}Building Hyperfan...${NC}"

# CRITICAL: Build frontend first - this MUST succeed
echo -e "${YELLOW}Building frontend (REQUIRED)...${NC}"
cd src/hyperfan-gui

# Clear frontend caches
rm -rf ./dist
rm -rf ./.parcel-cache

# Ensure node_modules exist
if [ ! -d "node_modules" ]; then
    echo -e "${YELLOW}Installing frontend dependencies...${NC}"
    npm install --no-package-lock --no-shrinkwrap --no-optional || { 
        echo -e "${RED}Frontend dependency installation failed!${NC}"
        exit 1 
    }
fi

# Build frontend - MUST succeed
npm run build || { 
    echo -e "${RED}Frontend build failed! Cannot continue.${NC}"
    exit 1 
}

# Verify dist folder exists and has content
if [ ! -d "dist" ] || [ ! -f "dist/index.html" ]; then
    echo -e "${RED}Frontend build verification failed! dist/index.html not found.${NC}"
    exit 1
fi

echo -e "${GREEN}Frontend built successfully!${NC}"
cd ../..

# Build Rust backend
echo -e "${YELLOW}Building Rust backend...${NC}"
cargo build --release

if [ $? -eq 0 ]; then
    echo -e "${GREEN}Build successful!${NC}"
    echo -e "${GREEN}Binary location: ./target/release/hyperfan-gui${NC}"
    echo -e "${YELLOW}To run: sudo ./target/release/hyperfan-gui${NC}"
else
    echo -e "${RED}Build failed!${NC}"
    exit 1
fi
