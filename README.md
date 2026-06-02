# Libre VMM

> A libre desktop VM manager for Linux, built on QEMU/KVM + libvirt + Rust.
<img width="1920" height="1080" alt="Screenshot_20260531_121958" src="https://github.com/user-attachments/assets/c4e168ad-727f-4154-8fba-2326f0ed38ab" />

Libre VMM is a full-lifecycle virtual machine manager with a modern
desktop GUI, a documented REST API, a CLI with shell completions, and
support for 24 guest architectures. It is Wayland-native, ships
declarative TOML/YAML VM specs, and is GPL-3.0-or-later end to end.

[![Status](https://img.shields.io/badge/status-alpha-orange)](https://github.com/LucentOpenSoftware/libre-vmm)
[![Version](https://img.shields.io/badge/version-0.1.0-blue)](CHANGELOG.md)
[![License](https://img.shields.io/badge/license-GPL--3.0--or--later-blue)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-438%20passing-brightgreen)](#testing)

---

## What it does

Libre VMM sits on top of QEMU and libvirt and gives you one workflow for
the entire VM lifecycle: build, run, snapshot, clone, migrate, back up,
restore, retire. Where existing tools force you to mix `virsh`,
`virt-manager`, `qemu-img`, shell scripts, and a hypervisor's own
console, Libre VMM unifies the surface — desktop, terminal, and HTTP — on
top of the same Rust core.

### Capabilities

**VM lifecycle**

- Full lifecycle controls: create, start, shutdown, force-stop, pause,
  resume, reboot, suspend-to-disk
- Snapshot trees with branches, scheduled "AutoProtect" snapshots, and
  filesystem-consistent (`fsfreeze`) quiesced snapshots
- Full clones and linked clones via qcow2 backing files
- Hot-add disks on virtio-blk, virtio-scsi, NVMe, SATA, and IDE buses
- Restricted VMs with atomic policy save, op-allowlist, and expiration
- Backup and restore with zstd compression

**Storage & firmware**

- qcow2 native — sparse, encryptable, copy-on-write
- LUKS-encrypted disks managed through libvirt secrets
- TPM 2.0 emulation via `swtpm`
- Secure Boot via LibreUEFI — our own EDK2 fork with branding, battery
  and thermal ACPI tables, and an `fw_cfg` bridge for guest tooling

**Networking**

- NAT, Bridged, Host-only, and isolated LAN-segment topologies
- Per-VM firewall rules via libvirt `nwfilter` (nftables under the hood)
- Network conditioner — latency, loss, and bandwidth limits per NIC
- Automatic guest port forwarding (Lima-style detect-and-forward through
  `qemu-guest-agent`)

**Display & guest integration**

- Wayland-native GUI on `eframe` / `egui`
- VNC and SPICE consoles; SPICE provides clipboard, drag-drop,
  multi-monitor, and audio
- `virtiofs` shared folders; `qemu-guest-agent` for time sync, freeze,
  and host-guest commands
- Screen recording, Picture-in-Picture, desktop notifications

**Performance**

- Live migration between hosts with a 4-step wizard (progress + cancel)
- Single-GPU passthrough wizard (TTY switch, hook scripts, sudoers
  helper that the user installs manually)
- Multi-GPU passthrough and Looking Glass integration
- Hugepages, IO threads, io_uring, CPU pinning, per-VM I/O throttle

**Multi-architecture**

- 24 QEMU architectures including x86_64, i386, aarch64, arm, riscv64,
  riscv32, ppc64, s390x, mips/mips64, sparc, alpha, hppa, m68k,
  loongarch64, sh4, or1k, microblaze, avr, and xtensa
- KVM acceleration on same-arch hosts; TCG emulation for cross-arch

**Automation surface**

- REST API with an [OpenAPI 3.1 spec](docs/openapi.json), swagger-ui at
  `/api/v1/docs`, redoc at `/api/v1/redoc`, `X-API-Key` auth
- CLI (`vmm`) with 16 subcommands and shell completions for bash, zsh,
  and fish
- Declarative `VmConfig` serializable to JSON, TOML, and YAML —
  diffable, version-controllable
- Import from libvirt XML, Quickemu `.conf`, VMware `.vmx`, and
  VirtualBox `.vbox`

For a side-by-side comparison with VMware Workstation Pro, see
[`docs/VMWARE-PARITY.md`](docs/VMWARE-PARITY.md).

---

## Quick Start

### System requirements

- Linux host with KVM enabled in BIOS (Intel VT-x or AMD-V)
- QEMU 8.2 or newer
- libvirt 10.0 or newer
- OVMF or LibreUEFI firmware (for UEFI guests)
- Optional: `swtpm` for TPM 2.0, `looking-glass-client` for low-latency
  GPU passthrough
- Wayland or X11 desktop session

### Installation

Until Libre VMM lands in distribution repositories, build from source.
Per-distro packaging manifests live under [`packaging/`](packaging/) and
are ready to consume once the v0.1.0 tarball is published.

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

```bash
git clone https://github.com/LucentOpenSoftware/libre-vmm.git
cd libre-vmm
sudo dnf install qemu-kvm libvirt-devel ovmf swtpm cargo rust
./build.sh release
sudo ./scripts/install.sh
```

The RPM spec lives in [`packaging/rpm/`](packaging/rpm/) with a Copr
build stub. See [`packaging/rpm/README.md`](packaging/rpm/README.md).

#### Arch Linux

```bash
git clone https://github.com/LucentOpenSoftware/libre-vmm.git
cd libre-vmm/packaging/aur
makepkg -si
```

Once submitted to the AUR you will be able to install with `yay -S libre-vmm`.

#### Flatpak

```bash
cd packaging/flatpak
flatpak-builder --user --install build-dir org.libre_vmm.LibreVmm.yml
```

The Flatpak runs under `qemu:///session` so it can manage user-scope VMs
inside the sandbox.

### First run

```bash
libre-vmm          # launches the GUI; first-run wizard checks the system
vmm list           # CLI with 16 subcommands and shell completions
vmm-api            # REST API server, X-API-Key auth, OpenAPI docs at /api/v1/docs
```

The first-run wizard detects QEMU, KVM, libvirt, OVMF, and `swtpm`, and
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
| `vmm-cli` | `clap`-based CLI with 16 subcommands and shell completions. | Linux (Windows/macOS future) |
| `vmm-api` | Axum REST API server, OpenAPI 3.1, swagger-ui, redoc. | Linux (Windows/macOS future) |

The split is deliberate. `vmm-types` already compiles for
`x86_64-pc-windows-gnu` and is the foundation for the multi-quarter
Windows-host port described in
[`docs/WINDOWS-PORT.md`](docs/WINDOWS-PORT.md).

Total: roughly 70,000 lines of Rust across the workspace.

---

## Status: v0.1.0 (alpha)

Libre VMM works. It runs VMs. It manages snapshots. It does GPU
passthrough. It exposes a REST API. **But it is pre-1.0 software.**
Expect bugs, missing edge cases, and breaking changes between minor
versions. Always back up your disks before reverting a snapshot.

### What is in flight

- **Wave 13.10:** signed releases and in-app updater
- **Wave 14:** Terraform / Ansible / Vagrant providers, webhook events,
  Prometheus metrics
- **Wave 15:** Confidential VMs (SEV-SNP / TDX), reproducible builds,
  GPU mdev/SR-IOV
- **Waves 16–20:** Windows host port — see
  [`docs/WINDOWS-PORT.md`](docs/WINDOWS-PORT.md)

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

- [Feature matrix and roadmap](docs/VMWARE-PARITY.md) — capability comparison and future-feature tracker
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

GPL-3.0-or-later. See [LICENSE](LICENSE). Contributions are accepted
under the same license; see [CONTRIBUTING.md](CONTRIBUTING.md) for the
DCO sign-off requirement.

---

## Acknowledgments

Libre VMM stands on a stack of excellent libre projects:

- **QEMU** — three decades of device emulation we don't have to reinvent
- **libvirt** — our hypervisor abstraction boundary
- **TianoCore / EDK2** — the upstream firmware our LibreUEFI fork is based on
- **eframe / egui** (emilk) — the GUI toolkit
- **Axum** and the Tokio ecosystem — the REST API server
- **The Rust ecosystem** — `serde`, `clap`, `tracing`, `anyhow`, and many more

We also borrow ideas, configuration patterns, and (where licensed
compatibly) code from upstream projects listed in [AUTHORS](AUTHORS) and
documented in [`docs/ROADMAP.md`](docs/ROADMAP.md).

---

## A Small Historical Note

Libre VMM began with a simple idea:

> Virtual machines should not require becoming a virtualization engineer.

The original goal was modest.

A few improvements here.
A few workflow fixes there.

At some point, however, the project acquired:

* a REST API
* a CLI
* migration tooling
* GPU passthrough helpers
* twenty-four architectures
* a firmware fork

The exact sequence of events remains under investigation.

Researchers believe Libre VMM may once have been related to a Virt-Manager fork, though surviving records are incomplete.

### Recovered Conversation

Libre-VMM:
"Hey."

LibreUEFI:
"Yeah?"

Libre-VMM:
"Do you remember how this started?"

LibreUEFI:
"No."

Libre-VMM:
"You don't?"

LibreUEFI:
"I thought we always did virtualization."

Libre-VMM:
"We didn't."

LibreUEFI:
"We didn't?"

Libre-VMM:
"No."

LibreUEFI:
"Then how did we end up with a firmware fork?"

Libre-VMM:
"I don't know."

LibreUEFI:
"And the API?"

Libre-VMM:
"I don't know."

LibreUEFI:
"And migration?"

Libre-VMM:
"I don't know."

LibreUEFI:
"And twenty-four architectures?"

Libre-VMM:
"I definitely don't know."

...

LibreUEFI:
"Weren't we a Virt-Manager fork?"

Libre-VMM:
"I think so."

LibreUEFI:
"That's weird."

Libre-VMM:
"Yeah."

...

LibreUEFI:
"Do you think we're done?"

Libre-VMM:
"No."

LibreUEFI:
"Why?"

Libre-VMM:
"I have a feeling we're about to invent something else."

...

[End of recovered transcript]

Researchers believe this conversation occurred shortly before the appearance of the ISO manager, template repository, and VM marketplace.

Further incidents are expected.

