#!/usr/bin/env bash
# Build Libre VMM.
# Sets up the local pkg-config path for libvirt and builds the project.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# Ensure local libvirt symlink exists
mkdir -p "$SCRIPT_DIR/lib/pkgconfig"
if [[ ! -f "$SCRIPT_DIR/lib/libvirt.so" ]]; then
    LIBVIRT_SO=$(find /usr/lib -name "libvirt.so.0" 2>/dev/null | head -1)
    if [[ -n "$LIBVIRT_SO" ]]; then
        ln -sf "$LIBVIRT_SO" "$SCRIPT_DIR/lib/libvirt.so"
        echo "[*] Linked $LIBVIRT_SO -> lib/libvirt.so"
    else
        echo "[!] libvirt.so not found. Install libvirt: sudo apt install libvirt-daemon-system"
        exit 1
    fi
fi

# Ensure pkg-config file exists
if [[ ! -f "$SCRIPT_DIR/lib/pkgconfig/libvirt.pc" ]]; then
    cat > "$SCRIPT_DIR/lib/pkgconfig/libvirt.pc" << PCEOF
prefix=/usr
exec_prefix=\${prefix}
libdir=$SCRIPT_DIR/lib
includedir=\${prefix}/include

Name: libvirt
Description: libvirt C library
Version: 10.0.0
Libs: -L\${libdir} -lvirt
PCEOF
    echo "[*] Created lib/pkgconfig/libvirt.pc"
fi

export PKG_CONFIG_PATH="$SCRIPT_DIR/lib/pkgconfig:${PKG_CONFIG_PATH:-}"

MODE="${1:-release}"
if [[ "$MODE" == "debug" ]]; then
    echo "[*] Building in debug mode..."
    cargo build
else
    echo "[*] Building in release mode..."
    cargo build --release
fi

echo "[*] Build complete!"
echo "    GUI: target/$([[ $MODE == debug ]] && echo debug || echo release)/vmm-gui"
echo "    CLI: target/$([[ $MODE == debug ]] && echo debug || echo release)/vmm-cli"
echo "    API: target/$([[ $MODE == debug ]] && echo debug || echo release)/vmm-api"
