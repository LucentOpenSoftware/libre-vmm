#!/usr/bin/env bash
# Libre VMM — Dependency Setup Script
# Installs all required system packages for building and running Libre VMM.
# Must be run with sudo or as root.
set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info()  { echo -e "${GREEN}[INFO]${NC}  $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC}  $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*"; exit 1; }

if [[ $EUID -ne 0 ]]; then
    error "This script must be run with sudo: sudo bash $0"
fi

info "Detecting distribution..."
if command -v apt-get &>/dev/null; then
    PKG_MGR="apt"
elif command -v dnf &>/dev/null; then
    PKG_MGR="dnf"
elif command -v pacman &>/dev/null; then
    PKG_MGR="pacman"
else
    error "Unsupported package manager. Install dependencies manually."
fi

info "Using package manager: $PKG_MGR"

case "$PKG_MGR" in
    apt)
        apt-get update -qq
        apt-get install -y \
            qemu-system-x86 \
            qemu-utils \
            qemu-system-gui \
            qemu-system-modules-spice \
            libvirt-daemon-system \
            libvirt-clients \
            libvirt-dev \
            virt-install \
            ovmf \
            swtpm \
            swtpm-tools \
            spice-client-gtk \
            libspice-client-gtk-3.0-dev \
            libgtk-3-dev \
            pkg-config \
            build-essential \
            bridge-utils \
            dnsmasq-base \
            ebtables \
            libguestfs-tools \
            cloud-image-utils
        ;;
    dnf)
        dnf install -y \
            qemu-kvm \
            qemu-img \
            libvirt \
            libvirt-devel \
            virt-install \
            edk2-ovmf \
            swtpm \
            swtpm-tools \
            spice-gtk3 \
            spice-gtk3-devel \
            gtk3-devel \
            pkg-config \
            gcc \
            bridge-utils \
            dnsmasq \
            libguestfs-tools
        ;;
    pacman)
        pacman -Syu --noconfirm \
            qemu-full \
            libvirt \
            virt-install \
            edk2-ovmf \
            swtpm \
            spice-gtk \
            gtk3 \
            pkgconf \
            base-devel \
            bridge-utils \
            dnsmasq \
            ebtables
        ;;
esac

info "Enabling and starting libvirtd..."
systemctl enable --now libvirtd
systemctl enable --now virtlogd

# Add current user to libvirt group
REAL_USER="${SUDO_USER:-$USER}"
if ! groups "$REAL_USER" | grep -q libvirt; then
    info "Adding $REAL_USER to libvirt and kvm groups..."
    usermod -aG libvirt "$REAL_USER"
    usermod -aG kvm "$REAL_USER"
    warn "You must log out and back in for group changes to take effect."
fi

# Ensure default network exists and is active
if ! virsh net-info default &>/dev/null; then
    info "Creating default NAT network..."
    virsh net-define /usr/share/libvirt/networks/default.xml 2>/dev/null || true
fi
virsh net-autostart default 2>/dev/null || true
virsh net-start default 2>/dev/null || true

# Verify KVM support
if [[ -e /dev/kvm ]]; then
    info "KVM acceleration: AVAILABLE"
else
    warn "KVM acceleration: NOT AVAILABLE (VMs will run in emulation mode — much slower)"
fi

info "============================================"
info "  Libre VMM dependencies installed!"
info "  Run: cargo build --release"
info "============================================"
