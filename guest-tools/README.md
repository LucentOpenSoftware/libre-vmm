# Libre VMM Guest Tools

Pre-packaged tools for Windows and Linux guests to enable full host-guest integration.

## Windows (`windows/`)

### Components

| File | Source | License | Purpose |
|------|--------|---------|---------|
| `virtio-win.iso` | [Fedora/Red Hat](https://fedorapeople.org/groups/virt/virtio-win/direct-downloads/stable-virtio/) | Apache-2.0 / BSD | VirtIO drivers (net, storage, balloon, GPU, serial, RNG), QEMU Guest Agent, SPICE agent |
| `spice-guest-tools.exe` | [spice-space.org](https://www.spice-space.org/download/windows/spice-guest-tools/) | Apache-2.0 | Clipboard sync, display auto-resize, USB redirection (NSIS installer, `/S` for silent) |
| `winfsp.msi` | [WinFsp](https://github.com/winfsp/winfsp) | GPL-3.0 | FUSE for Windows — required for virtiofs shared folder support |

### What each component provides

**virtio-win.iso** contains:
- `virtio-win-guest-tools.exe` — Combined installer (drivers + QGA + SPICE agent)
- Individual driver folders: `viostor/`, `NetKVM/`, `Balloon/`, `vioserial/`, `qxldod/`, `viogpudo/`, `viofs/`, `viorng/`, `pvpanic/`
- `guest-agent/` — QEMU Guest Agent MSI (`qemu-ga-x86_64.msi`)
- Signed drivers for Windows 7 through Windows 11 (x86, x64, ARM64)

**spice-guest-tools.exe** provides:
- QXL display driver (display auto-resize)
- SPICE VDAgent (clipboard, mouse, display change notifications)
- Silent install: `spice-guest-tools.exe /S`

**winfsp.msi** provides:
- FUSE filesystem support for Windows
- Required by the virtiofs driver (`viofs`) to mount host shared folders
- Silent install: `msiexec /i winfsp.msi /qn`

### Installation order (manual)

1. Mount `virtio-win.iso` via Libre VMM → VM menu → Install Guest Tools
2. Run `D:\virtio-win-guest-tools.exe` inside the guest (installs all drivers + QGA)
3. Install `spice-guest-tools.exe` for clipboard/display (if using SPICE)
4. Install `winfsp.msi` if using shared folders via virtiofs

### Silent install (via QGA)

```
# After mounting virtio-win.iso as D: drive
virtio-win-guest-tools.exe /S                    # All drivers + QGA
spice-guest-tools.exe /S                          # SPICE agent
msiexec /i winfsp.msi /qn                        # WinFsp (virtiofs)
```

## Linux

Linux guests use standard package managers — no bundled tools needed:

```bash
# Debian/Ubuntu
sudo apt install qemu-guest-agent spice-vdagent

# Fedora/RHEL
sudo dnf install qemu-guest-agent spice-vdagent

# Arch
sudo pacman -S qemu-guest-agent spice-vdagent

# openSUSE
sudo zypper install qemu-guest-agent spice-vdagent
```

## Future: Libre VMM Tools (all-in-one)

Planned single installer that bundles everything above plus:
- Auto-detection of missing components
- Libre VMM tray icon (connection status, shared folders)
- Auto-mount virtiofs shared folders
- Display resize helper
- Battery/thermal driver configuration
