#!/usr/bin/env bash
set -euo pipefail

# kdo installer
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/vivekpal1/kdo/main/install.sh | bash
#   ./install.sh                   # build from source (requires Rust)
#   ./install.sh --from-release    # download prebuilt binary
#
# Environment variables:
#   KDO_PREFIX   Install prefix (default: auto-detect ~/.local or /usr/local)
#   KDO_VERSION  Binary version for --from-release (default: latest)

REPO="vivekpal1/kdo"
VERSION="${KDO_VERSION:-latest}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
DIM='\033[2m'
BOLD='\033[1m'
RESET='\033[0m'

info()  { echo -e "${CYAN}${BOLD}kdo${RESET} $*"; }
ok()    { echo -e "  ${GREEN}ok${RESET}   $*"; }
err()   { echo -e "  ${RED}error${RESET} $*" >&2; }
dim()   { echo -e "  ${DIM}$*${RESET}"; }
step()  { echo -e "  ${BOLD}»${RESET} $*"; }

# ---------------------------------------------------------------------------
# Determine install prefix: KDO_PREFIX > ~/.local (if on PATH) > ~/.local (fallback)
# Never try /usr/local without sudo — that just breaks.
# ---------------------------------------------------------------------------
resolve_prefix() {
    if [ -n "${KDO_PREFIX:-}" ]; then
        echo "$KDO_PREFIX"
        return
    fi

    # Prefer ~/.local/bin if it's already on PATH
    if echo "$PATH" | tr ':' '\n' | grep -q "$HOME/.local/bin"; then
        echo "$HOME/.local"
        return
    fi

    # Fall back to ~/.local/bin and remind user to update PATH
    echo "$HOME/.local"
}

ensure_on_path() {
    local bindir="$1"
    if ! echo "$PATH" | tr ':' '\n' | grep -qx "$bindir"; then
        echo ""
        echo -e "  ${BOLD}Note:${RESET} ${bindir} is not on your PATH."
        echo -e "  Add this to your shell profile (${DIM}~/.zshrc${RESET} or ${DIM}~/.bashrc${RESET}):"
        echo ""
        echo -e "    ${CYAN}export PATH=\"${bindir}:\$PATH\"${RESET}"
        echo ""
    fi
}

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
        x86_64)          arch="x86_64" ;;
        aarch64 | arm64) arch="aarch64" ;;
        *)               err "unsupported architecture: $arch"; exit 1 ;;
    esac

    echo "${arch}-${os}"
}

install_from_source() {
    info "Installing kdo from source..."
    echo ""

    if ! command -v cargo &>/dev/null; then
        err "Rust toolchain not found."
        echo ""
        echo "  Install Rust first:"
        echo "    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
        echo ""
        echo "  Or install a prebuilt binary:"
        echo "    $0 --from-release"
        exit 1
    fi

    local prefix bindir tmpdir
    prefix="$(resolve_prefix)"
    bindir="${prefix}/bin"
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' EXIT

    step "cloning repository..."
    git clone --depth 1 "https://github.com/${REPO}.git" "$tmpdir/kdo" 2>/dev/null

    step "building release binary (this takes ~30s)..."
    (cd "$tmpdir/kdo" && cargo build --release --quiet)

    step "installing to ${bindir}..."
    mkdir -p "$bindir"
    install -m 755 "$tmpdir/kdo/target/release/kdo" "$bindir/kdo"

    echo ""
    ok "installed kdo $("$bindir/kdo" --version) → ${bindir}/kdo"
    ensure_on_path "$bindir"
    info "Run ${BOLD}kdo init${RESET} in your workspace to get started."
}

install_from_release() {
    info "Installing kdo prebuilt binary..."
    echo ""

    local platform prefix bindir
    platform="$(detect_platform)"
    prefix="$(resolve_prefix)"
    bindir="${prefix}/bin"

    dim "platform: ${platform}"

    if [ "$VERSION" = "latest" ]; then
        step "fetching latest release tag..."
        VERSION="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
            | grep '"tag_name"' \
            | sed -E 's/.*"([^"]+)".*/\1/')"
        if [ -z "$VERSION" ]; then
            err "could not determine latest version."
            echo ""
            echo "  No GitHub releases yet? Install from source instead:"
            echo "    $0"
            exit 1
        fi
    fi
    dim "version: ${VERSION}"

    local archive url tmpdir
    archive="kdo-${VERSION}-${platform}.tar.gz"
    url="https://github.com/${REPO}/releases/download/${VERSION}/${archive}"
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' EXIT

    step "downloading ${archive}..."
    if ! curl -fsSL "$url" -o "$tmpdir/$archive"; then
        err "download failed: ${url}"
        echo ""
        echo "  No prebuilt binary for your platform? Install from source:"
        echo "    $0"
        exit 1
    fi

    step "extracting..."
    tar xzf "$tmpdir/$archive" -C "$tmpdir"

    step "installing to ${bindir}..."
    mkdir -p "$bindir"
    install -m 755 "$tmpdir/kdo" "$bindir/kdo"

    echo ""
    ok "installed kdo ${VERSION} → ${bindir}/kdo"
    ensure_on_path "$bindir"
    info "Run ${BOLD}kdo init${RESET} in your workspace to get started."
}

main() {
    echo ""
    if [ "${1:-}" = "--from-release" ]; then
        install_from_release
    else
        install_from_source
    fi
}

main "$@"
