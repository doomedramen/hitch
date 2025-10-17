#!/bin/bash
# Hitch installer script
# Usage: curl -fsSL https://raw.githubusercontent.com/DoomedRamen/hitch/main/install.sh | bash

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Configuration
REPO="DoomedRamen/hitch"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"
BINARY_NAME="hitch"

# Functions
error() {
    echo -e "${RED}Error: $1${NC}" >&2
    exit 1
}

info() {
    echo -e "${GREEN}$1${NC}"
}

warning() {
    echo -e "${YELLOW}$1${NC}"
}

detect_os_arch() {
    # Detect OS
    OS=$(uname -s | tr '[:upper:]' '[:lower:]')
    case "$OS" in
        darwin) OS="darwin" ;;
        linux) OS="linux" ;;
        mingw*|msys*|cygwin*) OS="windows" ;;
        *) error "Unsupported operating system: $OS" ;;
    esac

    # Detect architecture
    ARCH=$(uname -m)
    case "$ARCH" in
        x86_64|amd64) ARCH="amd64" ;;
        aarch64|arm64) ARCH="arm64" ;;
        armv7l|armv6l) ARCH="arm" ;;
        *) error "Unsupported architecture: $ARCH" ;;
    esac

    info "Detected platform: ${OS}/${ARCH}"
}

get_latest_version() {
    # Try to get latest version from GitHub API
    VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')

    if [ -z "$VERSION" ]; then
        error "Failed to fetch latest version from GitHub"
    fi

    info "Latest version: ${VERSION}"
}

check_existing_installation() {
    if command -v hitch >/dev/null 2>&1; then
        EXISTING_VERSION=$(hitch --version 2>/dev/null | head -n1 | awk '{print $NF}' || echo "unknown")
        warning "Existing installation found: ${EXISTING_VERSION}"
        warning "This will overwrite the existing installation"
    fi
}

download_and_install() {
    # Construct download URL
    ARCHIVE_NAME="${BINARY_NAME}_${VERSION#v}_${OS}_${ARCH}.tar.gz"
    if [ "$OS" = "windows" ]; then
        ARCHIVE_NAME="${BINARY_NAME}_${VERSION#v}_${OS}_${ARCH}.zip"
    fi

    DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/${ARCHIVE_NAME}"

    info "Downloading: ${DOWNLOAD_URL}"

    # Create temporary directory
    TMP_DIR=$(mktemp -d)
    trap "rm -rf ${TMP_DIR}" EXIT

    # Download archive
    if ! curl -fsSL "${DOWNLOAD_URL}" -o "${TMP_DIR}/${ARCHIVE_NAME}"; then
        error "Failed to download ${DOWNLOAD_URL}"
    fi

    info "Downloaded to ${TMP_DIR}/${ARCHIVE_NAME}"

    # Extract archive
    cd "${TMP_DIR}"
    if [ "$OS" = "windows" ]; then
        unzip -q "${ARCHIVE_NAME}" || error "Failed to extract archive"
    else
        tar -xzf "${ARCHIVE_NAME}" || error "Failed to extract archive"
    fi

    # Verify binary exists
    if [ ! -f "${BINARY_NAME}" ]; then
        error "Binary not found in archive"
    fi

    # Make binary executable
    chmod +x "${BINARY_NAME}"

    # Install binary
    info "Installing to ${INSTALL_DIR}/${BINARY_NAME}"

    # Check if we need sudo
    if [ -w "${INSTALL_DIR}" ]; then
        mv "${BINARY_NAME}" "${INSTALL_DIR}/${BINARY_NAME}"
    else
        warning "Requiring sudo to install to ${INSTALL_DIR}"
        sudo mv "${BINARY_NAME}" "${INSTALL_DIR}/${BINARY_NAME}"
    fi

    # Verify installation
    if ! command -v hitch >/dev/null 2>&1; then
        error "Installation failed: hitch command not found in PATH"
    fi

    INSTALLED_VERSION=$(hitch --version 2>/dev/null | head -n1 || echo "unknown")
    info "Successfully installed: ${INSTALLED_VERSION}"
}

verify_checksum() {
    # Optional: Verify checksum if available
    CHECKSUMS_URL="https://github.com/${REPO}/releases/download/${VERSION}/checksums.txt"

    if curl -fsSL "${CHECKSUMS_URL}" -o "${TMP_DIR}/checksums.txt" 2>/dev/null; then
        cd "${TMP_DIR}"
        if grep -q "${ARCHIVE_NAME}" checksums.txt; then
            if command -v sha256sum >/dev/null 2>&1; then
                info "Verifying checksum..."
                grep "${ARCHIVE_NAME}" checksums.txt | sha256sum -c - >/dev/null 2>&1 || warning "Checksum verification failed"
            elif command -v shasum >/dev/null 2>&1; then
                info "Verifying checksum..."
                grep "${ARCHIVE_NAME}" checksums.txt | shasum -a 256 -c - >/dev/null 2>&1 || warning "Checksum verification failed"
            fi
        fi
    fi
}

print_usage() {
    cat <<EOF

${GREEN}Hitch has been installed!${NC}

Get started:
  ${YELLOW}hitch init${NC}                  # Initialize in your repo
  ${YELLOW}hitch status${NC}                # View environment status
  ${YELLOW}hitch promote <branch> to dev${NC}  # Deploy to dev

Documentation: https://github.com/${REPO}

EOF
}

# Main installation flow
main() {
    echo ""
    info "=== Hitch Installer ==="
    echo ""

    # Check for curl
    if ! command -v curl >/dev/null 2>&1; then
        error "curl is required but not installed"
    fi

    # Detect platform
    detect_os_arch

    # Get latest version
    get_latest_version

    # Check existing installation
    check_existing_installation

    # Verify checksum (optional)
    # verify_checksum

    # Download and install
    download_and_install

    # Print usage instructions
    print_usage
}

# Run main installation
main
