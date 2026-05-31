# LibreUEFI: Guest OS UEFI Requirements Research

Comprehensive research on what different guest operating systems need from UEFI firmware
to work optimally in QEMU/KVM virtual machines managed by Libre VMM.

---

## Table of Contents

1. [Windows](#windows)
2. [BSD Family](#bsd-family)
3. [macOS (Hackintosh-style)](#macos-hackintosh-style)
4. [Linux](#linux)
5. [ChromeOS / Android](#chromeos--android)
6. [Alternative OSes](#alternative-oses)
7. [Cross-OS UEFI Features](#cross-os-uefi-features)
8. [LibreUEFI Implementation Priorities](#libreuefi-implementation-priorities)

---

## Windows

### SMBIOS Fields Read

| SMBIOS Type | Fields | Why |
|-------------|--------|-----|
| Type 1 (System) | Manufacturer, Product Name, Serial, UUID, SKU, Family | VM detection; OEM license activation via MSDM/SLIC matching |
| Type 2 (Baseboard) | Manufacturer, Product Name, Serial | Additional hardware identification |
| Type 3 (Chassis) | Type field | Determines laptop vs desktop behavior (battery UI, power plans) |
| Type 0 (BIOS) | Vendor, Version | Firmware identification |

**VM Detection**: Windows and many applications check Type 1 Manufacturer for strings like "QEMU", "KVM", "VirtualBox". LibreUEFI should allow configurable SMBIOS strings so users can set custom manufacturer/product values.

**License Activation**: Windows OEM licenses (SLIC/MSDM) require matching SMBIOS system info (UUID, Manufacturer, Product, Serial, SKU, Family) from the host. LibreUEFI needs the ability to pass through ACPI MSDM and SLIC tables from the host, plus matching SMBIOS Type 1 data. UUID matching is critical -- activation is bound to the UUID for approximately 180 days.

### ACPI Tables Required/Used

| Table | Purpose |
|-------|---------|
| DSDT/SSDT | Core device tree, power management, CPU topology |
| MSDM | OEM product key for offline activation (replaces older SLIC) |
| SLIC | Legacy OEM activation (Windows 7/8 era, some still use it) |
| BGRT | Boot Graphics Resource Table -- displays OEM logo during boot |
| WPBT | Windows Platform Binary Table -- pre-OS binary injection |
| FADT | Fixed ACPI Description Table -- power management profiles |
| MCFG | PCI Express memory-mapped config space |
| HPET | High Precision Event Timer |

**BGRT Details**: Windows reads the BGRT to display the OEM logo during boot, setup, recovery, and Secure Boot remediation. The image must be a 24-bit (0xRRGGBB) or 32-bit (0xrrRRGGBB) BMP stored in EfiBootServicesData. Background must be true black; no transparency support. LibreUEFI should provide a configurable BGRT so Libre VMM can show a custom boot logo per VM.

**WPBT Caution**: WPBT allows firmware to inject a Windows PE binary that runs at every boot. This is used by OEMs for anti-theft software but is a known persistence/rootkit vector. LibreUEFI should NOT implement WPBT by default but could offer it as an opt-in feature for specific enterprise use cases. The binary runs as `%systemroot%\system32\wpbbin.exe` with ntdll-only dependencies.

### Secure Boot Requirements

- **Mandatory for Windows 11** (installation will halt without it)
- Requires Microsoft certificates enrolled in db:
  - Microsoft Windows Production PCA 2011 (Windows boot loaders)
  - Microsoft Corporation UEFI CA 2011 (shim, PCI ROMs)
  - Microsoft Corporation KEK CA 2011 (in KEK)
- OVMF needs SMM (System Management Mode) enabled: `-machine q35,smm=on` + pflash secure property
- Pre-enrolled OVMF VARS templates (`.ms.fd` files) are the easiest approach
- Tools: `EnrollDefaultKeys.efi`, `ovmf-vars-generator`, `ovmfkeyenroll` (Python)

### TPM Requirements

- **TPM 2.0 mandatory for Windows 11**
- Emulated via `swtpm` with TIS or CRB model
- LibreUEFI itself does not provide TPM -- this is a QEMU device (`-tpm emulator`) but the firmware must expose the TPM event log and ACPI TPM2 table

### UEFI Variables

- Secure Boot variables: PK, KEK, db, dbx (authenticated variables requiring SMM protection)
- Boot order variables: Boot####, BootOrder, BootNext
- ConIn, ConOut, ErrOut (console configuration)
- OsIndicationsSupported / OsIndications (capsule update support flags)

### Display/GOP Requirements

- Windows requires GOP for UEFI boot; minimum 1024x768 for non-integrated displays
- Windows 7 requires legacy Int10h video BIOS shim (OVMF provides this)
- Known bug: first cold boot may lock to 800x600; VirtIO-GPU recommended for higher resolutions
- GOP resolution configurable via OVMF Platform Configuration in Device Manager

### WHQL/Certification Needs

- WHQL BIOS setting switches system to UEFI mode with Secure Boot expectations
- UEFI 2.3.1+ required for Windows 10 security features (Credential Guard, Exploit Guard)
- GOP mandatory for Windows 8+ client systems
- Platform firmware must ensure physical memory consistency across S4 transitions
- ESRT support needed for Windows firmware update platform

### Battery Emulation

- QEMU has NO native battery emulation (tracked as GitLab issue #242)
- Upstream patches exist but are not yet merged (QEMU 9+ patch by KBapna)
- Workaround: custom ACPI SSDT table with battery device
- Important for: laptop power plans, Nvidia GPU passthrough (error 43 workaround), preventing unnecessary background tasks in VMs on battery-powered hosts

### Known OVMF Issues

- Secure Boot requires SMM-enabled q35 machine type
- Some distros (Arch) don't ship pre-enrolled key OVMF images
- NVRAM corruption possible with certain macOS SMBIOS profiles (Thunderbolt firmware update)
- CVE-2025-2296: EFI stub direct boot bypassed Secure Boot verification (fixed in QEMU 10.0)

### What Would Make the VM Experience Better

1. **Configurable SMBIOS** with per-VM profiles (hide VM identity, pass host OEM data)
2. **MSDM/SLIC table passthrough** for OEM license activation
3. **Pre-enrolled Secure Boot keys** with easy custom key enrollment UI
4. **Custom BGRT boot logo** per VM (Libre VMM branding or user-specified)
5. **Virtual battery ACPI device** for power-aware behavior
6. **TPM 2.0 integration** with swtpm auto-setup
7. **Automated key enrollment** without manual EFI shell interaction

---

## BSD Family

### FreeBSD

**SMBIOS**: FreeBSD reads standard SMBIOS tables for hardware identification. No special requirements beyond standard compliance.

**ACPI Tables**: Standard DSDT, FADT, MADT, MCFG. FreeBSD has mature ACPI support. Uses ACPI for power management, CPU enumeration, and device discovery.

**Secure Boot**: FreeBSD does NOT have native Secure Boot support. No Microsoft-signed shim exists. Workaround: combine loader.efi + kernel into single signed EFI binary using `uefisign`. LibreUEFI should support custom key enrollment for FreeBSD users who want Secure Boot.

**TPM**: Not required for installation.

**UEFI Variables**: Standard boot variables. FreeBSD's `efibootmgr` manages boot entries.

**Display/GOP**: Standard GOP support. Works with OVMF's default GOP implementation.

**Known Issues**: No OVMF packages exist for FreeBSD as a host, but OVMF binaries are OS-agnostic so Linux-built OVMF works fine. QEMU without KVM may have issues with some OVMF versions.

**Improvements**: Straightforward UEFI boot; main need is easy custom Secure Boot key management for users who want it.

### OpenBSD

**SMBIOS**: Standard SMBIOS reading. No unusual requirements.

**ACPI**: OpenBSD is historically strict about ACPI compliance. Added hardware-reduced ACPI support in 5.9. The `efifb(4)` driver handles EFI framebuffer. ACPI compliance matters more for OpenBSD than most other BSDs -- malformed ACPI tables that other OSes ignore may cause issues.

**Secure Boot**: Not supported. OpenBSD boots via its own custom UEFI loader.

**TPM**: Not required.

**Display/GOP**: Uses EFI GOP framebuffer via `efifb(4)` driver. When using QEMU UEFI, the 'vesa' monitor type does not work -- must use EFI GOP (set `monitor=none` in plan9.ini-style configs or rely on GOP).

**Known Issues**: Specific EDK2/OVMF binary versions matter -- some pre-built binaries fail with OpenBSD. Architecture-specific: x86 UEFI requires ACPI, while aarch64 has different requirements.

**Improvements**: LibreUEFI should ensure strict ACPI table compliance for OpenBSD compatibility. Test with OpenBSD's ACPI validator.

### NetBSD

**SMBIOS/ACPI**: Standard requirements. NetBSD has good UEFI support.

**NVMM Hypervisor**: NetBSD uses NVMM (its own hypervisor) with QEMU. NVMM supports hardware-accelerated virtualization.

**Known Issues**: No significant UEFI-specific issues reported.

### DragonFlyBSD

**SMBIOS/ACPI**: Standard requirements.

**Known Issues**: NVMM+QEMU UEFI boot fails with pflash -- workaround uses `-bios` flag instead. The UEFI firmware ROM device mapping conflicts with NVMM guest memory mappings. This is a QEMU NVMM issue, not an OVMF issue, but LibreUEFI should document the workaround.

**Improvements for BSDs Overall**:
1. Strict ACPI table compliance (especially for OpenBSD)
2. Custom Secure Boot key enrollment workflow (no shim dependency)
3. Test matrix against all BSD variants
4. Document pflash vs -bios workaround for DragonFlyBSD/NVMM

---

## macOS (Hackintosh-style)

### SMBIOS Fields (Critical)

macOS is the most SMBIOS-sensitive OS. It reads extensive SMBIOS data to determine hardware identity, driver loading, power management, and service eligibility.

| Field | Purpose | Example Values |
|-------|---------|----------------|
| ProductName (hw.model) | Determines driver sets, GPU profiles, USB maps, CPU power management | iMacPro1,1 / MacPro7,1 |
| Board-ID | Platform identification, paired with ProductName | Mac-551B86E5744E2388 |
| Serial Number | Apple service eligibility, iMessage/FaceTime activation | Must follow Apple encoding format |
| MLB (Motherboard Serial) | Additional verification for Apple services | Unique per board |
| SmUUID (Hardware UUID) | Global device identifier | Standard UUID format |
| ROM (MAC address) | Part of Apple service authentication | Lowercase, no colons |
| ProcessorType | CPU identification in System Information | 0x0F01 for Xeon-W |

**Critical**: SMBIOS profile choice affects CPU power management, GPU profiles, USB maps, and feature availability. iMacPro1,1 is recommended for dGPU setups. iMac18,3 recommended with Clover (avoids Thunderbolt firmware NVRAM corruption).

### ACPI Tables

- Standard DSDT/SSDT for device tree
- macOS expects Apple-specific ACPI methods in some cases
- No MSDM/SLIC/BGRT needed (Apple doesn't use these)

### Secure Boot

- macOS has its own Apple Secure Boot mechanism, separate from UEFI Secure Boot
- Standard UEFI Secure Boot should be DISABLED for hackintosh VMs
- When creating OVMF for macOS, uncheck "Pre-Enroll Keys"

### TPM

- Not required and not used by macOS in VMs
- macOS uses its own T2/Apple Silicon security chip on real hardware

### Special Requirements

1. **Apple SMC emulation**: Mandatory. QEMU's `isa-applesmc` device with the Apple OSK authentication key
2. **CPU configuration**: Penryn is safest CPU model; must pass through specific CPUID features (+invtsc, +ssse3, +sse4.2, +popcnt, +avx, +aes, +xsave, +xsaveopt); `vendor=GenuineIntel` required; `kvm=on` flag needed
3. **Machine type**: Q35 mandatory
4. **KVM MSR ignore**: Host needs `echo 1 > /sys/module/kvm/parameters/ignore_msrs`
5. **OpenCore bootloader**: Required as intermediary between OVMF and macOS; provides SMBIOS injection, kernel patching, driver loading
6. **`-smbios type=2`**: Required in QEMU command line

### Display/GOP

- No native GPU acceleration without passthrough
- VMware-compatible graphics adapter for basic display
- VirtIO GPU does not work with macOS
- GPU passthrough (VFIO) needed for full acceleration

### Known OVMF Issues

- NVRAM corruption with certain SMBIOS profiles that trigger Thunderbolt firmware updates
- OpenCore handles most firmware compatibility issues as an intermediary layer
- Latest OVMF versions work without patching (older guides required patched OVMF)

### What Would Make the VM Experience Better

1. **Built-in Apple SMC emulation support** (documentation/integration with isa-applesmc)
2. **SMBIOS profile templates** for common Mac models (iMacPro1,1, MacPro7,1, etc.)
3. **Serial number generation tools** integrated into Libre VMM
4. **Automatic CPU flag configuration** based on selected macOS version
5. **OpenCore ISO management** with version tracking
6. **Warning system** for SMBIOS profiles known to cause NVRAM issues

---

## Linux

### SMBIOS Fields

Linux reads SMBIOS for hardware identification but has no strict requirements. The `dmidecode` tool exposes all SMBIOS data. Distributions may use SMBIOS for:
- VM detection (for guest agent auto-start)
- Hardware quirk matching
- Vendor-specific driver loading

### ACPI Tables

Standard tables: DSDT, SSDT, FADT, MADT, MCFG, HPET. Linux has the most tolerant ACPI parser -- it will work with slightly non-compliant tables that would break OpenBSD.

### Secure Boot

Linux has mature Secure Boot support via the shim chain:

**Boot Chain**: UEFI firmware -> shim (signed by Microsoft) -> GRUB (signed by distro) -> kernel (signed by distro)

**Key Components**:
- **Shim**: First-stage bootloader signed by Microsoft's UEFI CA. Each distro builds its own shim with embedded distro certificate.
- **MOK (Machine Owner Key)**: Allows users to enroll custom keys for signing their own kernels/modules. MOK Manager provides UI for key enrollment at boot.
- **Module-signing-only keys**: Special OID (1.3.6.1.4.1.2312.16.1.2) restricts keys to only sign kernel modules, not boot components.

**OVMF Requirements for Linux Secure Boot**:
- q35 machine type with SMM enabled
- Pflash drives with secure property enabled
- Pre-enrolled Microsoft keys (`.ms.fd` OVMF images) or manual enrollment
- QEMU 10.0+ fixes CVE-2025-2296 (EFI stub bypassing Secure Boot)
- New `-shim` QEMU flag in 10.0+ for proper shim-based boot

### Direct Kernel Boot (fw_cfg)

QEMU's `-kernel` flag enables fast boot by bypassing the bootloader entirely:
- QEMU exposes kernel via fw_cfg interface to OVMF
- OVMF fetches kernel, places in memory, launches it
- Supports both EFI stub and EFI handover protocol
- Add `-append` for kernel command line, `-initrd` for initramfs
- **Caveat**: Incompatible with Secure Boot (signature check fails); QEMU 10.0 added unmodified kernel exposure to fix this

### EFI Stub Boot

Linux kernel with `CONFIG_EFI_STUB=y` acts as an EFI application:
- Kernel can be loaded directly by UEFI firmware without GRUB
- Kernel file needs `.efi` extension on ESP
- Boot parameters stored in NVRAM via `efibootmgr`
- Ideal for minimal VM boot chains

### systemd-boot

Modern, minimal UEFI boot manager:
- Can be loaded via QEMU `-kernel` flag (works for any EFI binary)
- Reads Boot Loader Specification Type #1 entries from `/loader/entries/`
- Supports auto-detection of kernels on ESP
- Can load additional drivers from `/EFI/systemd/drivers/`
- Supports Secure Boot variable enrollment when firmware is in setup mode
- Supports Unified Kernel Images (UKIs)

### Battery/Power Management

Same as Windows: QEMU has no native battery emulation. Linux guests don't see battery information. The upstream QEMU patch (issue #242) exposes host battery state via ACPI SSDT with sysfs backend (`/sys/class/power_supply/`).

### Display/GOP

- Linux uses EFI framebuffer (`efifb` or `simplefb`) during early boot
- Once GPU drivers load, GOP is no longer needed
- VirtIO-GPU provides the best experience with `virtio-gpu` kernel driver
- `simpledrm` in newer kernels works with any GOP-provided framebuffer

### What Would Make the VM Experience Better

1. **Fast boot mode**: Direct kernel boot via fw_cfg for development/testing VMs
2. **Secure Boot profiles**: Pre-configured for major distros (Ubuntu, Fedora, Arch)
3. **systemd-boot integration**: Support for UKI boot entries
4. **MOK enrollment UI**: Simplified key management in Libre VMM
5. **Virtual battery**: For laptop hosts running Linux VMs
6. **Auto-detection**: Detect Linux ISO/install and suggest optimal boot config

---

## ChromeOS / Android

### ChromeOS

**UEFI Boot**: ChromeOS Flex supports UEFI boot but has specific requirements:
- Must uncheck "Pre-Enroll Keys" in OVMF EFI disk settings or "Access Denied" error occurs
- VirGL GPU recommended for display (change from default VGA)
- ChromiumOS images may have empty EFI System Partitions, causing UEFI boot failures
- `crosvm` (ChromeOS VMM, written in Rust) is the native hypervisor but is ChromeOS-specific

**SMBIOS/ACPI**: No unusual requirements beyond standard UEFI compliance.

**Secure Boot**: ChromeOS has its own verified boot mechanism separate from UEFI Secure Boot. Standard UEFI Secure Boot should generally be disabled.

**Known Issues**: Not all ChromiumOS builds are QEMU-compatible; need `amd64-generic` or `betty` flavors.

**Improvements**: Auto-detect ChromeOS Flex images and disable pre-enrolled keys automatically.

### Android x86

**UEFI Boot**: Android-x86 supports UEFI boot with OVMF:
- Standard OVMF pflash setup works
- GPT partitioning recommended
- ext4 filesystem for installation partition
- KVM acceleration strongly recommended for performance

**SMBIOS/ACPI**: Standard requirements. No unusual SMBIOS needs.

**Secure Boot**: Not required; Android x86 does not use UEFI Secure Boot.

**Display**: `-vga virtio` recommended for best display performance.

**Known Issues**: ARM apps won't run on x86; performance can be poor without KVM; display issues with some VGA modes.

**Improvements**:
1. Auto-detect Android x86 ISOs
2. Suggest optimal QEMU settings (virtio, KVM, memory)
3. Pre-configured display settings

---

## Alternative OSes

### ReactOS

**UEFI Support**: Initial UEFI boot support has been implemented in the development branch (not yet in a stable release). FreeLoader (ReactOS bootloader) has been updated to support UEFI on x86, AMD64, ARM32, and ARM64. Successfully booted on Steam Deck.

**Current Limitations**: UEFI boot requires serial port in current implementation. Covers approximately 85% of hardware. No stable release with UEFI yet.

**SMBIOS/ACPI**: As a Windows-compatible OS, ReactOS reads similar SMBIOS fields as Windows but with fewer strict requirements.

**Secure Boot**: Not supported.

**Improvements**: Offer both Legacy BIOS and UEFI boot options for ReactOS VMs; monitor upstream for stable UEFI release.

### Haiku OS

**UEFI Support**: Haiku supports UEFI booting. Works in QEMU with OVMF -- boots to desktop. The `haiku_loader.efi` must be placed at `/EFI/BOOT/BOOTX64.EFI` on the ESP.

**Known Issues**:
- Installer does not automatically install EFI bootloader (manual Terminal steps required)
- Some kernel panics reported on UEFI x86_64 installs
- BIOS boot with MBR is more reliable than GPT+UEFI currently

**SMBIOS/ACPI**: Standard requirements. No unusual needs.

**Secure Boot**: Not supported.

**Improvements**: Auto-detect Haiku ISOs; offer BIOS boot as default with UEFI as option.

### Plan 9 / 9front

**UEFI Support**: 9front has had UEFI boot support for several years. Boot files include `bootx64.efi` and `bootia32.efi`. Works with QEMU+OVMF.

**Special Considerations**:
- Cannot use 'vesa' monitor type with UEFI -- must use EFI GOP (`monitor=none` in plan9.ini)
- Serial console not supported by EFI bootloader
- Only supports 8.3 filenames in bootloader
- ACPI RSD pointer automatically passed by EFI bootloader
- OVMF VARS must be writable and per-VM (cannot share between VMs)

**SMBIOS/ACPI**: Minimal requirements. Plan 9 uses ACPI primarily for CPU enumeration.

**Secure Boot**: Not supported.

**Improvements**: Template with `monitor=none` preset; document GOP-only display mode.

### TempleOS

**UEFI Support**: NONE. TempleOS only supports Legacy BIOS boot.

**Requirements**:
- CSM/Legacy boot mode required
- Hardcoded 640x480 VGA mode
- IDE/ATA interface (or AHCI with TinkerOS fork)
- PS/2 keyboard and mouse

**LibreUEFI Implication**: TempleOS VMs must use SeaBIOS, not LibreUEFI. Libre VMM should auto-detect TempleOS and switch to BIOS mode.

---

## Cross-OS UEFI Features

### BGRT (Boot Graphics Resource Table)

**Specification**: ACPI spec section 5.2.22

**Technical Details**:
- Image stored in EfiBootServicesData memory
- Format: 24-bit BMP (0xRRGGBB) or 32-bit BMP (0xrrRRGGBB)
- Must have true black background (no transparency)
- Table contains Image Offset X/Y for positioning
- Status field indicates if image was displayed during boot

**OS Support**:
- Windows 8+: Uses BGRT for boot logo, setup, recovery, Secure Boot remediation
- Linux: Kernel reads BGRT, some distros display it during early boot
- Other OSes: Generally ignore BGRT

**LibreUEFI Implementation**:
- Provide configurable BGRT with default Libre VMM logo
- Allow per-VM custom boot logos
- Minimal padding in logo resource for proper Windows scaling
- U-Boot has a reference implementation that could guide development

### ESRT (EFI System Resource Table)

**Purpose**: Lists firmware components that can be updated via UEFI capsule mechanism. Each entry has a GUID, current version, and last update status.

**How It Works**:
1. ESRT entries describe updatable firmware components
2. OS matches capsule GUIDs against ESRT entries
3. Capsule staged to ESP or RAM
4. UpdateCapsule() runtime service initiates update
5. On reboot, firmware processes capsule (verify, decrypt, flash)

**Relevance to LibreUEFI**:
- Could enable firmware self-update mechanism for LibreUEFI
- Linux `fwupd` reads ESRT from `/sys/firmware/efi/esrt/entries/`
- Windows uses ESRT for its firmware update platform
- Initial implementation: expose ESRT with LibreUEFI version info
- Future: support capsule-based firmware updates within the VM

### UEFI Runtime Services

**Key Services for VMs**:

| Service | Purpose | VM Considerations |
|---------|---------|-------------------|
| GetVariable / SetVariable | Read/write UEFI variables | Backed by OVMF_VARS.fd pflash; writable, persistent |
| GetNextVariableName | Enumerate variables | Used by `efivarfs` in Linux |
| GetTime / SetTime | RTC access | QEMU provides emulated RTC |
| ResetSystem | System reset | Maps to QEMU reset/shutdown |
| UpdateCapsule | Firmware update | Could be used for LibreUEFI updates |
| QueryCapsuleCapabilities | Check update support | Should report LibreUEFI capabilities |

**Variable Storage**: OVMF uses split pflash (OVMF_CODE.fd read-only + OVMF_VARS.fd read-write). Each VM MUST have its own VARS copy. Variables with `EFI_VARIABLE_RUNTIME_ACCESS` remain accessible after ExitBootServices(). Authenticated variables (Secure Boot keys) require SMM protection.

**Linux Integration**: `efivarfs` mounted at `/sys/firmware/efi/efivars` provides userspace access to UEFI variables.

### Automated Secure Boot Key Enrollment

**Available Tools**:

| Tool | Method | Notes |
|------|--------|-------|
| EnrollDefaultKeys.efi | UEFI shell application | Ships with some OVMF packages; enrolls MS certs + distro-specific PK |
| ovmf-vars-generator | Script (rhuefi) | Generates VARS file with keys; uses expect + serial console |
| ovmfkeyenroll | Python (PyPI) | CLI tool; generates OVMF_VAR.sb.fd directly |
| Pre-enrolled templates | Distro packages | `.ms.fd` files (Fedora, Ubuntu, Debian) |
| Manual UI | OVMF Device Manager | Custom Mode -> enroll PK, KEK, db individually |

**Enrollment Order**: PK must be enrolled LAST (it transitions from Setup Mode to User Mode, activating Secure Boot).

**LibreUEFI Strategy**:
1. Ship with pre-enrolled Microsoft keys (for Windows/Linux Secure Boot)
2. Provide "setup mode" VARS for custom key enrollment
3. Integrate enrollment automation into Libre VMM (no manual EFI shell needed)
4. Support per-VM key databases
5. Include `EnrollDefaultKeys.efi` equivalent in LibreUEFI build

---

## LibreUEFI Implementation Priorities

### Tier 1: Essential (All VMs)

1. **Configurable SMBIOS injection** -- per-VM manufacturer, product, serial, UUID, SKU
2. **GOP display** -- configurable resolution, VirtIO-GPU support
3. **UEFI variable storage** -- per-VM pflash VARS with proper isolation
4. **Standard ACPI tables** -- DSDT, FADT, MADT, MCFG, HPET (strictly compliant for OpenBSD)
5. **Boot device management** -- configurable boot order, PXE, disk, CD-ROM

### Tier 2: Windows/Linux Optimization

6. **Secure Boot with pre-enrolled keys** -- Microsoft certs + automated enrollment
7. **TPM 2.0 integration** -- auto-setup swtpm, expose TPM2 ACPI table
8. **BGRT boot logo** -- configurable per VM with default Libre VMM branding
9. **MSDM/SLIC passthrough** -- for OEM Windows license activation
10. **SMM support** -- required for Secure Boot variable protection

### Tier 3: Advanced Features

11. **Direct kernel boot** -- fw_cfg integration for Linux fast boot
12. **ESRT table** -- expose firmware version, enable future self-update
13. **Virtual battery ACPI device** -- pass host battery state to guest
14. **Custom ACPI table injection** -- user-provided SSDT tables
15. **Apple SMC emulation support** -- documentation/integration for macOS VMs

### Tier 4: Guest-Specific Profiles

16. **macOS profile** -- SMBIOS templates, CPU flag presets, OpenCore guidance
17. **Windows 11 profile** -- Secure Boot + TPM auto-setup, SMBIOS customization
18. **BSD profile** -- strict ACPI compliance mode, custom key enrollment
19. **Legacy BIOS fallback** -- for TempleOS, older OSes (SeaBIOS integration)
20. **ChromeOS profile** -- auto-disable pre-enrolled keys, VirGL setup

---

## Sources

### Windows
- [Windows 11 on KVM with TPM and Secure Boot](https://insights.ditatompel.com/en/tutorials/run-windows-11-tpm-and-secure-boot-on-kvm/)
- [Building Windows 11 VM with QEMU TPM](https://macroform-node.medium.com/building-a-windows-11-vm-with-qemu-using-tpm-emulation-for-research-malware-analysis-part-1-8846378b9582)
- [HackBGRT - Boot logo changer](https://github.com/Metabolix/HackBGRT)
- [Microsoft Boot Screen Components](https://learn.microsoft.com/en-us/windows-hardware/drivers/bringup/boot-screen-components)
- [VM Detection bypass](https://github.com/SafeExamBrowser/seb-win-refactoring/issues/57)
- [SMBIOS for OEM license in libvirt](https://gist.github.com/Informatic/49bd034d43e054bd1d8d4fec38c305ec)
- [qemu-ovmf-secureboot](https://github.com/rhuefi/qemu-ovmf-secureboot)
- [libvirt Secure Boot](https://libvirt.org/kbase/secureboot.html)
- [Windows activation with MSDM on KVM](https://leduccc.medium.com/prevent-activation-issues-on-your-qemu-windows-guest-with-oem-windows-licenses-5bf03ecf513d)
- [MSDM/SLIC passthrough on Proxmox](https://dannyda.com/2025/06/08/how-to-passthrough-hardcoded-slic-msdm-oem-windows-license-to-vm-on-pve-proxmox-ve/)
- [dropWPBT](https://github.com/Jamesits/dropWPBT)
- [WPBT Builder](https://github.com/tandasat/WPBT-Builder)
- [QEMU Battery Patch](https://github.com/KBapna/QEMU-Battery-Support-Patch)
- [QEMU Battery Issue #242](https://gitlab.com/qemu-project/qemu/-/issues/242)
- [Microsoft UEFI firmware requirements](https://learn.microsoft.com/en-us/windows-hardware/design/device-experiences/oem-uefi)

### BSD
- [FreeBSD UEFI with OVMF](https://forums.freebsd.org/threads/booting-uefi-with-ovmf.54492/)
- [FreeBSD UEFI Secure Boot](https://freebsdfoundation.org/freebsd-uefi-secure-boot/)
- [Booting OpenBSD in EFI mode with QEMU](https://www.cambus.net/booting-openbsd-kernels-in-efi-mode-with-qemu/)
- [NetBSD Virtualization Guide](https://www.netbsd.org/docs/guide/en/chap-virt.html)
- [DragonFlyBSD NVMM UEFI bug](https://bugs.dragonflybsd.org/issues/3310)

### macOS
- [OSX-KVM](https://github.com/kholia/OSX-KVM)
- [OpenCore Install Guide - SMBIOS](https://dortania.github.io/OpenCore-Install-Guide/extras/smbios-support.html)
- [KVM-Opencore](https://github.com/Leoyzen/KVM-Opencore)
- [OpenCore-ISO for Proxmox](https://github.com/LongQT-sea/OpenCore-ISO)

### Linux
- [Linux EFI Stub Documentation](https://docs.kernel.org/admin-guide/efi-stub.html)
- [QEMU fw_cfg Specification](https://www.qemu.org/docs/master/specs/fw_cfg.html)
- [systemd-boot Documentation](https://www.freedesktop.org/software/systemd/man/latest/systemd-boot.html)
- [Ubuntu Secure Boot Documentation](https://documentation.ubuntu.com/security/security-features/platform-protections/secure-boot/)
- [Arch Wiki - Secure Boot](https://wiki.archlinux.org/title/Unified_Extensible_Firmware_Interface/Secure_Boot)
- [Debian SecureBoot VM](https://wiki.debian.org/SecureBoot/VirtualMachine)

### ChromeOS / Android
- [crosvm](https://github.com/google/crosvm)
- [ChromeOS Flex on Proxmox](https://kevindavid.org/code/2024/03/20/chrome-os-flex-proxmox.html)
- [Android-x86 QEMU HowTo](https://www.android-x86.org/documentation/qemu.html)

### Alternative OSes
- [ReactOS UEFI boot](https://www.osnews.com/story/137072/reactos-gets-support-for-uefi-booting/)
- [Haiku UEFI Booting Guide](https://www.haiku-os.org/guides/uefi_booting/)
- [9front OVMF wiki](https://wiki.9front.org/OVMF)
- [TempleOS Boot Documentation](https://templeos.info/Wb/Doc/Boot.DD.HTML)

### Cross-OS UEFI
- [UEFI Spec - Runtime Services](https://uefi.org/specs/UEFI/2.10/08_Services_Runtime_Services.html)
- [UEFI Spec - Firmware Update (ESRT)](https://uefi.org/specs/UEFI/2.10_A/23_Firmware_Update_and_Reporting.html)
- [ovmfkeyenroll](https://pypi.org/project/ovmfkeyenroll/)
- [openSUSE Secure Boot with qemu-kvm](https://en.opensuse.org/openSUSE:UEFI_Secure_boot_using_qemu-kvm)
- [OVMF Fedora viewport resolution](https://developer.fedoraproject.org/tools/virtualization/setting-viewport-resolution-using-ovmf-bios.html)
