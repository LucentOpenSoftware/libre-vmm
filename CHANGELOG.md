# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-31

Initial public release. Implements roughly **85-90% of VMware Workstation
Pro 17 feature parity** for single-host VM management, with several
differentiators VMware Workstation structurally cannot ship.

Workspace: `vmm-types`, `vmm-core`, `vmm-gui`, `vmm-cli`, `vmm-api`.
~70,000 lines of Rust. 438 tests passing across the workspace.

### Added

#### Core VM management
- Full lifecycle: create, start, shutdown, force-stop, pause, resume, reboot, suspend
- 24 QEMU guest architectures (x86_64, ARM64, RISC-V, MIPS, PPC, s390x, m68k, SPARC, and more), defined in `vmm-core/src/qemu_archs.rs`
- 40+ guest OS templates with auto-configured devices, firmware, and tools ISOs
- macOS guest support — Apple SMC OSK injection, Penryn CPU, OpenCore bootloader, vmxnet3 (Sierra → Sequoia)
- Hot-add and hot-unplug of disks on all four buses (virtio-blk, virtio-scsi, NVMe, SATA, IDE)
- CPU pinning, IO thread pinning, CPU topology (sockets × cores × threads), up to 512 vCPUs and 1 TiB RAM (clamped)
- Resource limits: memory reservation/shares, disk I/O throttle, NIC bandwidth caps
- Hugepages toggle, `io_uring` AIO backend, side-channel mitigation toggle
- Side-channel mitigation toggle (`l1d_flush=on/off`) per VM
- Memory ballooning UI with stats
- Restricted VMs — atomic policy save, op-allowlist, expiration dates
- VM notes, tags, folders, favorites, box types (Standard / Hardware Lab / Power User)

#### Snapshots & cloning
- Visual snapshot tree with branches, parent lines, and current-state marker
- Live snapshots including RAM state
- AutoProtect — scheduled snapshots with retention
- Quiesced snapshots via `qemu-guest-agent` `guest-fsfreeze-freeze/thaw` with drop-guard that guarantees thaw on any path
- Full clones and linked clones (qcow2 backing files)
- Template VMs marked read-only as clone bases

#### Storage
- qcow2 native (sparse, encryptable, COW), with import/export to VMDK, raw, VDI
- LUKS-encrypted disks at create time
- Disk compaction (`qemu-img convert` reclaim)
- Disk format conversion UI
- Independent-persistent and nonpersistent disk modes
- BTRFS CoW auto-disable for VM directories
- Virtual Media Manager with orphan detection

#### Networking
- NAT, Bridged, Host-only, and **LAN segments** (isolated VM-to-VM bridges)
- DHCP server config via libvirt
- Port forwarding GUI
- **Per-VM nftables firewall rules** via libvirt nwfilter
- **Automatic guest port forwarding** — Lima-style detect via `qemu-guest-agent` (`list_guest_listeners` + `sync_auto_forwards`)
- Network conditioner — latency, packet loss, bandwidth per NIC
- NIC bandwidth limiting
- USB controllers (1.1, 2.0, 3.0) and USB device passthrough

#### Firmware & security
- **LibreUEFI** — our own EDK2 fork with branding, battery/thermal ACPI extensions, and `fw_cfg` bridge
- Secure Boot via LibreUEFI
- swtpm-based TPM 2.0 emulation
- Boot-to-firmware option
- Memory zeroization for passphrases (zeroize-on-drop)
- Atomic file writes for all config files

#### Guest integration
- `qemu-guest-agent` + `spice-vdagent` (rich open-vm-tools equivalent)
- Bidirectional clipboard, drag-and-drop files (SPICE)
- Time sync, autofit display
- **virtiofs** shared folders (POSIX-correct, faster than VMware HGFS)
- Cloud-init / NoCloud datasource provisioning
- Windows autounattend.xml provisioning
- Ignition / Butane support (Fedora CoreOS)
- Display: 1-8 monitors, virtio-gpu with 3D acceleration (virgl)

#### Display & console
- VNC and SPICE display protocols
- Multi-monitor (up to 8 heads)
- Screen recording — start/stop from console
- Picture-in-Picture host window
- Console toolbar with Looking Glass quick-launch
- Tabbed VM views ("Quick Switch")
- Full-screen mode, high-DPI scaling

#### Live migration (differentiator)
- 4-step GUI wizard for live migration between Libre VMM hosts
- Progress reporting and cancellation
- Sidebar "Migrate..." entry per VM

#### GPU passthrough (differentiator)
- IOMMU group scanner (`vmm-core/src/hardware/pci.rs`)
- VFIO bind/unbind helpers (`vmm-core/src/hardware/vfio.rs`)
- 4-step single-GPU passthrough wizard with hook script generation and sudoers helper
- Multi-GPU passthrough
- Looking Glass integration (IVSHMEM + auto-launch client)

#### REST API (differentiator)
- `vmm-api` Axum server, 18 routes
- OpenAPI 3.1 spec generated to [`docs/openapi.json`](docs/openapi.json)
- Swagger-UI at `/api/v1/docs`, ReDoc at `/api/v1/redoc`
- X-API-Key authentication

#### CLI
- `vmm-cli` with 16 subcommands (`clap`-based)
- Shell completions for bash, zsh, fish via `vmm completions <shell>`
- `install.sh` wires completions into the host shell config

#### UX
- First-run wizard (7 steps, 837 lines) with system detection
- System check module (`vmm-core/src/system_check.rs`) — KVM, libvirt, OVMF, swtpm, virtiofsd
- VM discovery on first run from libvirt, Quickemu (incl. snap paths), `/var/lib/libvirt`, VirtualBox, VMware
- Import wizards for libvirt XML, Quickemu `.conf`, VMware `.vmx`, VirtualBox `.vbox`
- Disk handling options on import: symlink, copy, move, convert
- Home tab with quick actions, recent VMs grid, error banner
- Dark mode default, configurable UI scale
- Desktop notifications (VM started, stopped, snapshot done)
- OS icons in sidebar

#### Differentiators vs VMware Workstation
- **Live migration** between hosts
- **GPU passthrough** (single, multi, Looking Glass)
- **24 guest architectures** vs VMware's x86-only
- **REST API + OpenAPI 3.1 spec**
- **qcow2 native** vs VMDK only
- **Cloud-init / autounattend** built in
- **Wayland-native** — egui + winit, see [`docs/WAYLAND-COMPATIBILITY.md`](docs/WAYLAND-COMPATIBILITY.md)
- **Per-VM nftables firewall**
- **Automatic guest port forwarding**
- **Declarative TOML/YAML VM specs** — `VmConfig::to_toml`, `to_yaml`, `from_toml`, `from_yaml`
- **LibreUEFI** custom firmware
- **CLI-first ergonomics** — 16 subcommands with completions
- **Network conditioner** per NIC
- Tags + folders + favorites (VMware has neither tags nor favorites)
- Picture-in-Picture and screen recording (VMware has neither)

#### Distribution
- Debian / Ubuntu `.deb` packaging in [`packaging/debian/`](packaging/debian/) (control, rules, postinst, install)
- Fedora / RHEL `.rpm` spec + Copr stub in [`packaging/rpm/`](packaging/rpm/)
- Arch Linux `PKGBUILD` + `.SRCINFO` in [`packaging/aur/`](packaging/aur/)
- Flatpak manifest + metainfo + cargo-sources stub in [`packaging/flatpak/`](packaging/flatpak/)
- `scripts/setup-deps.sh` and `scripts/install.sh` for from-source installs

#### Documentation
- `README.md`, `CHANGELOG.md`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `SECURITY.md`, `AUTHORS`
- `docs/VMWARE-PARITY.md` — 350-line parity matrix and differentiator catalogue
- `docs/ROADMAP.md` — 10 historical waves plus parity-driven Waves 11-15
- `docs/WINDOWS-PORT.md` — multi-quarter cross-platform strategy
- `docs/WAYLAND-COMPATIBILITY.md` — compositor matrix with contributor sign-offs
- `docs/libreuefi-guest-os-requirements.md` — firmware requirements for guests
- `docs/openapi.json` — generated OpenAPI 3.1 spec

### Architecture notes
- `vmm-types` (pure data, no I/O) extracted from `vmm-core`. 3,243 lines: 11 enums, 8 structs, plus referenced helpers. Compiles cleanly for `x86_64-pc-windows-gnu`. Foundation for the Windows port (see [`docs/WINDOWS-PORT.md`](docs/WINDOWS-PORT.md), Wave 16.A1).
- Workspace dependencies pinned: `serde 1`, `tokio 1`, `anyhow 1`, `thiserror 2`, `tracing 0.1`, `uuid 1`, `dirs 6`, `eframe`/`egui 0.30`, `axum 0.8`.

### Known issues
- Five pre-existing dead-code warnings (intentional — infrastructure for upcoming waves).
- Windows host support: `vmm-types` compiles for `x86_64-pc-windows-gnu`, but full Windows host backend (`LibvirtWhpx`) is a multi-quarter Phase B effort. See [`docs/WINDOWS-PORT.md`](docs/WINDOWS-PORT.md).
- AppImage build and signed releases are deferred to a release-pipeline wave.
- LAN segment auto-creation of the libvirt network is a follow-up (`NetworkMode::LanSegment(name)` itself ships).
- Per-VM firewall GUI editor is a follow-up (data model + libvirt nwfilter XML are shipped).
- SPICE multi-monitor is currently up to 4 heads in-guest; configuration supports 8.
- Snapshot consolidation has a partial `qemu-img commit` UI; full UX is a follow-up.

### Built with
- Rust 1.75+ (workspace `edition = "2021"`)
- QEMU 8.2+
- libvirt 10.0+
- `eframe` / `egui` 0.30
- `axum` 0.8
- LibreUEFI (EDK2 fork)

### Build notes
- `libvirt-dev` headers are required to build. The repo ships a `lib/libvirt.so` symlink and `lib/pkgconfig/libvirt.pc` workaround so the workspace builds even when distro `libvirt-dev` is unavailable. Use `./build.sh` (sets `PKG_CONFIG_PATH`) or export it manually before `cargo build`.

[unreleased]: https://github.com/LucentOpenSoftware/libre-vmm/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/LucentOpenSoftware/libre-vmm/releases/tag/v0.1.0
