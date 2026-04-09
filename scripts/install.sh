#!/usr/bin/env bash

# Exit immediately if a command exits with a non-zero status
set -e

# --- Configuration ---
APP_NAME="anatro-rs"
INSTALL_ROOT="$HOME/.local"
INSTALL_BIN_DIR="$INSTALL_ROOT/bin"
EXECUTABLE_PATH="$INSTALL_BIN_DIR/$APP_NAME"

# --- Formatting ---
# ANSI escape codes for styling
BOLD='\033[1m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[0;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

log_info() { echo -e "${BLUE}[INFO]${NC} $1"; }
log_success() { echo -e "${GREEN}[SUCCESS]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARNING]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }
log_step() { echo -e "\n${BOLD}>>> $1${NC}"; }

# --- Script Start ---
echo -e "${BOLD}==================================================${NC}"
echo -e "${BOLD}       $APP_NAME Automated Installer        ${NC}"
echo -e "${BOLD}==================================================${NC}"

# --- Pre-flight Checks ---
log_step "Checking dependencies..."

if ! command -v cargo &> /dev/null; then
    log_error "Cargo is not installed or not in PATH."
    echo "Please install Rust and Cargo (e.g., via https://rustup.rs/) before running this script."
    exit 1
fi
log_info "Cargo found at $(command -v cargo)"

# Ensure the destination directory exists
if [ ! -d "$INSTALL_BIN_DIR" ]; then
    log_info "Creating installation directory: $INSTALL_BIN_DIR"
    mkdir -p "$INSTALL_BIN_DIR"
fi

# --- Build & Install ---
BUILD_BIN="target/release/$APP_NAME"

if [ -f "$BUILD_BIN" ]; then
    log_step "Installing Pre-built Binary..."
    log_info "Found pre-built binary at $BUILD_BIN. Copying to $INSTALL_BIN_DIR..."
    cp "$BUILD_BIN" "$EXECUTABLE_PATH"
    log_success "Binary copied successfully."
else
    log_step "Building and Installing $APP_NAME..."
    log_info "This may take a moment. Building in release mode..."
    if cargo build --release; then
        log_success "Compilation completed."
        log_info "Copying to $INSTALL_BIN_DIR..."
        cp "$BUILD_BIN" "$EXECUTABLE_PATH"
    else
        log_error "Build failed."
        exit 1
    fi
fi

# --- Verification ---
log_step "Verifying Installation..."

if [ -f "$EXECUTABLE_PATH" ]; then
    # Ensure it's executable
    chmod +x "$EXECUTABLE_PATH"
    
    # Try to get the version to prove it runs
    if "$EXECUTABLE_PATH" --version &> /dev/null; then
        INSTALLED_VERSION=$("$EXECUTABLE_PATH" --version)
        log_success "$APP_NAME ($INSTALLED_VERSION) is successfully installed at:"
        echo -e "          ${BOLD}$EXECUTABLE_PATH${NC}"
        
        # --- Cleanup ---
        log_step "Cleaning up build artifacts..."
        log_info "Removing target/release directory to free up space..."
        if rm -rf target/release; then
            log_success "Cleanup complete."
        else
            log_warn "Failed to remove target/release."
        fi
    else
        log_warn "Binary exists at $EXECUTABLE_PATH but '--version' failed."
    fi
else
    log_error "Failed to find the executable at $EXECUTABLE_PATH after installation."
    exit 1
fi

# --- Post-install Checks ---
echo ""
echo -e "${BOLD}==================================================${NC}"
if [[ ":$PATH:" != *":$INSTALL_BIN_DIR:"* ]]; then
    log_warn "The directory $INSTALL_BIN_DIR is NOT in your PATH."
    echo "You will need to add it to your shell's configuration file"
    echo "to run '$APP_NAME' globally."
else
    log_success "$APP_NAME is installed and ready to use!"
    echo "You can now run '$APP_NAME --help' from anywhere."
fi
echo -e "${BOLD}==================================================${NC}"
