# Libre VMM

> A libre alternative to VMware Workstation, built on QEMU/KVM + libvirt + Rust.

Libre VMM is a desktop VM manager for Linux that runs production-grade
virtual machines without the things that make VMware Workstation painful:
no Broadcom Support Portal, no kernel modules that break every update, no
XWayland-only display stack, no proprietary disk format. It ships
Wayland-native, supports **24 guest architectures**, exposes a documented
**REST API with OpenAPI 3.1**, and is **GPL-3.0-or-later** end to end.

[![Status](https://img.shields.io/badge/status-alpha-orange)](https://github.com/LucentOpenSoftware/libre-vmm)
[![Version](https://img.shields.io/badge/version-0.1.0-blue)](CHANGELOG.md)
[![License](https://img.shields.io/badge/license-GPL--3.0--or--later-blue)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-438%20passing-brightgreen)](#testing)

---

## Why Libre VMM?

VMware Workstation has good bones but a broken delivery model. Broadcom's
support portal is a maze, the Linux installer rots with every kernel
update, there is no first-class CLI or REST API, qcow2 is not supported,
GPU passthrough is locked behind vSphere, and Wayland is "supported" only
via XWayland with visible glitches. Libre VMM is what an honest, modern
replacement looks like.

We are **~85-90% feature-equivalent** with VMware Workstation Pro 17 for
single-host VM management today, and we ship several capabilities that
VMware Workstation structurally cannot. See
[`docs/VMWARE-PARITY.md`](docs/VMWARE-PARITY.md) for the full feature
matrix.

### Differentiators

These are the answers to "why not VMware?":

- **Live migration between hosts** — VMware Workstation can't. Libre VMM has a 4-step GUI wizard with progress and cancel.
- **GPU passthrough** — single-GPU wizard, multi-GPU, and Looking Glass integration. vSphere-only on VMware.
- **24 guest architectures** — x86_64, ARM64, RISC-V, MIPS, PPC, s390x, and 18 more. VMware does x86 only.
- **REST API + OpenAPI 3.1 spec** — Terraform/Ansible-friendly out of the box. VMware has `vmrun`.
- **qcow2 native** — sparse, encryptable, COW backing files. Strictly better than VMDK.
- **Cloud-init + autounattend.xml** built in — open, hackable, future-proof.
- **Wayland-native** — egui + winit. See [`docs/WAYLAND-COMPATIBILITY.md`](docs/WAYLAND-COMPATIBILITY.md).
- **Per-VM nftables firewall rules** via libvirt nwfilter. VMware has nothing comparable.
- **Automatic guest port forwarding** — Lima-style detect-and-forward through `qemu-guest-agent`.
- **Declarative TOML/YAML VM specs** — diffable, version-controllable. VMware's `.vmx` is opaque.
- **LibreUEFI** — our own EDK2 fork with branding, battery/thermal ACPI extras, and an `fw_cfg` bridge.
- **Kernel-version-independent** — KVM is in-tree. We don't ship out-of-tree modules.
- **GPL-3.0-or-later**, no Broadcom portal, no MFA, no "free for X but not Y" licensing.
- **Network conditioner** (latency, loss, bandwidth) on every NIC.

For the full per-feature breakdown, see [`docs/VMWARE-PARITY.md`](docs/VMWARE-PARITY.md).

---

## Quick Start

### System requirements

- Linux host with KVM (Intel VT-x or AMD-V enabled in BIOS)
- QEMU 8.2 or newer
- libvirt 10.0 or newer
- OVMF or LibreUEFI firmware (for UEFI guests)
- Optional: `swtpm` for TPM 2.0, `looking-glass-client` for low-latency GPU passthrough
- Wayland or X11 desktop session

### Installation

Until Libre VMM lands in distribution repositories, build from source.
Per-distro packaging manifests live under
[`packaging/`](packaging/) and are ready to consume once the v0.1.0
tarball is published.

#### Debian / Ubuntu

```bash
git clone https://github.com/LucentOpenSoftware/libre-vmm.git
cd libre-vmm
./scripts/setup-deps.sh        # installs qemu, libvirt, ovmf, swtpm
./build.sh release
sudo ./scripts/install.sh
```

A Debian source package (`debian/control`, `rules`, `postinst`) is in
[`packaging/debian/`](packaging/debian/) — see
[`packaging/debian/README.md`](packaging/debian/README.md) for the
`debuild` workflow.

#### Fedora / RHEL

The RPM spec lives in [`packaging/rpm/`](packaging/rpm/) with a Copr build
stub. See [`packaging/rpm/README.md`](packaging/rpm/README.md).

```bash
git clone https://github.com/LucentOpenSoftware/libre-vmm.git
cd libre-vmm
sudo dnf install qemu-kvm libvirt-devel ovmf swtpm cargo rust
./build.sh release
sudo ./scripts/install.sh
```

#### Arch Linux

A PKGBUILD is provided at [`packaging/aur/PKGBUILD`](packaging/aur/PKGBUILD).
Once submitted to the AUR you will be able to run:

```bash
yay -S libre-vmm        # planned AUR name
```

For now, build locally:

```bash
git clone https://github.com/LucentOpenSoftware/libre-vmm.git
cd libre-vmm/packaging/aur
makepkg -si
```

#### Flatpak

A Flatpak manifest is at
[`packaging/flatpak/org.libre_vmm.LibreVmm.yml`](packaging/flatpak/org.libre_vmm.LibreVmm.yml).
The Flatpak runs under `qemu:///session` so it can manage user-scope VMs
inside the sandbox.

```bash
cd packaging/flatpak
flatpak-builder --user --install build-dir org.libre_vmm.LibreVmm.yml
```

### First run

```bash
libre-vmm          # launches the GUI; first-run wizard checks the system
vmm list           # CLI; 16 subcommands, includes shell completions
vmm-api            # REST API server, X-API-Key auth, OpenAPI docs at /api/v1/docs
```

The first-run wizard detects QEMU, KVM, libvirt, OVMF, and swtpm, and
offers to import existing VMs from libvirt, Quickemu, VirtualBox, and
VMware libraries.

---

## Architecture

```
+----------+   +----------+   +----------+
| vmm-gui  |   | vmm-cli  |   | vmm-api  |
+----+-----+   +----+-----+   +----+-----+
     \              |              /
      \             |             /
       +----- vmm-core (libvirt FFI, Linux) -----+
                       |
                  vmm-types
       (pure data, cross-platform, compiles for Windows)
```

| Crate | Purpose | Platform |
|---|---|---|
| `vmm-types` | Pure data types — `VmConfig`, `VmInfo`, enums. No I/O, no platform code. | any (incl. `x86_64-pc-windows-gnu`) |
| `vmm-core` | libvirt FFI, system integration, hypervisor backend. | Linux |
| `vmm-gui` | Desktop client built on `eframe` / `egui` 0.30. | Linux (Windows/macOS future) |
| `vmm-cli` | `clap`-based CLI, 16 subcommands, shell completions. | Linux (Windows/macOS future) |
| `vmm-api` | Axum REST API server, OpenAPI 3.1, swagger-ui, redoc. | Linux (Windows/macOS future) |

The split is deliberate. `vmm-types` already compiles for
`x86_64-pc-windows-gnu` and is the foundation for the multi-quarter
Windows-host port described in
[`docs/WINDOWS-PORT.md`](docs/WINDOWS-PORT.md).

Total: ~70,000 lines of Rust across the workspace.

---

## Status: v0.1.0 (alpha)

Libre VMM works. It runs VMs. It manages snapshots. It does GPU
passthrough. It exposes a REST API. **But it is pre-1.0 software.**
Expect bugs, missing edge cases, and breaking changes between minor
versions. Always back up your disks before reverting a snapshot.

### What works today

- Full VM lifecycle (create, start, shutdown, force-stop, pause, resume, reboot, suspend)
- Snapshot tree with branches, AutoProtect scheduled snapshots, and quiesced (fsfreeze) snapshots
- Full and linked clones (qcow2 backing files), template VMs
- LUKS-encrypted disks, swtpm TPM 2.0, Secure Boot via LibreUEFI
- Hot-add disks on all four buses (virtio-blk, virtio-scsi, NVMe, SATA, IDE)
- 24 guest architectures (`qemu_archs.rs`)
- LAN segments (isolated VM-to-VM bridges), NAT, Bridged, Host-only networking
- Per-VM firewall rules (libvirt nwfilter), network conditioner, NIC bandwidth limits
- Live migration between hosts (4-step wizard, progress, cancel)
- Single-GPU passthrough wizard (TTY switch + hook scripts), multi-GPU, Looking Glass
- REST API with OpenAPI 3.1 spec at [`docs/openapi.json`](docs/openapi.json), swagger-ui at `/api/v1/docs`, redoc at `/api/v1/redoc`
- Declarative TOML/YAML VM specs (`VmConfig::to_toml` / `to_yaml`)
- VM discovery and import from libvirt, Quickemu, VirtualBox, VMware libraries
- First-run wizard (7 steps) with system detection and dependency guidance
- Cloud-init / autounattend.xml provisioning
- Screen recording, Picture-in-Picture, desktop notifications
- Restricted VMs (atomic policy save, op-allowlist, expiration)
- Backup/restore with zstd compression

### What is in flight

- **Wave 13.10:** signed releases and in-app updater
- **Wave 14:** Terraform / Ansible / Vagrant providers, webhook events, Prometheus metrics
- **Wave 15:** Confidential VMs (SEV-SNP / TDX), reproducible builds, GPU mdev/SR-IOV
- **Waves 16-20:** Windows host port — see [`docs/WINDOWS-PORT.md`](docs/WINDOWS-PORT.md)

The full per-wave plan is in [`docs/ROADMAP.md`](docs/ROADMAP.md).

### Testing

438 tests pass across the workspace:

- `vmm-core`: 403
- `vmm-types`: 30
- `vmm-api`: 5

Run them locally:

```bash
PKG_CONFIG_PATH=$PWD/lib/pkgconfig cargo test --workspace
```

---

## Documentation

- [VMware Parity & Future Vision](docs/VMWARE-PARITY.md) — feature matrix, differentiators, anti-features
- [Implementation Roadmap](docs/ROADMAP.md) — wave-by-wave plan with source attribution
- [Windows Port Strategy](docs/WINDOWS-PORT.md) — multi-quarter cross-platform plan
- [Wayland Compatibility](docs/WAYLAND-COMPATIBILITY.md) — Sway / Hyprland / GNOME / KDE matrix
- [LibreUEFI Guest OS Requirements](docs/libreuefi-guest-os-requirements.md)
- [REST API Spec](docs/openapi.json) — also rendered at `/api/v1/docs` (swagger-ui) and `/api/v1/redoc`

---

## Contributing

Patches, bug reports, packaging help, and translation work are all
welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for the development
environment setup, commit conventions, and DCO sign-off requirement.

Looking for somewhere to start? The `📋 planned` rows in
[`docs/VMWARE-PARITY.md`](docs/VMWARE-PARITY.md) and the per-wave tables
in [`docs/ROADMAP.md`](docs/ROADMAP.md) are pre-scoped tasks with file
pointers and effort estimates.

---

## Security

See [SECURITY.md](SECURITY.md) for the supported-version table, the
disclosure policy, and the list of hardening measures the codebase
already takes (path traversal mitigations, command-injection guards,
XML-injection guards, atomic file writes, mutex-poisoning recovery,
passphrase zeroization). **Do not file public issues for security
reports.**

---

## License

GPL-3.0-or-later. See [LICENSE](LICENSE) (to be added before v0.1.0
tag). Contributions are accepted under the same license; see
[CONTRIBUTING.md](CONTRIBUTING.md) for the DCO sign-off requirement.

---

## Acknowledgments

Libre VMM stands on a stack of excellent libre projects:

- **QEMU** — 30 years of device emulation we don't have to reinvent
- **libvirt** — our hypervisor abstraction boundary
- **TianoCore / EDK2** — the upstream firmware our LibreUEFI fork is based on
- **eframe / egui** (emilk) — the GUI toolkit
- **Axum** and the Tokio ecosystem — the REST API server
- **The Rust ecosystem** — `serde`, `clap`, `tracing`, `anyhow`, and many more

We also borrow ideas, configuration patterns, and (where licensed
compatibly) code from upstream projects listed in [AUTHORS](AUTHORS) and
documented in [`docs/ROADMAP.md`](docs/ROADMAP.md).
