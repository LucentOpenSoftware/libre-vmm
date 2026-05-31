# Libre VMM vs VMware Workstation Pro 17 — Parity & Future Vision

> **Purpose:** This document defines what Libre VMM is, what it must become to be a real VMware Workstation replacement, and — more importantly — what it should become to make VMware look obsolete.
>
> **Authoritative status tracker.** When a feature ships, mark its row. When VMware ships something new, add a row.
>
> **Last revised:** 2026-05-30
> **VMware reference version:** Workstation Pro 17.6.x (free for personal & commercial use since Nov 2024, distributed via Broadcom Support Portal)
> **Libre VMM reference version:** 0.1.0 dev

---

## 1. Executive Summary

Libre VMM is already at **~85-90% feature parity** with VMware Workstation Pro for single-host VM management. The remaining 10-15% is mostly **UX polish, packaging, and a small number of differentiator features** — not foundational engineering gaps.

VMware's strategic vulnerability is everything **around** the product: Broadcom's distribution UX is hostile, the Linux installer rots with every kernel update, there is no first-class CLI/API, qcow2 isn't supported, GPU passthrough is locked behind vSphere, and Wayland is broken. **Every one of these is a free win for a libre alternative built on QEMU/libvirt.**

This document is organized around three commitments:

1. **Parity** — match what VMware does well, on better foundations (qcow2, virtiofs, libvirt, KVM, OVMF).
2. **Differentiation** — ship the things VMware structurally cannot (live migration, GPU passthrough, declarative configs, REST API, Wayland-native, kernel-version-independent).
3. **Discipline** — refuse to copy VMware's mistakes (Unity mode, vctl, ThinPrint, proprietary disk formats, opaque licensing).

---

## 2. Feature Parity Matrix

Legend: ✅ shipped · 🚧 partial · 📋 planned · ❌ not started · 🚫 won't do (rationale in §5)

### 2.1 VM Creation & Templates

| Feature | VMware Pro 17 | Libre VMM | Notes |
|---|---|---|---|
| Guest OS catalog (200+) | ✅ ~200 profiles | 🚧 40+ templates | Wave 2.7 plans 500+ via Quickget |
| Easy Install (autounattend) | ✅ Windows + Linux | ✅ `unattended.rs` | cloud-init + Sysprep |
| OVF/OVA import/export | ✅ | ✅ `ova.rs` | |
| Template VMs (read-only base) | ✅ Pro only | ✅ `template_library.rs` | |
| Encrypted VM at create | ✅ Pro only | ✅ `encryption.rs` | LUKS > AES-256-XTS proprietary |
| **Native disk format** | VMDK only | **qcow2 (superior)** | Differentiator |
| Disk types: thin / preallocated / split | ✅ 3 modes | ✅ qcow2 sparse + preallocated | Split-2GB not needed (no FAT32) |

### 2.2 Hardware Customization

| Feature | VMware Pro 17 | Libre VMM | Notes |
|---|---|---|---|
| Max vCPUs per VM | 32 | 512 (clamped) | We win |
| Max memory per VM | 128 GB | 1 TiB (clamped) | We win |
| CPU topology (sockets×cores×threads) | ✅ | ✅ `CpuTopology` | |
| Nested virtualization | ✅ VT-x/EPT to guest | ✅ KVM-native | |
| Disk controllers | IDE, SATA, SCSI, NVMe | virtio-blk, virtio-scsi, NVMe, SATA, IDE | virtio > VMware paravirtual |
| Independent-persistent / nonpersistent disks | ✅ | ✅ `DiskMode` enum | Shipped Wave 11.3 |
| Hot-add disk | ✅ SCSI/NVMe | ✅ `hotplug_disk`/`hotunplug_disk` | Shipped Wave 11.9 (libvirt attach_device_flags, all 4 buses) |
| Network adapters / NIC types | e1000, e1000e, vmxnet3, vlance | virtio, e1000e, rtl8139 | vmxnet3 mapped to virtio |
| Network modes | NAT, Bridged, Host-only, Custom, LAN segment | NAT, Bridged, Host-only | **P1 GAP** — LAN segments (isolated VM-to-VM) |
| USB controllers | 1.1, 2.0, 3.1 | 1.1, 2.0, 3.0 | |
| USB device passthrough | ✅ | ✅ `usb.rs` | |
| Sound | ✅ HD Audio | ✅ AC97/ich9-intel-hda | |
| Display: up to 8 monitors | ✅ | ✅ 1-8 (`display_count`) | Shipped Wave 11.5 |
| 3D acceleration | DX11 + OpenGL 4.3 | ✅ virtio-gpu-gl | virgl + VirtIO 3D |
| TPM 2.0 | ✅ Pro only | ✅ swtpm | |
| Virtual UEFI / Secure Boot | ✅ | ✅ LibreUEFI | LibreUEFI advantage |
| Full-VM encryption | ✅ Pro only | ✅ LUKS | |
| Restricted VMs (policy lock) | ✅ Pro only | ✅ `RestrictionPolicy` | Shipped Wave 11.8 (atomic save, expiration, op-allowlist) |
| Side-channel mitigation toggle | ✅ per-VM | ✅ `side_channel_mitigations` | Shipped Wave 11.4 |
| Printer (ThinPrint) | ✅ | 🚫 | Proprietary, never |
| Serial / Parallel ports | ✅ | ✅ `SerialPortConfig`/`ParallelPortConfig` + UI | Shipped Wave 11.6 |

### 2.3 Guest Integration

| Feature | VMware Pro 17 | Libre VMM | Notes |
|---|---|---|---|
| Guest tools daemon | VMware Tools | qemu-guest-agent + spice-vdagent | open-vm-tools equivalent is rich |
| Copy/paste (bidirectional) | ✅ | ✅ SPICE | |
| Drag-and-drop files | ✅ | ✅ SPICE | |
| Shared folders | HGFS | **virtiofs** | virtiofs >> HGFS (POSIX-correct, faster) |
| Time sync | ✅ | ✅ qemu-ga | |
| Multi-monitor in guest | ✅ up to 8 | 🚧 SPICE up to 4 | |
| Autofit display | ✅ | ✅ SPICE resize | |
| **Unity mode** (seamless windows) | ✅ Pro only, Windows guests | 🚫 | Massive maintenance burden, broken even at VMware |
| Quiesced snapshots (VSS / freeze) | ✅ | ✅ `create_snapshot_quiesced` | Shipped Wave 11.7 (drop-guard guarantees thaw) |

### 2.4 Snapshots & Cloning

| Feature | VMware Pro 17 | Libre VMM | Notes |
|---|---|---|---|
| Snapshot manager (visual tree) | ✅ | ✅ painter-rendered tree with branches | Shipped Wave 11.1 |
| Live snapshot with memory | ✅ | ✅ | |
| AutoProtect (scheduled snapshots) | ✅ Pro only | ✅ `auto_snapshot.rs` | |
| Full clone | ✅ | ✅ `clone.rs` | |
| Linked clone | ✅ Pro only | ✅ qcow2 backing file | |
| Snapshot consolidation | manual, painful | 🚧 | Add `qemu-img commit` UI |

### 2.5 Networking

| Feature | VMware Pro 17 | Libre VMM | Notes |
|---|---|---|---|
| Virtual Network Editor | ✅ vmnetcfg, Pro only | ✅ `network_editor.rs` | |
| DHCP server config | ✅ Pro only | ✅ libvirt managed | |
| Port forwarding GUI | ✅ Pro only | ✅ `port_forward.rs` | |
| Port forwarding presets (SSH/RDP/HTTP) | ❌ | 📋 Wave 5.2 | We win |
| LAN segments (isolated VM-to-VM) | ✅ Pro only | ✅ `NetworkMode::LanSegment(name)` | Shipped Wave 11.2 (auto-create libvirt network is follow-up) |
| Bridge auto-detection | ✅ | 📋 Wave 5.3 | |
| **Network conditioner** (latency, loss, bandwidth) | ✅ per-NIC | ✅ `network_conditioner.rs` | |
| Per-VM firewall rules | ❌ VMware can't | ✅ `FirewallRule` + libvirt nwfilter | Shipped Wave 12.5 (auto-define is follow-up) |
| Automatic guest port forwarding | ❌ VMware can't | ✅ `sync_auto_forwards` + qemu-ga listener probe | Shipped Wave 12.6 |
| SSH integration (one-click) | ❌ VMware can't | 📋 Wave 5.6 | Differentiator |
| Passt network backend | ❌ VMware can't | 📋 Wave 5.9 | Differentiator |

### 2.6 Performance & Optimization

| Feature | VMware Pro 17 | Libre VMM | Notes |
|---|---|---|---|
| CPU pinning / affinity | ✅ | ✅ `resource_limits.rs` | |
| Memory ballooning (UI) | ✅ via Tools | ✅ `balloon.rs` | |
| Memory reservations / shares | ✅ | ✅ `resource_limits.rs` | |
| Disk pre-allocation | ✅ | ✅ qemu-img | |
| Disk compact (reclaim) | ✅ | ✅ `disk_manage.rs` | |
| Disk format conversion | VMDK variants only | ✅ qcow2↔raw↔vmdk↔vdi | We win |
| Hugepages toggle | ❌ | ✅ `hugepages` flag | |
| IO threads / iothread pinning | ❌ | ✅ `io_threads` | We win |
| Disk I/O throttle (IOPS, bandwidth) | ✅ | ✅ `resource_limits.rs` | |
| NIC bandwidth limit | ❌ | ✅ network bandwidth XML | We win |
| io_uring backend | ❌ | ✅ `iouring` flag | We win |

### 2.7 Advanced / Enterprise

| Feature | VMware Pro 17 | Libre VMM | Notes |
|---|---|---|---|
| **Live migration** (between hosts) | ❌ Pro can't (vSphere only) | ✅ 4-step GUI wizard + progress + cancel | **Massive differentiator** — GUI shipped Wave 12.1 |
| Multi-host management | ❌ | 🚧 `remote_hosts.rs` | |
| vSphere/ESXi integration | ✅ Pro only, read-mostly | 🚫 closed protocol | |
| **GPU passthrough (vfio-pci)** | ❌ vSphere only | ✅ `vfio.rs`, `pci.rs` | **Massive differentiator** |
| **Single-GPU passthrough** | ❌ | ✅ 4-step wizard with hook script gen | Shipped Wave 12.2 |
| **Looking Glass** integration | ❌ | ✅ `looking_glass.rs` | Differentiator |
| Boot-to-firmware | ✅ | ✅ `boot_to_firmware` | |
| Containers / vctl | 🚫 deprecated by VMware | 🚫 | Skip — failed bet |
| Replay debugging | 🚫 removed long ago | 🚫 | Never |
| `vmrun` CLI (limited) | 🚧 ~30 commands | ✅ `vmm-cli` (16 subcommands) | We have parity, growing |
| **REST API** | ❌ | ✅ `vmm-api` (18 routes, OpenAPI 3.1, swagger-ui, redoc) | **Massive differentiator** — Shipped Wave 12.4 |
| Terraform/Ansible providers | community only | 📋 Wave 14.1-14.2 (OpenAPI spec ready) | |
| Cloud-init / NoCloud datasource | partial via Easy Install | ✅ `unattended.rs` | We win |
| Backup with retention | ✅ Pro only, rudimentary | ✅ `backup.rs` zstd-compressed | We win |
| Audit logging | minimal | 📋 structured `tracing` logs | |

### 2.8 UX

| Feature | VMware Pro 17 | Libre VMM | Notes |
|---|---|---|---|
| VM library / inventory sidebar | ✅ | ✅ `sidebar.rs` | |
| Folders | ✅ | ✅ `folder` field | |
| Tags | ❌ | ✅ `tags` field | We win |
| Favorites | ❌ | ✅ `favorite` flag | We win |
| Recent VMs on home tab | ✅ | ✅ `home.rs` redesigned (quick actions, recent grid, error banner) | Shipped Wave 11.10 |
| Full-screen mode | ✅ | ✅ | |
| Quick Switch (tabbed VMs) | ✅ | ✅ `tab_bar.rs` | |
| Dark mode | ✅ since 17.0 | ✅ default theme | |
| High-DPI scaling | ✅ | ✅ ui_scale | |
| Basic / Expert mode | ✅ via tier (Player/Pro) | 🚧 Box types (Standard/Hardware Lab/Power User) | We did it better |
| First-run setup wizard | ❌ | 📋 Wave 9.1 | |
| **Wayland-native** | ❌ (XWayland only, glitchy) | ✅ egui + winit + `docs/WAYLAND-COMPATIBILITY.md` | **Differentiator** |
| Screen recording (built-in) | ❌ | ✅ `screen_recording.rs` | We win |
| Multi-monitor host (PiP) | ❌ | ✅ `pip.rs` Picture-in-Picture | We win |
| Notifications (desktop) | ❌ | ✅ `notifications.rs` | We win |

### 2.9 Multi-Architecture

| Feature | VMware Pro 17 | Libre VMM | Notes |
|---|---|---|---|
| Host: x86_64 Linux/Windows | ✅ | ✅ | |
| Host: ARM64 Linux/Windows | ❌ (Fusion-only) | ✅ (KVM-arm64) | **Differentiator** |
| Guest: ARM64 | Fusion only (Apple Silicon) | ✅ + 22 more | **Massive differentiator** |
| Guest: RISC-V, MIPS, PPC, s390x, etc. | ❌ | ✅ `qemu_archs.rs` (24 archs) | **Massive differentiator** |

### 2.10 Distribution / Install (the silent killer)

| Aspect | VMware Pro 17 | Libre VMM (target) |
|---|---|---|
| Install method | Broadcom Portal, account, MFA, `.bundle` script | ✅ apt/dnf/pacman/flatpak/AUR manifests all in `packaging/` |
| Kernel update breaks install | ✅ every time (vmmon/vmnet OOT modules) | 🚫 KVM is in-tree |
| Offline install | ❌ | ✅ |
| License clarity | confusing (free/paid/commercial flux) | GPL-3.0-or-later, no question |
| Distribution UX | hostile | 📋 critical Wave 11 priority |

---

## 3. Gap Closure Roadmap (Waves 11-15)

The existing ROADMAP.md (Waves 1-10) covers research-driven feature work. This section adds a **parity-driven** roadmap focused specifically on VMware-killer features.

### Wave 11: Parity Polish (the last 10%)
**Goal:** Close every P0/P1 gap from §2. Make a VMware user feel at home.

| # | Task | Effort | Files |
|---|---|---|---|
| 11.1 | **Snapshot tree visual renderer** — branches, parent lines, current marker | 3hr | `views/snapshots.rs` |
| 11.2 | **LAN segments** — isolated bridges between VMs | 2hr | `network.rs`, `xml_builder.rs` |
| 11.3 | **Independent-persistent / nonpersistent disk modes** | 2hr | `config.rs`, `xml_builder.rs` (qcow2 `transient` or backing-rebase on shutdown) |
| 11.4 | **Side-channel mitigations toggle** per-VM | 30min | `config.rs`, `xml_builder.rs` (`l1d_flush=on/off`) |
| 11.5 | **Display: bump to 8 heads** | 30min | `config.rs` (display_count clamp), virtio-gpu config |
| 11.6 | **Serial/parallel port UI** | 1hr | `vm_settings.rs`, `xml_builder.rs` |
| 11.7 | **Quiesced snapshots** via qemu-ga `guest-fsfreeze-freeze/thaw` | 1hr | `snapshot.rs` |
| 11.8 | **Restricted VMs** — policy file (read-only flags, USB block, expiration date) | 3hr | new `vmm-core/src/restricted.rs` |
| 11.9 | **Hot-add disk while running** via QMP | 2hr | `disk_manage.rs`, QMP integration |
| 11.10 | **Recent VMs / Home tab polish** | 1hr | `home.rs` |

### Wave 12: The Differentiator Showcase
**Goal:** Make the features VMware can't ship the headline experience. Document them prominently.

| # | Task | Effort | Files |
|---|---|---|---|
| 12.1 | **Live migration GUI** — drag VM between remote hosts in sidebar | ✅ shipped | 4-step wizard, sidebar "Migrate…" entry, progress bar, cancel |
| 12.2 | **Single-GPU passthrough wizard** — TTY switch, display-manager stop, restore | ✅ shipped | `views/single_gpu_setup.rs` 4-step wizard with hook script generation + sudoers helper |
| 12.3 | **Looking Glass quick-launch** from console toolbar | ✅ shipped | `console_toolbar.rs` + `action_launch_looking_glass` |
| 12.4 | **REST API documentation site** (OpenAPI + try-it console) | ✅ shipped | swagger-ui at `/api/v1/docs`, redoc at `/api/v1/redoc`, generated `docs/openapi.json` |
| 12.5 | **Per-VM nftables firewall rules** | 🚧 data model + XML shipped | `FirewallRule` + libvirt nwfilter; GUI editor is follow-up |
| 12.6 | **Automatic guest port forwarding** (Lima-style detect-and-forward) | ✅ shipped | `sync_auto_forwards` + `list_guest_listeners` via qemu-ga |
| 12.7 | **Declarative VM specs** — export/import as TOML/YAML, diffable | ✅ shipped | `VmConfig::to_toml/to_yaml/from_toml/from_yaml/save_toml/save_yaml` |
| 12.8 | **Wayland-native test matrix** — verify Sway, Hyprland, GNOME, KDE | ✅ shipped | `docs/WAYLAND-COMPATIBILITY.md` (275 lines, contributor sign-off table) |
| 12.9 | **Container-native workflows** — systemd-nspawn or podman quadlet alongside VMs | 🚧 data model shipped | `vmm-core/src/container.rs` (backends stubbed for future wave) |
| 12.10 | **CLI completion** — bash/zsh/fish | ✅ shipped | `vmm completions <shell>` + install.sh hooks |

### Wave 13: Distribution & Onboarding (the Broadcom-killer)
**Goal:** Make install painless. This is where users actually choose us over VMware.

| # | Task | Effort | Files |
|---|---|---|---|
| 13.1 | **Flatpak manifest** with libvirt qemu:///session for sandbox | ✅ shipped | `packaging/flatpak/` (manifest + metainfo + cargo-sources stub + README) |
| 13.2 | **AppImage** with bundled qemu/libvirt | 4hr | CI |
| 13.3 | **Debian/Ubuntu .deb** with proper dependencies | ✅ shipped | `packaging/debian/` (control + rules + postinst + 5 more) |
| 13.4 | **Fedora/RHEL .rpm** + Copr repo | ✅ shipped | `packaging/rpm/` (spec + Copr stub + README) |
| 13.5 | **Arch AUR** package | ✅ shipped | `packaging/aur/PKGBUILD` + `.SRCINFO` + README |
| 13.6 | **First-run wizard** — detect KVM, libvirt, OVMF, swtpm; offer to install missing | ✅ shipped | `views/first_run.rs` (837 lines, 7-step wizard) + `vmm-core/src/system_check.rs` (520 lines) |
| 13.7 | **VM discovery on first run** — auto-import existing libvirt/Quickemu/VirtualBox VMs | ✅ shipped | `discover_and_group` + extended scan paths (Quickemu, snap, `/var/lib/libvirt`) |
| 13.8 | **Migration wizard from VMware** — point at `.vmx` library, batch-import | ✅ shipped | `scan_vmware_library` + menu entry "Import from VMware Library..." |
| 13.9 | **Migration wizard from VirtualBox** — point at `~/VirtualBox VMs/`, batch-import | ✅ shipped | `scan_vbox_library` + menu entry "Import from VirtualBox Library..." |
| 13.10 | **In-app updater** with signed releases | 4hr | new `vmm-core/src/update.rs` |

### Wave 14: Ecosystem & Automation
**Goal:** Make Libre VMM the choice for ops/dev/devops. VMware never owned this.

| # | Task | Effort | Files |
|---|---|---|---|
| 14.1 | **Terraform provider** wrapping REST API | 8hr | new `terraform-provider-librevmm/` |
| 14.2 | **Ansible collection** with `librevmm.vm.*` modules | 6hr | new `ansible_collections/` |
| 14.3 | **Vagrant provider** | 4hr | new `vagrant-librevmm/` |
| 14.4 | **Webhook events** — VM lifecycle events POST to URL | 2hr | `vmm-api/routes.rs` |
| 14.5 | **Server mode** — headless daemon variant of vmm-api | 2hr | systemd unit + auth hardening |
| 14.6 | **Prometheus metrics endpoint** | 2hr | `vmm-api/metrics.rs` |
| 14.7 | **OpenTelemetry tracing** through `tracing` integration | 1hr | wire OTLP exporter |
| 14.8 | **systemd auto-start with delay/dependency graph** | 1hr | `config.rs` |

### Wave 15: The Future (post-VMware-parity)
**Goal:** Lead the category. Features VMware will never build.

| # | Task | Effort | Files |
|---|---|---|---|
| 15.1 | **VM signing & attestation** — sign config + disk hashes, verify on boot | 6hr | new `vmm-core/src/attestation.rs` |
| 15.2 | **Reproducible VM builds** — declarative spec → byte-identical disk | 8hr | extends 12.7 |
| 15.3 | **Bcachefs/BTRFS-native COW clones** without qcow2 backing | 3hr | `clone.rs` |
| 15.4 | **GPU mediated devices** (mdev / SR-IOV) for non-passthrough sharing | 4hr | new `vmm-core/src/mdev.rs` |
| 15.5 | **VM "instant boot"** — snapshot-based < 1s cold boot | 4hr | leverage RAM snapshots |
| 15.6 | **AI workload preset** — virtio-vfio for NVIDIA/AMD with CUDA passthrough verified | 4hr | new template + VFIO presets |
| 15.7 | **Confidential VMs** (AMD SEV-SNP, Intel TDX) | 8hr | `xml_builder.rs` + LibreUEFI |
| 15.8 | **Distributed compute fabric** — VMs across multiple Libre VMM hosts, auto-rebalance | huge | `migration.rs` + scheduler |

---

## 4. Differentiators We Lead With

These are the answers to "why not VMware?":

1. **Live migration** — VMware can't (vSphere only). We can.
2. **GPU passthrough** — VMware can't on Workstation. We have it, and Looking Glass.
3. **24 guest architectures** — VMware does x86. We do ARM, RISC-V, MIPS, PPC, s390x, ...
4. **REST API + Terraform + Ansible + Vagrant** — VMware has `vmrun`.
5. **qcow2** — sparse, snapshottable, encryptable, backing-file COW. VMDK can't match it.
6. **Cloud-init / autounattend** built-in — VMware's Easy Install is closed and rigid.
7. **Wayland-native** — VMware is XWayland-only and glitchy.
8. **Free, GPL-3.0** — no Broadcom portal, no MFA, no commercial-license question.
9. **Kernel-version-independent** — KVM is in-tree. VMware breaks every kernel update.
10. **Per-VM nftables firewall** — VMware has nothing comparable.
11. **Declarative config (TOML/YAML diffable)** — VMware's `.vmx` is opaque.
12. **CLI-first ergonomics** — VMware's `vmrun` is a tacked-on afterthought.
13. **Network conditioner across all NICs and per-VM** — VMware has it but it's hidden in advanced settings.
14. **LibreUEFI** — own firmware with branding, battery/thermal ACPI, fw_cfg bridge. VMware uses generic Intel UEFI.

---

## 5. What We Will NOT Build (and Why)

These are tempting because VMware ships them. They are traps.

| Anti-Feature | Why Skip |
|---|---|
| **Unity mode** (seamless windows) | Massive maintenance burden, breaks on every Windows update, broken even at VMware. SPICE clipboard + drag-drop covers 90% of the use case. |
| **vctl / built-in containers** | VMware deprecated it. Containers belong in Podman/Docker, not a VM manager. |
| **ThinPrint** | Proprietary, drives no adoption. Native USB printer passthrough is enough. |
| **VMDK monolithic-sparse variants** | qcow2 is strictly better. We import VMDK for migration only. |
| **Replay debugging** | VMware removed it in WS 8. Was a dead end. |
| **VM teams** | VMware removed it. Folders + LAN segments cover the use case. |
| **Workstation Server / Shared VMs** | VMware removed it. Our REST API is the modern replacement. |
| **Proprietary guest tools** | open-vm-tools / qemu-guest-agent / spice-vdagent are free, sufficient, distro-packaged. |
| **Easy Install closed format** | We use cloud-init + autounattend.xml — open, hackable, future-proof. |
| **vSphere lock-in** | We integrate with libvirt remotes. Anyone running ESXi can convert via OVA. |

---

## 6. Strategic Bets

These aren't features — they're commitments that shape every decision.

1. **libvirt as the integration boundary.** Stay with libvirt as our hypervisor abstraction. The day someone wants Xen, Cloud Hypervisor, or Firecracker support, libvirt is the seam. This is why VMware's standalone-engine bet was a strategic mistake.

2. **QEMU as the substrate.** Don't reinvent a hypervisor. QEMU has 30 years of device emulation. Compete on UX and packaging, not on emulation correctness.

3. **VirtIO everywhere.** Default to virtio devices for everything that supports them. Fall back to legacy (e1000, IDE) only for guests that need it (old Windows, retro OSes).

4. **LibreUEFI as the firmware monoculture.** Our own EDK2 fork lets us add features (battery reporting, fw_cfg bridge, custom branding) that generic OVMF can't. This is a moat.

5. **GPL-3.0-or-later.** No Apache, no MIT for the core. We never want a Broadcom moment.

6. **Single-binary GUI + separate API daemon.** Don't bundle a daemon in the GUI. `vmm-api` runs headless on servers; `vmm-gui` is the desktop client. This is how we get from "Workstation" to "Workstation + Server" without rearchitecting.

7. **Refuse cloud lock-in.** No telemetry, no signed-mandatory updates, no phone-home. Reproducible builds.

8. **One opinionated default per choice.** Don't ship 14 audio backends with no recommendation. Default to PipeWire, document the rest. VMware drowns users in advanced settings.

---

## 7. Open Questions

These need decisions before the next wave starts. Discuss in issues:

1. **Tag system unification:** We have `tags`, `folder`, `favorite`, `box_type`. Is that 4 axes of organization too many?
2. **Headless server story:** Should `vmm-api` ship as a separate package (`librevmm-server`), or always alongside the GUI?
3. **Container support:** Containers in the same UI as VMs (Lima/Multipass model) — yes or no? Wave 12.9 says yes; skeptics say it dilutes focus.
4. **Mobile companion app:** A simple iOS/Android app talking to `vmm-api` for power-ops. Worth it, or scope creep?
5. **Web GUI alternative:** Some users want a browser UI on top of `vmm-api`. Build it, or leave to community?
6. **Plugin API:** Allow third-party plugins (e.g., custom OS catalog entries, custom backup backends)? When?

---

## 8. How to Use This Document

- **For contributors:** Pick a `📋 planned` row, open an issue referencing the cell, ship.
- **For users:** §4 is your sales pitch. §2 is your honest gap list.
- **For maintainers:** Every release, update the column. Every VMware release, audit §2 for new VMware features and add rows.
- **For VMware refugees:** Start with §3 Wave 13 (Distribution) and §4 (Differentiators). If both look credible, you'll never miss Broadcom Portal.

---

*This document is the strategic north star. The tactical work lives in [ROADMAP.md](ROADMAP.md). When the two disagree, this one wins.*
