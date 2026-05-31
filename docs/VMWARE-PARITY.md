# Feature Coverage Matrix — Libre VMM vs. VMware Workstation Pro 17

> **Purpose.** A side-by-side feature table to track where Libre VMM stands
> relative to a comparable commercial reference. This is documentation,
> not advocacy.
>
> **Authoritative status tracker.** When a feature ships, update its row.
> When the reference product ships a new feature, add a row.
>
> **Last revised:** 2026-05-30
> **Reference version:** VMware Workstation Pro 17.6.x
> **Libre VMM reference version:** 0.1.0 dev

---

## 1. Overview

For single-host VM management, Libre VMM covers roughly 85–90% of the
feature surface offered by VMware Workstation Pro 17. The remaining gap
is dominated by UX polish and a small number of features the project
either has not yet built or has deliberately chosen not to build (§5).

This document organises the comparison around three axes:

1. **Parity** — features both products implement, with column notes on
   the underlying technology.
2. **Coverage gaps** — features in the reference product that Libre VMM
   has not yet shipped, with roadmap pointers (§3).
3. **Capabilities outside the reference set** — features Libre VMM
   ships that have no equivalent in Workstation Pro. These are surfaced
   in the matrix rather than in a separate marketing list.

---

## 2. Feature Matrix

Legend: ✅ shipped · 🚧 partial · 📋 planned · ❌ not present · 🚫 won't ship (rationale in §5)

### 2.1 VM Creation & Templates

| Feature | VMware Pro 17 | Libre VMM | Notes |
|---|---|---|---|
| Guest OS catalog (200+) | ✅ ~200 profiles | 🚧 40+ templates | Wave 2.7 plans 500+ via Quickget |
| Easy Install (autounattend) | ✅ Windows + Linux | ✅ `unattended.rs` | cloud-init + Sysprep |
| OVF/OVA import/export | ✅ | ✅ `ova.rs` | |
| Template VMs (read-only base) | ✅ Pro only | ✅ `template_library.rs` | |
| Encrypted VM at create | ✅ Pro only | ✅ `encryption.rs` | Backed by LUKS |
| Native disk format | VMDK | qcow2 | qcow2 supports sparse, encryption, and backing-file COW |
| Disk types: thin / preallocated / split | ✅ 3 modes | ✅ qcow2 sparse + preallocated | Split-2GB not needed (no FAT32 target) |

### 2.2 Hardware Customization

| Feature | VMware Pro 17 | Libre VMM | Notes |
|---|---|---|---|
| Max vCPUs per VM | 32 | 512 (clamped) | |
| Max memory per VM | 128 GB | 1 TiB (clamped) | |
| CPU topology (sockets×cores×threads) | ✅ | ✅ `CpuTopology` | |
| Nested virtualization | ✅ VT-x/EPT to guest | ✅ KVM-native | |
| Disk controllers | IDE, SATA, SCSI, NVMe | virtio-blk, virtio-scsi, NVMe, SATA, IDE | |
| Independent-persistent / nonpersistent disks | ✅ | ✅ `DiskMode` enum | Shipped Wave 11.3 |
| Hot-add disk | ✅ SCSI/NVMe | ✅ `hotplug_disk`/`hotunplug_disk` | Shipped Wave 11.9 (libvirt `attach_device_flags`, all four buses) |
| Network adapters / NIC types | e1000, e1000e, vmxnet3, vlance | virtio, e1000e, rtl8139 | `vmxnet3` import mapped to virtio |
| Network modes | NAT, Bridged, Host-only, Custom, LAN segment | NAT, Bridged, Host-only, LAN segment | Shipped Wave 11.2 |
| USB controllers | 1.1, 2.0, 3.1 | 1.1, 2.0, 3.0 | |
| USB device passthrough | ✅ | ✅ `usb.rs` | |
| Sound | ✅ HD Audio | ✅ AC97 / ich9-intel-hda | |
| Display: up to 8 monitors | ✅ | ✅ 1–8 (`display_count`) | Shipped Wave 11.5 |
| 3D acceleration | DirectX 11 + OpenGL 4.3 | ✅ virtio-gpu-gl | virgl + VirtIO 3D |
| TPM 2.0 | ✅ Pro only | ✅ `swtpm` | |
| Virtual UEFI / Secure Boot | ✅ | ✅ LibreUEFI | |
| Full-VM encryption | ✅ Pro only | ✅ LUKS | |
| Restricted VMs (policy lock) | ✅ Pro only | ✅ `RestrictionPolicy` | Shipped Wave 11.8 (atomic save, expiration, op-allowlist) |
| Side-channel mitigation toggle | ✅ per-VM | ✅ `side_channel_mitigations` | Shipped Wave 11.4 |
| Printer (ThinPrint) | ✅ | 🚫 | Proprietary protocol; USB printer passthrough covers the use case |
| Serial / Parallel ports | ✅ | ✅ `SerialPortConfig`/`ParallelPortConfig` + UI | Shipped Wave 11.6 |

### 2.3 Guest Integration

| Feature | VMware Pro 17 | Libre VMM | Notes |
|---|---|---|---|
| Guest tools daemon | VMware Tools | `qemu-guest-agent` + `spice-vdagent` | open-vm-tools provides the cross-equivalent |
| Copy/paste (bidirectional) | ✅ | ✅ via SPICE | |
| Drag-and-drop files | ✅ | ✅ via SPICE | |
| Shared folders | HGFS | virtiofs | virtiofs is POSIX-correct and generally faster |
| Time sync | ✅ | ✅ via `qemu-ga` | |
| Multi-monitor in guest | ✅ up to 8 | 🚧 SPICE up to 4 | |
| Autofit display | ✅ | ✅ via SPICE resize | |
| Unity mode (seamless windows) | ✅ Pro only, Windows guests | 🚫 | High maintenance cost; clipboard + drag-drop cover most workflows |
| Quiesced snapshots (VSS / freeze) | ✅ | ✅ `create_snapshot_quiesced` | Shipped Wave 11.7 (RAII drop guard ensures thaw) |

### 2.4 Snapshots & Cloning

| Feature | VMware Pro 17 | Libre VMM | Notes |
|---|---|---|---|
| Snapshot manager (visual tree) | ✅ | ✅ painter-rendered tree with branches | Shipped Wave 11.1 |
| Live snapshot with memory | ✅ | ✅ | |
| AutoProtect (scheduled snapshots) | ✅ Pro only | ✅ `auto_snapshot.rs` | |
| Full clone | ✅ | ✅ `clone.rs` | |
| Linked clone | ✅ Pro only | ✅ qcow2 backing file | |
| Snapshot consolidation | manual | 🚧 | `qemu-img commit` UI planned |

### 2.5 Networking

| Feature | VMware Pro 17 | Libre VMM | Notes |
|---|---|---|---|
| Virtual Network Editor | ✅ vmnetcfg, Pro only | ✅ `network_editor.rs` | |
| DHCP server config | ✅ Pro only | ✅ libvirt managed | |
| Port forwarding GUI | ✅ Pro only | ✅ `port_forward.rs` | |
| Port forwarding presets (SSH/RDP/HTTP) | ❌ | 📋 Wave 5.2 | |
| LAN segments (isolated VM-to-VM) | ✅ Pro only | ✅ `NetworkMode::LanSegment(name)` | Shipped Wave 11.2 (auto-create libvirt network is a follow-up) |
| Bridge auto-detection | ✅ | 📋 Wave 5.3 | |
| Network conditioner (latency, loss, bandwidth) | ✅ per-NIC | ✅ `network_conditioner.rs` | |
| Per-VM firewall rules | ❌ | ✅ `FirewallRule` + libvirt nwfilter | Shipped Wave 12.5 (auto-define is a follow-up) |
| Automatic guest port forwarding | ❌ | ✅ `sync_auto_forwards` + qemu-ga listener probe | Shipped Wave 12.6 |
| SSH integration (one-click) | ❌ | 📋 Wave 5.6 | |
| Passt network backend | ❌ | 📋 Wave 5.9 | |

### 2.6 Performance & Optimization

| Feature | VMware Pro 17 | Libre VMM | Notes |
|---|---|---|---|
| CPU pinning / affinity | ✅ | ✅ `resource_limits.rs` | |
| Memory ballooning (UI) | ✅ via Tools | ✅ `balloon.rs` | |
| Memory reservations / shares | ✅ | ✅ `resource_limits.rs` | |
| Disk pre-allocation | ✅ | ✅ via `qemu-img` | |
| Disk compact (reclaim) | ✅ | ✅ `disk_manage.rs` | |
| Disk format conversion | VMDK variants only | qcow2↔raw↔vmdk↔vdi | |
| Hugepages toggle | ❌ | ✅ `hugepages` flag | |
| IO threads / iothread pinning | ❌ | ✅ `io_threads` | |
| Disk I/O throttle (IOPS, bandwidth) | ✅ | ✅ `resource_limits.rs` | |
| NIC bandwidth limit | ❌ | ✅ via libvirt bandwidth XML | |
| io_uring backend | ❌ | ✅ `iouring` flag | |

### 2.7 Advanced / Enterprise

| Feature | VMware Pro 17 | Libre VMM | Notes |
|---|---|---|---|
| Live migration (between hosts) | ❌ (vSphere only) | ✅ 4-step GUI wizard + progress + cancel | Shipped Wave 12.1 |
| Multi-host management | ❌ | 🚧 `remote_hosts.rs` | |
| vSphere/ESXi integration | ✅ Pro only, read-mostly | 🚫 closed protocol | OVA import is the supported migration path |
| GPU passthrough (vfio-pci) | ❌ (vSphere only) | ✅ `vfio.rs`, `pci.rs` | |
| Single-GPU passthrough | ❌ | ✅ 4-step wizard with hook-script generation | Shipped Wave 12.2 |
| Looking Glass integration | ❌ | ✅ `looking_glass.rs` | |
| Boot-to-firmware | ✅ | ✅ `boot_to_firmware` | |
| Containers / vctl | 🚫 deprecated upstream | 🚫 | See §5 |
| Replay debugging | 🚫 removed in WS 8 | 🚫 | |
| CLI tool | 🚧 `vmrun`, ~30 commands | ✅ `vmm-cli` (16 subcommands + shell completions) | |
| REST API | ❌ | ✅ `vmm-api` (18 routes, OpenAPI 3.1, swagger-ui, redoc) | Shipped Wave 12.4 |
| Terraform/Ansible providers | community only | 📋 Wave 14.1–14.2 (OpenAPI spec ready) | |
| Cloud-init / NoCloud datasource | partial via Easy Install | ✅ `unattended.rs` | |
| Backup with retention | ✅ Pro only, rudimentary | ✅ `backup.rs` (zstd-compressed) | |
| Audit logging | minimal | 📋 structured `tracing` logs | |

### 2.8 UX

| Feature | VMware Pro 17 | Libre VMM | Notes |
|---|---|---|---|
| VM library / inventory sidebar | ✅ | ✅ `sidebar.rs` | |
| Folders | ✅ | ✅ `folder` field | |
| Tags | ❌ | ✅ `tags` field | |
| Favorites | ❌ | ✅ `favorite` flag | |
| Recent VMs on home tab | ✅ | ✅ `home.rs` redesigned | Shipped Wave 11.10 |
| Full-screen mode | ✅ | ✅ | |
| Quick Switch (tabbed VMs) | ✅ | ✅ `tab_bar.rs` | |
| Dark mode | ✅ since 17.0 | ✅ default theme | |
| High-DPI scaling | ✅ | ✅ `ui_scale` | |
| Basic / Expert mode | ✅ via tier (Player/Pro) | 🚧 Box types (Standard / Hardware Lab / Power User) | |
| First-run setup wizard | ❌ | ✅ `views/first_run.rs` (7 steps) | Shipped Wave 13.6 |
| Wayland-native | ❌ (XWayland only) | ✅ egui + winit | See `docs/WAYLAND-COMPATIBILITY.md` |
| Screen recording (built-in) | ❌ | ✅ `screen_recording.rs` | |
| Multi-monitor host (Picture-in-Picture) | ❌ | ✅ `pip.rs` | |
| Notifications (desktop) | ❌ | ✅ `notifications.rs` | |

### 2.9 Multi-Architecture

| Feature | VMware Pro 17 | Libre VMM | Notes |
|---|---|---|---|
| Host: x86_64 Linux/Windows | ✅ | ✅ (Windows host: see Wave 16, `docs/WINDOWS-PORT.md`) | |
| Host: ARM64 Linux/Windows | ❌ (Fusion only on Apple Silicon) | ✅ via KVM-arm64 | |
| Guest: ARM64 | Fusion only (Apple Silicon) | ✅ + 22 other architectures | |
| Guest: RISC-V, MIPS, PPC, s390x, etc. | ❌ | ✅ `qemu_archs.rs` (24 architectures) | |

### 2.10 Distribution & Install

| Aspect | VMware Pro 17 | Libre VMM |
|---|---|---|
| Install method | Vendor portal, account required, `.bundle` installer script | apt / dnf / pacman / flatpak / AUR manifests in `packaging/` |
| Kernel update tolerance | Out-of-tree modules (`vmmon`, `vmnet`) — installer breaks each kernel | KVM is in-tree |
| Offline install | ❌ | ✅ |
| License clarity | tiered free/paid/commercial | GPL-3.0-or-later, single tier |

---

## 3. Coverage Gap Roadmap (Waves 11–15)

The main [ROADMAP.md](ROADMAP.md) covers research-driven feature waves (1–10).
This section adds the coverage-driven roadmap that maps directly onto §2 gaps.

### Wave 11: Coverage Polish

**Goal:** Close the P0/P1 gaps from §2.

| # | Task | Effort | Files |
|---|---|---|---|
| 11.1 | Snapshot tree visual renderer — branches, parent lines, current marker | ✅ shipped | `views/snapshots.rs` |
| 11.2 | LAN segments — isolated bridges between VMs | ✅ shipped | `network.rs`, `xml_builder.rs` |
| 11.3 | Independent-persistent / nonpersistent disk modes | ✅ shipped | `config.rs`, `xml_builder.rs` |
| 11.4 | Side-channel mitigations toggle per-VM | ✅ shipped | `config.rs`, `xml_builder.rs` |
| 11.5 | Display: bump to 8 heads | ✅ shipped | `config.rs`, virtio-gpu config |
| 11.6 | Serial / parallel port UI | ✅ shipped | `vm_settings.rs`, `xml_builder.rs` |
| 11.7 | Quiesced snapshots via qemu-ga `guest-fsfreeze-freeze/thaw` | ✅ shipped | `snapshot.rs` |
| 11.8 | Restricted VMs — policy file (read-only flags, op-allowlist, expiration) | ✅ shipped | `vmm-core/src/restricted.rs` |
| 11.9 | Hot-add disk while running | ✅ shipped | `disk_manage.rs` |
| 11.10 | Recent VMs / Home tab polish | ✅ shipped | `home.rs` |

### Wave 12: Capability Spotlight

**Goal:** Ship and document the capabilities not present in the reference product.

| # | Task | Effort | Files |
|---|---|---|---|
| 12.1 | Live migration GUI — drag a VM between remote hosts in the sidebar | ✅ shipped | 4-step wizard, sidebar "Migrate…" entry, progress bar, cancel |
| 12.2 | Single-GPU passthrough wizard — TTY switch, display-manager stop, restore | ✅ shipped | `views/single_gpu_setup.rs` |
| 12.3 | Looking Glass quick-launch from console toolbar | ✅ shipped | `console_toolbar.rs` + `action_launch_looking_glass` |
| 12.4 | REST API documentation site (OpenAPI + try-it console) | ✅ shipped | swagger-ui at `/api/v1/docs`, redoc at `/api/v1/redoc`, generated `docs/openapi.json` |
| 12.5 | Per-VM nftables firewall rules | 🚧 data model + XML shipped | `FirewallRule` + libvirt nwfilter; GUI editor is a follow-up |
| 12.6 | Automatic guest port forwarding (Lima-style detect-and-forward) | ✅ shipped | `sync_auto_forwards` + `list_guest_listeners` via qemu-ga |
| 12.7 | Declarative VM specs — export/import as TOML/YAML | ✅ shipped | `VmConfig::to_toml/to_yaml/from_toml/from_yaml/save_toml/save_yaml` |
| 12.8 | Wayland-native test matrix — verify Sway, Hyprland, GNOME, KDE | ✅ shipped | `docs/WAYLAND-COMPATIBILITY.md` |
| 12.9 | Container-native workflows — systemd-nspawn or Podman quadlet alongside VMs | 🚧 data model shipped | `vmm-core/src/container.rs` (backends stubbed for a future wave) |
| 12.10 | CLI completion — bash / zsh / fish | ✅ shipped | `vmm completions <shell>` + `install.sh` hooks |

### Wave 13: Distribution & Onboarding

**Goal:** Lower the install and first-run friction.

| # | Task | Effort | Files |
|---|---|---|---|
| 13.1 | Flatpak manifest with libvirt `qemu:///session` for sandbox | ✅ shipped | `packaging/flatpak/` (manifest + metainfo + cargo-sources stub + README) |
| 13.2 | AppImage with bundled QEMU/libvirt | 4hr | CI |
| 13.3 | Debian/Ubuntu `.deb` with proper dependencies | ✅ shipped | `packaging/debian/` |
| 13.4 | Fedora/RHEL `.rpm` + Copr repo | ✅ shipped | `packaging/rpm/` |
| 13.5 | Arch AUR package | ✅ shipped | `packaging/aur/PKGBUILD` + `.SRCINFO` + README |
| 13.6 | First-run wizard — detect KVM, libvirt, OVMF, swtpm; offer to install missing | ✅ shipped | `views/first_run.rs` + `vmm-core/src/system_check.rs` |
| 13.7 | VM discovery on first run — auto-import existing libvirt/Quickemu/VirtualBox VMs | ✅ shipped | `discover_and_group` + extended scan paths |
| 13.8 | Library import from `.vmx` files — batch-import wizard | ✅ shipped | `scan_vmware_library` + menu entry |
| 13.9 | Library import from `.vbox` files — batch-import wizard | ✅ shipped | `scan_vbox_library` + menu entry |
| 13.10 | In-app updater with signed releases | 4hr | new `vmm-core/src/update.rs` |

### Wave 14: Ecosystem & Automation

**Goal:** Integrate with the standard infrastructure-as-code toolchain.

| # | Task | Effort | Files |
|---|---|---|---|
| 14.1 | Terraform provider wrapping the REST API | 8hr | new `terraform-provider-librevmm/` |
| 14.2 | Ansible collection with `librevmm.vm.*` modules | 6hr | new `ansible_collections/` |
| 14.3 | Vagrant provider | 4hr | new `vagrant-librevmm/` |
| 14.4 | Webhook events — VM lifecycle events POST to a URL | 2hr | `vmm-api/routes.rs` |
| 14.5 | Server mode — headless daemon variant of `vmm-api` | 2hr | systemd unit + auth hardening |
| 14.6 | Prometheus metrics endpoint | 2hr | `vmm-api/metrics.rs` |
| 14.7 | OpenTelemetry tracing through `tracing` integration | 1hr | wire OTLP exporter |
| 14.8 | systemd auto-start with delay / dependency graph | 1hr | `config.rs` |

### Wave 15: Forward-Looking Features

**Goal:** Capabilities for which the project has no direct reference in the comparison product.

| # | Task | Effort | Files |
|---|---|---|---|
| 15.1 | VM signing & attestation — sign config + disk hashes, verify on boot | 6hr | new `vmm-core/src/attestation.rs` |
| 15.2 | Reproducible VM builds — declarative spec → byte-identical disk | 8hr | extends 12.7 |
| 15.3 | Bcachefs/BTRFS-native COW clones without qcow2 backing | 3hr | `clone.rs` |
| 15.4 | GPU mediated devices (mdev / SR-IOV) for non-passthrough sharing | 4hr | new `vmm-core/src/mdev.rs` |
| 15.5 | VM "instant boot" — snapshot-based < 1s cold boot | 4hr | leverage RAM snapshots |
| 15.6 | AI workload preset — virtio-vfio for NVIDIA/AMD with verified CUDA passthrough | 4hr | new template + VFIO presets |
| 15.7 | Confidential VMs (AMD SEV-SNP, Intel TDX) | 8hr | `xml_builder.rs` + LibreUEFI |
| 15.8 | Distributed compute fabric — VMs across multiple Libre VMM hosts, auto-rebalance | large | `migration.rs` + scheduler |

---

## 4. Design Decisions: Deliberately Omitted Features

Features present in the reference product that Libre VMM has chosen not
to implement, with the reason recorded so the decision is reviewable.

| Feature | Rationale |
|---|---|
| Unity mode (seamless windows) | Large, ongoing maintenance burden tied to host-window-manager internals and guest-OS APIs. The combination of SPICE clipboard, drag-drop, and shared folders covers the majority of the workflow it enables. |
| `vctl` / built-in container engine | Deprecated upstream. Containers are well served by Podman and Docker; embedding a separate engine inside a VM manager has not historically been a productive investment. |
| ThinPrint | Proprietary protocol with limited adoption. USB printer passthrough handles the same use case using open device emulation. |
| VMDK monolithic-sparse variants | qcow2 covers the same use case (sparse allocation, snapshots, encryption). VMDK is supported as an import format for migrating existing VMs. |
| Replay debugging | Removed from the reference product in Workstation 8. The investment cost would not pay back. |
| VM teams | Removed from the reference product. Folders, tags, and LAN segments cover the organisational and topology use cases. |
| Workstation Server / Shared VMs | Removed from the reference product. The Libre VMM REST API serves the same need. |
| Proprietary guest tools | open-vm-tools, qemu-guest-agent, and spice-vdagent provide equivalent functionality under open licences and ship in distribution repositories. |
| Closed unattended-install format | Cloud-init and `autounattend.xml` are open standards already widely supported by guest operating systems. |
| vSphere/ESXi protocol integration | Proprietary protocol. OVA import is the supported migration path; the project does not implement closed network protocols. |

---

## 5. Architectural Commitments

These decisions shape day-to-day design choices and are recorded so new
contributors can evaluate proposed changes against them.

1. **libvirt is the hypervisor abstraction boundary.** Adding Xen,
   Cloud Hypervisor, or Firecracker support — should we ever need it —
   should be possible by adding a libvirt driver, not by introducing a
   second parallel abstraction inside Libre VMM.

2. **QEMU is the device-emulation substrate.** We don't reinvent device
   models. Engineering effort goes into UX, packaging, and
   integration, not into competing with QEMU on emulation correctness.

3. **VirtIO is the device default.** Legacy device models (e1000, IDE)
   are emitted only when a guest OS requires them (old Windows, retro
   operating systems).

4. **LibreUEFI is the firmware default.** Our EDK2 fork lets us add
   features (battery and thermal ACPI, fw_cfg bridge, branding) that
   generic OVMF builds don't expose. Generic OVMF remains a supported
   alternative for users who prefer it.

5. **GPL-3.0-or-later for the core.** Permissive-licensed components
   are acceptable as upstream dependencies, but new first-party code is
   GPL-3.0-or-later so the project as a whole remains copyleft.

6. **GUI and API are separate binaries.** `vmm-api` runs headless on
   servers; `vmm-gui` is the desktop client. Either can be deployed
   independently, and the same client can talk to multiple servers.

7. **No telemetry, no phone-home, no signed-mandatory updates.** The
   project commits to reproducible builds and to never collecting usage
   data from running installs.

8. **One opinionated default per choice.** When the system has to pick
   between, e.g., audio backends, we default to one (PipeWire) and
   document the rest. Advanced settings are available but not surfaced
   by default.

---

## 6. Open Questions

These should be resolved before the next wave of work begins. Each is a
GitHub Discussion candidate.

1. **Tag system unification.** The current model has `tags`, `folder`,
   `favorite`, and `box_type`. Four organisational axes may be too
   many; some may consolidate or move under `tags`.

2. **Headless server packaging.** Should `vmm-api` ship as a separate
   package (`librevmm-server`), or always alongside the GUI? Trade-off
   is install footprint vs. installation simplicity.

3. **Container engine surface area.** Wave 12.9 shipped a container
   data model. Whether to add real backends (nspawn / Podman / Docker)
   inside Libre VMM, or to point users at Distrobox / Toolbox, is open.

4. **Mobile companion app.** A small iOS/Android app speaking to
   `vmm-api` for power operations may be useful, or may be scope creep.

5. **Web GUI alternative.** Some operators would prefer a browser UI on
   top of `vmm-api`. Project-built, or community-developed?

6. **Plugin API.** Third-party plugins (custom OS catalog entries,
   custom backup backends) — desirable, but only after the public API
   surface stabilises.

---

## 7. How to Use This Document

- **Contributors:** Pick a `📋 planned` row from §3, open an issue
  referencing the cell, and ship.
- **Users:** §2 is the honest coverage list. §4 explains the
  deliberate omissions. §5 sets context for what kinds of changes
  the project will accept.
- **Maintainers:** Update the matrix on every release. When the
  reference product publishes a new feature, add a row.

---

*The tactical work breakdown lives in [ROADMAP.md](ROADMAP.md). When
this document and ROADMAP.md disagree about a feature's intent or
status, treat this one as canonical and file an issue to reconcile.*
