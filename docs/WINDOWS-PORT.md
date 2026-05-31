# Libre VMM — Windows Port Strategy

> **Status:** Long-term initiative. Multi-quarter scope. Tracking begins May 2026.
>
> **Goal:** Native Windows host support — running VMs *on* Windows, not just managing remote Linux hosts from Windows. The full first-class experience.
>
> **Non-goal:** Replicating WSL2 (Linux-on-Windows). This document covers native Windows hypervisor integration.

---

## 1. The Decision

Libre VMM will be a **truly cross-platform VM manager** with native backends on Linux and Windows. Both hosts use:
- The same Rust workspace (`vmm-core`, `vmm-gui`, `vmm-cli`, `vmm-api`)
- The same configuration format (`VmConfig` → libvirt XML)
- The same disk format (qcow2)
- The same UEFI firmware (LibreUEFI/OVMF)
- The same display protocols (VNC, SPICE)
- The same REST API and CLI

What differs is the **hypervisor backend layer**: Linux uses KVM via libvirt; Windows uses **QEMU+WHPX** via libvirt.

### Why QEMU+WHPX instead of native Hyper-V

| Criterion | QEMU + WHPX | Native Hyper-V backend |
|---|---|---|
| Codebase reuse | ~85% shared with Linux | ~30% — new abstraction layer needed everywhere |
| Disk format | qcow2 (current) | VHDX (new code) |
| VM portability between Linux/Windows | ✅ same XML, same disk | ❌ different formats |
| Live migration Linux ↔ Windows | ✅ possible with QEMU on both sides | ❌ different hypervisors |
| TPM emulation | swtpm (current) | Microsoft vTPM (new code) |
| UEFI firmware | LibreUEFI (current) | Microsoft UEFI (new code) |
| Snapshot trees | qcow2 backing files (current) | Hyper-V .avhdx checkpoints (new code) |
| GPU passthrough | vfio-equivalent via DDA | Hyper-V DDA |
| Engineering cost (rough) | 6-9 months | 18-24 months |
| Future maintenance | one feature codebase | two parallel feature codebases forever |
| Precedent | VMware Workstation 16+ uses WHPX | virt-manager Windows abandoned |

**Decision: QEMU+WHPX backend via libvirt.** This is what VMware Workstation 16+ does (they switched from their proprietary VMM to WHPX). It's the proven path.

### What we accept by choosing QEMU+WHPX

- **Hyper-V cannot run concurrently** on the same Windows install. Users must pick: Hyper-V Manager + Docker Desktop + WSL2 + Windows Containers (all use Hyper-V) **or** Libre VMM + VirtualBox + VMware (all use WHPX). Same constraint VMware has.
- **Slightly more setup** than Hyper-V Manager: user must enable WHPX feature, install QEMU.
- **No nested virtualization of Hyper-V guests** from inside Libre VMM VMs (rare use case).

### What we get

- **One feature codebase** for everything that isn't OS-specific
- **VMs are portable** — `vm-name.qcow2` + `vm-name.toml` work on any Libre VMM host
- **Live migration across operating systems** — Linux host → Windows host of the same VM
- **Same documentation, same UX, same i18n** — VMware-killer messaging stays consistent

---

## 2. Architectural Strategy

The current architecture:

```
┌───────────────────────────────────────────┐
│  vmm-gui  vmm-cli  vmm-api                │
└────────────────┬──────────────────────────┘
                 │
        ┌────────▼────────┐
        │    vmm-core      │
        │  (libvirt FFI    │
        │   + Unix calls)  │
        └────────┬────────┘
                 │
        ┌────────▼────────┐
        │    libvirt       │
        │   ↓              │
        │    KVM / QEMU    │
        └──────────────────┘
```

The target architecture:

```
┌───────────────────────────────────────────────────┐
│  vmm-gui  vmm-cli  vmm-api                        │
│  (cross-platform: Windows, Linux, macOS)          │
└────────────────────┬──────────────────────────────┘
                     │
        ┌────────────▼────────────┐
        │      vmm-types          │
        │  (pure Rust, no I/O)    │
        └────────────┬────────────┘
                     │
        ┌────────────▼────────────┐
        │   Hypervisor trait       │
        │   (vmm-hypervisor crate)│
        └─┬──────────────────────┬┘
          │                      │
   ┌──────▼────────┐    ┌────────▼────────┐
   │ LibvirtKvm    │    │ LibvirtWhpx     │
   │ (Linux)       │    │ (Windows)       │
   └──────┬────────┘    └────────┬────────┘
          │                      │
       libvirt → KVM         libvirt → QEMU → WHPX
```

### Crate-level changes

| Crate | Today | Future |
|---|---|---|
| `vmm-types` | (doesn't exist) | **new** — pure types: `VmConfig`, `VmInfo`, `SnapshotInfo`, all enums. No I/O, no platform code. Compiles everywhere. |
| `vmm-core` | Mixed types + libvirt + Unix code | Trimmed: only Linux-specific helpers and the `LibvirtKvm` Hypervisor impl. `cfg(unix)` gated. |
| `vmm-windows` | (doesn't exist) | **new** — Windows-specific helpers and the `LibvirtWhpx` Hypervisor impl. `cfg(windows)` gated. |
| `vmm-hypervisor` | (doesn't exist) | **new** — defines the `Hypervisor` trait, hosts platform-conditional re-exports. |
| `vmm-gui` | Depends on vmm-core directly | Depends on vmm-types + vmm-hypervisor (platform-agnostic). |
| `vmm-cli` | Same | Same. |
| `vmm-api` | Same | Same. |

### The Hypervisor trait

```rust
#[async_trait]
pub trait Hypervisor: Send + Sync {
    fn connection_info(&self) -> ConnectionInfo;

    async fn list_vms(&self) -> Result<Vec<VmInfo>>;
    async fn create_vm(&self, config: &VmConfig) -> Result<()>;
    async fn start_vm(&self, name: &str) -> Result<()>;
    async fn shutdown_vm(&self, name: &str) -> Result<()>;
    async fn force_stop_vm(&self, name: &str) -> Result<()>;
    // ... (mirror the current HypervisorConnection methods)

    async fn create_snapshot(&self, vm: &str, snap: &str, desc: &str, with_mem: bool) -> Result<()>;
    async fn list_snapshots(&self, vm: &str) -> Result<Vec<SnapshotInfo>>;
    // ...

    async fn get_console_port(&self, vm: &str) -> Result<Option<u16>>;
    fn console_protocol(&self) -> DisplayProtocol;

    async fn migrate(&self, vm: &str, dest: &dyn Hypervisor, kind: MigrationKind) -> Result<()>;

    // Returns false on Windows — VFIO is Linux-only. Windows uses DDA via Hyper-V which we don't ship.
    fn supports_pci_passthrough(&self) -> bool;
}
```

Implementations:
- `LibvirtKvm` (Linux) — wraps current `HypervisorConnection`
- `LibvirtWhpx` (Windows) — new impl, talks to libvirt Windows build, configures QEMU with `-accel whpx`
- `Remote` — wraps the REST API (already enables cross-platform "thin client" use)

---

## 3. Phases

### Phase A: Foundation (1–2 months)
**Goal:** Codebase compiles on Windows with stubs. No actual VM operations work yet, but the architecture supports them.

- A1. ✅ **SHIPPED** Create `vmm-types` crate. Extract pure data types from vmm-core. 3,243 lines extracted (11 enums + 8 structs + 5 referenced module structs + pure helpers). Compiles cleanly for `x86_64-pc-windows-gnu`. 30 dedicated tests + 403 vmm-core tests still passing.
- A2. Cross-platform path helpers (`platform_dirs::*` for config, data, cache). ~3 days.
- A3. Cross-platform process spawning helper. Replace direct `Command::new()` with a wrapper that handles `.exe` extension and shell differences. ~3 days.
- A4. Create `vmm-hypervisor` crate with the trait. Move `HypervisorConnection` into `LibvirtKvm` impl. Wire vmm-gui to use the trait via a `Box<dyn Hypervisor>`. ~1 week.
- A5. Cross-platform file permissions (`std::fs::Permissions`) — already cross-platform; the issue is `os::unix::fs::PermissionsExt` calls. Stub them on Windows via `windows::Win32` ACLs or document that 0o600 becomes "owner-only" via Windows ACL. ~1 week.
- A6. Cross-compile setup: `cargo build --target x86_64-pc-windows-gnu` succeeds with all-stub `LibvirtWhpx`. ~3 days.
- A7. CI: Windows builds on every commit (GitHub Actions windows-latest runner). ~2 days.

### Phase B: Windows Backend Core (3–4 months)
**Goal:** Actually run a VM on Windows.

- B1. Set up WHPX detection: check Windows feature `HypervisorPlatform`, `HypervisorPlatformSlatVm`, `VirtualMachinePlatform`. Helpful error messages telling user how to enable. ~1 week.
- B2. Bundle QEMU for Windows — automated download from official QEMU Windows builds, hash-verified, stored in `%ProgramFiles%\LibreVMM\qemu\`. ~1 week.
- B3. Bundle OVMF/LibreUEFI for Windows (cross-compile from the existing EDK2 fork). ~2 weeks.
- B4. Bundle libvirt for Windows — use the official libvirt Windows port. ~1 week.
- B5. `LibvirtWhpx` impl: list_vms, create_vm, start_vm, stop_vm. Calls libvirt with `-accel whpx` instead of `-accel kvm` in the generated XML. ~3 weeks.
- B6. Storage: qcow2 disk creation works on Windows (qemu-img.exe). VHDX import support (one-direction: import from Hyper-V Manager). ~2 weeks.
- B7. Networking: Windows TAP driver setup OR user-mode networking only. NAT mode works without driver. Bridged mode needs OpenVSwitch/TAP. Decide and document. ~3 weeks.
- B8. SPICE on Windows: bundle spice-gtk or build native. SPICE client works on Windows historically (Citrix used it). ~2 weeks.
- B9. Snapshots: qcow2 backing files work on Windows. Test snapshot tree end-to-end. ~1 week.
- B10. TPM: swtpm has Windows builds — bundle it. ~1 week.
- B11. Live migration over TCP between a Linux and Windows host (this is the killer demo). ~2 weeks.

### Phase C: Build/Distribution (1 month)
- C1. Windows MSI installer via WiX or `cargo-wix`. ~1 week.
- C2. Code signing infrastructure (need a code signing cert). ~3 days setup, then per-release.
- C3. Auto-updater integrated with installer. ~1 week.
- C4. winget manifest submission. ~3 days.
- C5. Chocolatey package (optional, lower priority). ~3 days.
- C6. Documentation: "Installing Libre VMM on Windows". ~3 days.

### Phase D: Feature Parity (2 months)
- D1. Audit all Wave 11–13 features against Windows backend. Triage: works / needs Windows-specific impl / not applicable. ~1 week.
- D2. USB passthrough on Windows (libusb-1.0 + driver setup). ~2 weeks.
- D3. GPU passthrough on Windows: **explicitly out of scope** for v1. Document that vfio-pci is Linux-only. Future: DDA via WMI. ~1 day.
- D4. Looking Glass on Windows: client-side works (it's already a Windows app). IVSHMEM bundling needed. ~1 week.
- D5. Cloud-init / autounattend on Windows: should work cross-platform. Verify. ~3 days.
- D6. Backup/restore: works (uses qcow2). Verify on Windows file paths. ~2 days.
- D7. Container backend (Wave 12.9): podman.exe and docker.exe both ship for Windows. ~1 week.
- D8. First-run wizard: Windows-specific paths for system check (WHPX, QEMU, OVMF, libvirt, swtpm). ~1 week.
- D9. VM library discovery: VirtualBox `~/VirtualBox VMs/`, VMware `My Documents\Virtual Machines\`, Hyper-V `C:\ProgramData\Microsoft\Windows\Hyper-V\`. ~1 week.
- D10. Hyper-V VM import: parse `.vmcx` files (one-direction, read-only). ~2 weeks.

### Phase E: Polish & Launch (1 month)
- E1. End-to-end testing matrix: Windows 11 / Windows 10 / Server 2022 / Server 2019. ~1 week.
- E2. Performance benchmarking: WHPX vs KVM on same hardware (dual-boot). Document expected overhead. ~3 days.
- E3. Wayland-style "is the GUI native" check on Windows: ensure GPU acceleration via egui_wgpu works. ~3 days.
- E4. Bug bash + RC1. ~1 week.
- E5. Release announcement, video demo, blog post. ~3 days.

**Total: 8–11 months part-time, 5–7 months full-time.**

---

## 4. Strategic Bets in This Document

1. **One codebase, two backends.** Refuse the temptation to fork the Windows port. Every feature ships everywhere or nowhere.

2. **QEMU is the substrate everywhere.** Skip Hyper-V's native API. Skip VHDX. Skip Microsoft vTPM. Stay with the tools we already know.

3. **libvirt is the abstraction everywhere.** libvirt's Windows port is less polished but real. We help make it better by being a high-profile user.

4. **WHPX is the acceleration on Windows.** Don't ship without it; software emulation isn't competitive. Tell users to enable it as a hard requirement.

5. **Hyper-V is an *importer*, not a backend.** We read `.vmcx` and migrate VMs out of Hyper-V Manager. We don't compete with Hyper-V on its home turf.

6. **GPU passthrough is Linux-only for v1.** Windows users who need GPU passthrough should use the Linux host. Don't promise DDA support we can't maintain.

7. **macOS host support is a separate question.** Apple's Hypervisor.framework would need yet another backend. Defer until Windows ships.

8. **The Rust toolchain stays.** No P/Invoke surgery beyond what `windows-rs` already wraps. Stay in safe Rust where possible.

---

## 5. What This Document Doesn't Solve

These are real questions that need answers as the port progresses:

- **License management of bundled QEMU/libvirt/OVMF on Windows.** All GPL — we ship sources alongside, or link to upstream. Legal review needed.
- **Code signing certificate cost.** ~$300/year for a real cert. Worth it for installer trust.
- **The "Hyper-V vs WHPX" user education problem.** Many Windows users won't know which they have enabled. First-run wizard must handle this gracefully.
- **The Antivirus problem.** Windows Defender and third-party AV may flag QEMU.exe and KVM-like operations. Need allowlist guidance.
- **Network bridging on Windows.** Bridge mode requires a TAP driver. Either we ship one (vendor risk) or document third-party setup.
- **Update cadence.** Linux distributions have package managers; Windows users will need either the auto-updater or manual MSI reinstalls. Pick the model and stick to it.

---

## 6. Tracking

This document is the strategic plan. Tactical tasks for each phase land in [ROADMAP.md](ROADMAP.md) under new wave numbers:

- Wave 16: Foundation (Phase A)
- Wave 17: Windows Backend Core (Phase B)
- Wave 18: Build/Distribution (Phase C)
- Wave 19: Feature Parity Audit (Phase D)
- Wave 20: Polish & Launch (Phase E)

When a phase task ships, mark it here AND in ROADMAP.md.

When new Windows-specific features appear in VMware Workstation, audit them against [VMWARE-PARITY.md](VMWARE-PARITY.md) and add rows. The parity matrix becomes the contract: same features, both OSes.

---

*The Windows port is a multi-quarter commitment. It's worth doing because it eliminates the only honest reason a VMware Workstation user can't switch today.*
