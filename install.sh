#!/usr/bin/env bash
set -euo pipefail

# kdo installer
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/vivekpal1/kdo/main/install.sh | bash
#   ./install.sh                   # build from source (requires Rust)
#   ./install.sh --from-release    # download prebuilt binary

VERSION="${KDO_VERSION:-latest}"
PREFIX="${KDO_PREFIX:-/usr/local}"
BINDIR="${PREFIX}/bin"
REPO="vivekpal1/kdo"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
DIM='\033[2m'
BOLD='\033[1m'
RESET='\033[0m'

info()  { echo -e "${CYAN}${BOLD}kdo${RESET} $*"; }
ok()    { echo -e "  ${GREEN}ok${RESET} $*"; }
err()   { echo -e "  ${RED}error${RESET} $*" >&2; }
dim()   { echo -e "  ${DIM}$*${RESET}"; }

detect_platform() {
    local os arch
    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Linux)  os="unknown-linux-gnu" ;;
        Darwin) os="apple-darwin" ;;
        *)      err "unsupported OS: $os"; exit 1 ;;
    esac

    case "$arch" in
        x86_64)  arch="x86_64" ;;
        aarch64|arm64) arch="aarch64" ;;
        *)       err "unsupported architecture: $arch"; exit 1 ;;
    esac

    echo "${arch}-${os}"
}

install_from_source() {
    info "Installing from source..."

    if ! command -v cargo &>/dev/null; then
        err "Rust toolchain not found."
        echo ""
        echo "  Install Rust first:"
        echo "    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
        echo ""
        echo "  Or install from a prebuilt binary:"
        echo "    $0 --from-release"
        exit 1
    fi

    local tmpdir
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' EXIT

    dim "cloning repository..."
    git clone --depth 1 "https://github.com/${REPO}.git" "$tmpdir/kdo" 2>/dev/null
    cd "$tmpdir/kdo"

    dim "building release binary..."
    cargo build --release --quiet

    dim "installing to ${BINDIR}..."
    install -d "$BINDIR"
    install -m 755 target/release/kdo "$BINDIR/kdo"

    ok "installed kdo to ${BINDIR}/kdo"
    echo ""
    dim "$(kdo --version)"
    echo ""
    info "Run ${BOLD}kdo init${RESET} in your workspace to get started."
}

install_from_release() {
    info "Installing prebuilt binary..."

    local platform
    platform="$(detect_platform)"
    dim "platform: ${platform}"

    if [ "$VERSION" = "latest" ]; then
        dim "fetching latest release..."
        VERSION="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/')"
        if [ -z "$VERSION" ]; then
            err "could not determine latest version. Try installing from source instead."
            exit 1
        fi
    fi
    dim "version: ${VERSION}"

    local archive="kdo-${VERSION}-${platform}.tar.gz"
    local url="https://github.com/${REPO}/releases/download/${VERSION}/${archive}"

    local tmpdir
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' EXIT

    dim "downloading ${archive}..."
    if ! curl -fsSL "$url" -o "$tmpdir/$archive"; then
        err "download failed: $url"
        echo ""
        echo "  No prebuilt binary for your platform? Install from source:"
        echo "    $0"
        exit 1
    fi

    dim "extracting..."
    tar xzf "$tmpdir/$archive" -C "$tmpdir"

    dim "installing to ${BINDIR}..."
    install -d "$BINDIR"
    install -m 755 "$tmpdir/kdo" "$BINDIR/kdo"

    ok "installed kdo ${VERSION} to ${BINDIR}/kdo"
    echo ""
    info "Run ${BOLD}kdo init${RESET} in your workspace to get started."
}

main() {
    echo ""
    info "kdo installer"
    echo ""

    if [ "${1:-}" = "--from-release" ]; then
        install_from_release
    else
        install_from_source
    fi
}

main "$@"
