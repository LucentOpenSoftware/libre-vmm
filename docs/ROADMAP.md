# Libre VMM — Implementation Roadmap

> Researched from: OSX-KVM, vm-curator, Lima, Vectras VM, Quickgui/Quickemu, VMware Unlocker (DrDonk)
> Created: 2026-03-29

---

## Wave 1: Core Polish & macOS Support (Current Sprint)
**Goal:** Fix existing issues, add macOS VM support, clean up SPICE

| # | Task | Source | Effort | Files |
|---|------|--------|--------|-------|
| 1.1 | Remove all `[SPICE-DBG]` eprintln debug lines | Internal | 30min | `spice.rs` |
| 1.2 | Guard `spice_main_send_monitor_config` with agent check | Internal | 15min | `spice.rs` |
| 1.3 | Skip `spice_channel_connect()` for USB redirect channels | Internal | 15min | `spice.rs` |
| 1.4 | Fix CD/DVD `change-media` (eject before insert) | Internal | 30min | `media_dialog.rs` |
| 1.5 | **macOS VM support** — Apple SMC device (OSK key from Unlocker), Penryn CPU, vmware-svga, vmxnet3 | OSX-KVM + vm-curator + Unlocker | 3hr | `xml_builder.rs`, `config.rs`, `template_library.rs` |
| 1.6 | macOS wizard template (Sierra → Sequoia profiles) | vm-curator profiles | 1hr | `template_library.rs`, `wizard.rs` |
| 1.7 | OpenCore bootloader ISO integration (download/select) | OSX-KVM | 1hr | `config.rs`, `vm_settings.rs` |
| 1.8 | `ignore_msrs` kernel param check + warning dialog | OSX-KVM | 30min | `app.rs` or new `system_check.rs` |
| 1.9 | Display protocol dropdown default to VNC while SPICE stabilizes | Internal | 15min | `config.rs` |
| 1.10 | **macOS VMware Tools ISO** bundling (darwin.iso from Unlocker) | Unlocker | 30min | guest tools |

---

## Wave 2: VM Import & OS Download Wizard
**Goal:** Import existing VMs from other managers + download ISOs directly

| # | Task | Source | Effort | Files |
|---|------|--------|--------|-------|
| 2.1 | **VM Import Wizard** — libvirt XML parser | vm-curator `import.rs` | 3hr | new `vmm-core/src/import.rs`, `vmm-gui/src/views/import_wizard.rs` |
| 2.2 | Quickemu `.conf` import | vm-curator | 1hr | `import.rs` |
| 2.3 | VMware `.vmx` import | New | 2hr | `import.rs` |
| 2.4 | VirtualBox `.vbox` import | New | 2hr | `import.rs` |
| 2.5 | Disk handling options (symlink, copy, move, convert) | vm-curator | 1hr | `import.rs`, `disk.rs` |
| 2.6 | **OS Download Wizard** — searchable OS list with icons | Quickgui | 3hr | new `views/os_download.rs` |
| 2.7 | OS catalog (500+ entries from quickget) | Quickgui/Quickemu | 2hr | new `vmm-core/src/os_catalog.rs` |
| 2.8 | ISO download with progress bar + desktop notification | Quickgui | 2hr | `os_download.rs` |
| 2.9 | Version/edition selection (e.g., Ubuntu → 24.04 → Desktop/Server) | Quickgui | 1hr | `os_download.rs` |
| 2.10 | Auto-configure VM from downloaded OS profile | vm-curator + Quickgui | 1hr | `template_library.rs` |

---

## Wave 3: GPU Passthrough & 3D Acceleration
**Goal:** GPU passthrough (single + multi), Looking Glass, 3dfx retro

| # | Task | Source | Effort | Files |
|---|------|--------|--------|-------|
| 3.1 | **IOMMU group scanner** — detect PCI devices + groups | vm-curator `pci.rs` | 2hr | new `vmm-core/src/hardware/pci.rs` |
| 3.2 | **PCI passthrough UI** — select GPUs, NVMe, USB controllers | vm-curator | 3hr | new `views/pci_passthrough.rs` |
| 3.3 | VFIO bind/unbind scripts (auto-bind before launch, restore after) | vm-curator `single_gpu.rs` | 2hr | new `vmm-core/src/hardware/vfio.rs` |
| 3.4 | **Single-GPU passthrough** — stop display manager, TTY switch | vm-curator | 3hr | `vfio.rs`, new `views/single_gpu_setup.rs` |
| 3.5 | **Multi-GPU passthrough** — secondary GPU to VM | vm-curator | 2hr | new `views/multi_gpu_setup.rs` |
| 3.6 | **Looking Glass integration** — IVSHMEM setup + auto-launch client | vm-curator | 2hr | new `vmm-core/src/looking_glass.rs` |
| 3.7 | VFIO/IOMMU system setup wizard (modprobe, initramfs) | vm-curator | 2hr | `single_gpu_setup.rs` |
| 3.8 | LibreVirgil driver installer ISO — pack 9 drivers into installer | Internal (librevirgil) | 3hr | build script |
| 3.9 | 3dfx wrapper ISO support for retro DOS/Win95 gaming | Vectras VM | 1hr | `config.rs`, `xml_builder.rs` |
| 3.10 | Custom virtio-win ISO with SPICE agent + Win FSP + LibreVirgil drivers | Internal | 2hr | build script |

---

## Wave 4: Advanced VM Management
**Goal:** Snapshots, cloning, notes, backup, QMP integration

| # | Task | Source | Effort | Files |
|---|------|--------|--------|-------|
| 4.1 | **VM Notes** — free-text per-VM notes | vm-curator | 1hr | `config.rs`, `vm_settings.rs` |
| 4.2 | **VM Clone** — full clone + linked clone (backing file) | Proxmox | 2hr | new `vmm-core/src/clone.rs`, `views/clone_dialog.rs` |
| 4.3 | **VM Templates** — mark VM as read-only base for clones | Proxmox | 1hr | `config.rs`, UI |
| 4.4 | **Snapshot tree view** — visual tree of snapshots with branches | VirtualBox | 3hr | `views/snapshots.rs` |
| 4.5 | **Live snapshots** with RAM state (savevm/loadvm) | Vectras VM (QMP migrate) | 2hr | `snapshot.rs` |
| 4.6 | **QMP direct integration** — Unix socket control channel | Vectras VM | 3hr | new `vmm-core/src/qmp.rs` |
| 4.7 | Hot-swap CD/DVD via QMP (not virsh) | Vectras VM | 1hr | `qmp.rs`, `media_dialog.rs` |
| 4.8 | **Backup/restore** with retention policies | Proxmox | 2hr | new `vmm-core/src/backup.rs` |
| 4.9 | Snapshot scheduling (auto-snapshot every N hours) | Proxmox | 1hr | `backup.rs` |
| 4.10 | VM rename with config migration | vm-curator | 30min | `config.rs` |

---

## Wave 5: Network & Connectivity
**Goal:** Advanced networking, port forwarding presets, SSH integration

| # | Task | Source | Effort | Files |
|---|------|--------|--------|-------|
| 5.1 | **MacVTap network mode** | Gap analysis | 1hr | `config.rs`, `xml_builder.rs` |
| 5.2 | **Port forwarding presets** — SSH(22), RDP(3389), HTTP(80/443), VNC(5900) | vm-curator | 1hr | `port_forward.rs` |
| 5.3 | **Bridge network auto-detection** — list available bridges + status | vm-curator | 1hr | new `vmm-core/src/network.rs` |
| 5.4 | Bridge setup wizard (create virbr0, configure DHCP) | vm-curator | 2hr | `network.rs` |
| 5.5 | **Automatic port forwarding** — detect guest listening ports | Lima | 2hr | `network.rs` |
| 5.6 | **SSH integration** — detect terminal emulator, one-click SSH | Quickgui | 1hr | `views/summary.rs` |
| 5.7 | SSH auto-detect (probe port for SSH banner) | Quickgui | 30min | `network.rs` |
| 5.8 | **Per-VM firewall rules** | Proxmox | 2hr | `config.rs`, `xml_builder.rs` |
| 5.9 | Passt network backend support | vm-curator | 1hr | `config.rs`, `xml_builder.rs` |
| 5.10 | NIC bandwidth limiting | Gap analysis | 30min | `config.rs`, `xml_builder.rs` |

---

## Wave 6: Cloud & Provisioning
**Goal:** Cloud-init, cloud image downloads, automated guest setup

| # | Task | Source | Effort | Files |
|---|------|--------|--------|-------|
| 6.1 | **Cloud-init ISO builder** — user-data + meta-data + network-config | Proxmox + Lima | 3hr | new `vmm-core/src/cloud_init.rs` |
| 6.2 | Cloud image download (Ubuntu cloud, Fedora cloud, Debian cloud) | Lima templates | 2hr | `os_catalog.rs` |
| 6.3 | Cloud-init GUI — hostname, user/password, SSH keys, packages | Proxmox | 2hr | new `views/cloud_init.rs` |
| 6.4 | **Auto file sharing** — mount host dirs in guest automatically | Lima | 2hr | `shared_folder` enhancements |
| 6.5 | Guest agent auto-install flow | Internal | 1hr | `guest_tools.rs` |
| 6.6 | **Ignition/Butane** support (for Fedora CoreOS) | Lima | 1hr | `cloud_init.rs` |

---

## Wave 7: USB & Peripheral Management
**Goal:** Full USB passthrough, audio routing, serial ports

| # | Task | Source | Effort | Files |
|---|------|--------|--------|-------|
| 7.1 | **USB device picker** — enumerate host USB devices (libudev) | vm-curator `usb.rs` | 2hr | new `vmm-core/src/hardware/usb_enum.rs` |
| 7.2 | USB passthrough UI — select devices to forward | vm-curator | 2hr | new `views/usb_passthrough.rs` |
| 7.3 | USB hotplug via QMP (attach/detach while running) | Vectras VM | 1hr | `qmp.rs` |
| 7.4 | Hub filtering + keyboard/mouse detection warnings | vm-curator | 30min | `usb_enum.rs` |
| 7.5 | **Audio backend selector** — SPICE, PulseAudio, PipeWire, none | Gap analysis | 30min | `config.rs`, `xml_builder.rs` |
| 7.6 | Serial/parallel port configuration | VirtualBox | 1hr | `config.rs`, `xml_builder.rs` |
| 7.7 | **Shared clipboard** status indicator + toggle | SPICE feature | 30min | `console.rs` |

---

## Wave 8: Performance & Storage
**Goal:** Disk management, BTRFS optimization, memory ballooning UI

| # | Task | Source | Effort | Files |
|---|------|--------|--------|-------|
| 8.1 | **Virtual Media Manager** — list all disks, orphan detection | VirtualBox | 2hr | new `views/media_manager.rs` |
| 8.2 | Disk compact (qemu-img convert for reclaim) | VirtualBox | 1hr | `disk.rs` |
| 8.3 | Disk format conversion UI (qcow2↔raw↔vmdk↔vdi) | vm-curator | 1hr | `disk_manage.rs` |
| 8.4 | **BTRFS CoW auto-disable** for VM directories | vm-curator | 30min | `disk.rs` |
| 8.5 | **Memory ballooning UI** — current/max slider, stats display | Proxmox | 1hr | `vm_settings.rs`, `balloon.rs` |
| 8.6 | **Huge pages toggle** (2MB/1GB) | Gap analysis | 30min | `config.rs`, `xml_builder.rs` |
| 8.7 | IO thread pinning / CPU pinning | Proxmox | 1hr | `config.rs`, `xml_builder.rs` |
| 8.8 | Disk I/O limits (IOPS + bandwidth caps) | Proxmox | 30min | `config.rs`, `xml_builder.rs` |
| 8.9 | Cache mode selector (none/writeback/writethrough/unsafe) | Gap analysis | 15min | already done, verify UI |

---

## Wave 9: UX & Quality of Life
**Goal:** Polish, discoverability, first-run experience

| # | Task | Source | Effort | Files |
|---|------|--------|--------|-------|
| 9.1 | **First-run setup wizard** — detect QEMU, KVM, libvirt, OVMF | vm-curator | 2hr | new `views/first_run.rs` |
| 9.2 | **VM discovery** — scan for existing QEMU/libvirt VMs on first run | vm-curator | 1hr | `import.rs` |
| 9.3 | **Basic/Expert UI mode** toggle | VirtualBox | 2hr | app-wide |
| 9.4 | **Screen recording** — start/stop recording VM display | Gap analysis | 2hr | `console.rs` |
| 9.5 | **VM auto-start** at host boot (systemd unit generation) | Proxmox | 1hr | `config.rs` |
| 9.6 | Startup delay for auto-start VMs | Proxmox | 15min | `config.rs` |
| 9.7 | Shutdown timeout (graceful → force-kill timer) | vm-curator | 30min | `lifecycle` |
| 9.8 | Resource tags/groups — organize VMs by project | Proxmox | 1hr | `config.rs`, sidebar |
| 9.9 | Desktop notifications (VM started, stopped, snapshot done) | Quickgui | 1hr | app-wide |
| 9.10 | OS icons/logos in VM list sidebar | Quickgui + vm-curator | 1hr | sidebar |

---

## Wave 10: Multi-Arch & Exotic
**Goal:** ARM, RISC-V, retro OS support, ROM management

| # | Task | Source | Effort | Files |
|---|------|--------|--------|-------|
| 10.1 | **120+ OS profiles** (retro, BSD, Unix, mobile) | vm-curator TOML | 2hr | `template_library.rs` |
| 10.2 | Floppy disk support (boot floppy for OS/2, DOS) | vm-curator | 30min | `config.rs`, `xml_builder.rs` |
| 10.3 | **ROM/BIOS file management** (Classic Mac ROM, custom BIOS) | vm-curator + Vectras | 1hr | `config.rs` |
| 10.4 | Per-arch firmware auto-detection (Arch, Debian, Fedora, NixOS paths) | vm-curator | 1hr | `qemu_archs.rs` |
| 10.5 | Android-x86 / LineageOS / Bliss OS profiles | vm-curator | 30min | `template_library.rs` |
| 10.6 | Classic Mac OS profiles (System 6-9 with SheepShaver/Basilisk) | vm-curator | 1hr | `template_library.rs` |
| 10.7 | **Headless VM mode** (no display, background service) | vm-curator | 30min | `config.rs` |
| 10.8 | Multi-arch UEFI firmware bundling (embed OVMF as asset) | Vectras VM | 1hr | build system |

---

## Summary: Priority vs Effort Matrix

```
                    LOW EFFORT ◄─────────────────► HIGH EFFORT
                    │                                        │
  HIGH IMPACT  ─────┤  1.5 macOS support          3.1-3.6 GPU passthrough
                    │  1.4 CD/DVD fix             2.1-2.5 Import wizard
                    │  4.1 VM Notes               2.6-2.9 OS download
                    │  5.2 Port fwd presets        4.6 QMP integration
                    │  9.10 OS icons               6.1-6.3 Cloud-init
                    │                                        │
  LOW IMPACT   ─────┤  8.4 BTRFS CoW             10.1 120+ profiles
                    │  8.6 Huge pages              9.3 Basic/Expert mode
                    │  10.2 Floppy support         9.4 Screen recording
                    │  10.7 Headless mode           7.1-7.3 USB picker
                    │                                        │
                    └────────────────────────────────────────┘
```

---

## Source Attribution

| Source | License | What We Borrow |
|--------|---------|----------------|
| [OSX-KVM](https://github.com/kholia/osx-kvm) | MIT-ish | macOS QEMU args, SMC, OpenCore approach |
| [VMware Unlocker](https://github.com/DrDonk/unlocker) | MIT | Apple SMC OSK key injection technique, vSMC data structures, darwin.iso tools |
| [vm-curator](https://github.com/mroboff/vm-curator) | MIT | Import wizard, GPU passthrough, PCI scanner, OS profiles, USB enum |
| [Lima](https://github.com/lima-vm/lima) | Apache 2.0 | Auto port forwarding, cloud image templates, auto file sharing concepts |
| [Vectras VM](https://github.com/xoureldeen/Vectras-VM-Android) | GPL-3.0 | QMP integration patterns, ROM management, 3dfx support |
| [Quickgui](https://github.com/quickemu-project/quickgui) | MIT | OS download wizard flow, searchable OS catalog, SSH detection |
| Proxmox VE | AGPL-3.0 | Cloud-init, backup retention, firewall, VM templates/clones |
| VirtualBox | GPL-2.0 | Snapshot tree, virtual media manager, basic/expert mode |
