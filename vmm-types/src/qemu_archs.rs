//! Pure data types from `vmm-core/src/qemu_archs.rs`. No I/O, no platform code.
//!
//! ## Wave 16.A1 (Windows port foundation)
//! Extracted from vmm-core so vmm-gui can compile on any platform.
//!
//! QEMU architecture definitions — machine types, CPU models, and device defaults.
//!
//! Inspired by UTM's approach: architecture → machine type → device defaults.
//! This module provides the data model for Box 2 (Hardware Lab) cross-architecture
//! emulation, supporting all major QEMU target architectures.
//!
//! ## What stays in vmm-core::qemu_archs
//!
//! - `qemu_binary()` (hardcoded `/usr/bin/qemu-system-…` path is Linux-specific)
//! - `is_binary_available()` (touches the filesystem)
//! - `detect_installed_architectures()` (filesystem)
//! - `query_machine_types()` (spawns the QEMU process)

use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// Architecture enum — all QEMU system targets
// ─────────────────────────────────────────────────────────────────────────────

/// QEMU target architecture. Each variant maps to a `qemu-system-{arch}` binary.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QemuArch {
    X86_64,
    I386,
    Aarch64,
    Arm,
    Riscv64,
    Riscv32,
    Ppc,
    Ppc64,
    S390x,
    Mips,
    Mipsel,
    Mips64,
    Mips64el,
    Sparc,
    Sparc64,
    M68k,
    Alpha,
    Hppa,
    Loongarch64,
    Xtensa,
    Avr,
    Or1k,
    Sh4,
    Microblaze,
}

impl Default for QemuArch {
    fn default() -> Self {
        QemuArch::X86_64
    }
}

impl QemuArch {
    /// All architectures, ordered by popularity / usefulness.
    pub fn all() -> Vec<QemuArch> {
        vec![
            QemuArch::X86_64,
            QemuArch::Aarch64,
            QemuArch::Arm,
            QemuArch::Riscv64,
            QemuArch::Riscv32,
            QemuArch::Ppc64,
            QemuArch::Ppc,
            QemuArch::Mips64el,
            QemuArch::Mips64,
            QemuArch::Mipsel,
            QemuArch::Mips,
            QemuArch::S390x,
            QemuArch::Sparc64,
            QemuArch::Sparc,
            QemuArch::M68k,
            QemuArch::I386,
            QemuArch::Alpha,
            QemuArch::Hppa,
            QemuArch::Loongarch64,
            QemuArch::Xtensa,
            QemuArch::Sh4,
            QemuArch::Or1k,
            QemuArch::Microblaze,
            QemuArch::Avr,
        ]
    }

    /// Primary architectures that most users will care about.
    pub fn common() -> Vec<QemuArch> {
        vec![
            QemuArch::X86_64,
            QemuArch::Aarch64,
            QemuArch::Arm,
            QemuArch::Riscv64,
            QemuArch::Ppc64,
            QemuArch::Mips64el,
            QemuArch::S390x,
            QemuArch::I386,
        ]
    }

    /// QEMU binary suffix (e.g., `qemu-system-x86_64`).
    pub fn qemu_suffix(&self) -> &str {
        match self {
            QemuArch::X86_64 => "x86_64",
            QemuArch::I386 => "i386",
            QemuArch::Aarch64 => "aarch64",
            QemuArch::Arm => "arm",
            QemuArch::Riscv64 => "riscv64",
            QemuArch::Riscv32 => "riscv32",
            QemuArch::Ppc => "ppc",
            QemuArch::Ppc64 => "ppc64",
            QemuArch::S390x => "s390x",
            QemuArch::Mips => "mips",
            QemuArch::Mipsel => "mipsel",
            QemuArch::Mips64 => "mips64",
            QemuArch::Mips64el => "mips64el",
            QemuArch::Sparc => "sparc",
            QemuArch::Sparc64 => "sparc64",
            QemuArch::M68k => "m68k",
            QemuArch::Alpha => "alpha",
            QemuArch::Hppa => "hppa",
            QemuArch::Loongarch64 => "loongarch64",
            QemuArch::Xtensa => "xtensa",
            QemuArch::Avr => "avr",
            QemuArch::Or1k => "or1k",
            QemuArch::Sh4 => "sh4",
            QemuArch::Microblaze => "microblaze",
        }
    }

    /// Human-readable display name.
    pub fn display_name(&self) -> &str {
        match self {
            QemuArch::X86_64 => "x86_64 (PC/Intel/AMD)",
            QemuArch::I386 => "i386 (32-bit PC)",
            QemuArch::Aarch64 => "ARM64 (aarch64)",
            QemuArch::Arm => "ARM (aarch32)",
            QemuArch::Riscv64 => "RISC-V 64-bit",
            QemuArch::Riscv32 => "RISC-V 32-bit",
            QemuArch::Ppc => "PowerPC 32-bit",
            QemuArch::Ppc64 => "PowerPC 64-bit",
            QemuArch::S390x => "IBM z/Architecture (s390x)",
            QemuArch::Mips => "MIPS (big-endian)",
            QemuArch::Mipsel => "MIPS (little-endian)",
            QemuArch::Mips64 => "MIPS64 (big-endian)",
            QemuArch::Mips64el => "MIPS64 (little-endian)",
            QemuArch::Sparc => "SPARC 32-bit",
            QemuArch::Sparc64 => "SPARC 64-bit (UltraSPARC)",
            QemuArch::M68k => "Motorola 68000",
            QemuArch::Alpha => "DEC Alpha",
            QemuArch::Hppa => "HP PA-RISC",
            QemuArch::Loongarch64 => "LoongArch 64-bit",
            QemuArch::Xtensa => "Xtensa",
            QemuArch::Avr => "AVR (Atmel/Microchip)",
            QemuArch::Or1k => "OpenRISC",
            QemuArch::Sh4 => "SuperH SH-4",
            QemuArch::Microblaze => "Xilinx MicroBlaze",
        }
    }

    /// Short category description.
    pub fn category(&self) -> &str {
        match self {
            QemuArch::X86_64 | QemuArch::I386 => "Desktop / Server",
            QemuArch::Aarch64 | QemuArch::Arm => "Mobile / Embedded / Server",
            QemuArch::Riscv64 | QemuArch::Riscv32 => "Emerging",
            QemuArch::Ppc | QemuArch::Ppc64 => "Legacy Server / Workstation",
            QemuArch::S390x => "Mainframe",
            QemuArch::Mips | QemuArch::Mipsel | QemuArch::Mips64 | QemuArch::Mips64el => {
                "Embedded / Networking"
            },
            QemuArch::Sparc | QemuArch::Sparc64 => "Legacy Server",
            QemuArch::M68k => "Retro Computing",
            QemuArch::Alpha => "Retro Computing",
            QemuArch::Hppa => "Retro Computing",
            QemuArch::Loongarch64 => "Emerging",
            QemuArch::Xtensa
            | QemuArch::Avr
            | QemuArch::Or1k
            | QemuArch::Sh4
            | QemuArch::Microblaze => "Microcontroller / Specialty",
        }
    }

    /// Whether this architecture can use KVM on x86_64 hosts (same-arch virtualization).
    pub fn can_use_kvm_on_x86(&self) -> bool {
        matches!(self, QemuArch::X86_64 | QemuArch::I386)
    }

    /// Bit width of the architecture.
    pub fn bits(&self) -> u8 {
        match self {
            QemuArch::X86_64
            | QemuArch::Aarch64
            | QemuArch::Riscv64
            | QemuArch::Ppc64
            | QemuArch::S390x
            | QemuArch::Mips64
            | QemuArch::Mips64el
            | QemuArch::Sparc64
            | QemuArch::Alpha
            | QemuArch::Hppa
            | QemuArch::Loongarch64 => 64,
            QemuArch::I386
            | QemuArch::Arm
            | QemuArch::Riscv32
            | QemuArch::Ppc
            | QemuArch::Mips
            | QemuArch::Mipsel
            | QemuArch::Sparc
            | QemuArch::M68k
            | QemuArch::Or1k
            | QemuArch::Sh4
            | QemuArch::Microblaze
            | QemuArch::Xtensa => 32,
            QemuArch::Avr => 8,
        }
    }

    /// Whether UEFI firmware is typically available for this architecture.
    pub fn has_uefi_support(&self) -> bool {
        matches!(self, QemuArch::X86_64 | QemuArch::I386 | QemuArch::Aarch64)
    }

    /// Whether this architecture supports USB controllers.
    pub fn has_usb_support(&self) -> bool {
        !matches!(
            self,
            QemuArch::S390x
                | QemuArch::Sparc
                | QemuArch::Sparc64
                | QemuArch::Avr
                | QemuArch::Or1k
                | QemuArch::Microblaze
        )
    }

    /// Whether this architecture supports audio devices.
    pub fn has_audio_support(&self) -> bool {
        matches!(
            self,
            QemuArch::X86_64
                | QemuArch::I386
                | QemuArch::Aarch64
                | QemuArch::Ppc
                | QemuArch::Ppc64
                | QemuArch::M68k
        )
    }

    /// Whether this architecture supports SPICE display.
    pub fn has_spice_support(&self) -> bool {
        matches!(
            self,
            QemuArch::X86_64
                | QemuArch::I386
                | QemuArch::Aarch64
                | QemuArch::Arm
                | QemuArch::Ppc
                | QemuArch::Ppc64
        )
    }

    /// Whether this architecture supports VirtIO devices.
    pub fn has_virtio_support(&self) -> bool {
        !matches!(
            self,
            QemuArch::Avr | QemuArch::Or1k | QemuArch::Sh4 | QemuArch::Microblaze
        )
    }

    /// Default machine type for this architecture.
    pub fn default_machine(&self) -> &str {
        match self {
            QemuArch::X86_64 | QemuArch::I386 => "q35",
            QemuArch::Aarch64 | QemuArch::Arm => "virt",
            QemuArch::Riscv64 | QemuArch::Riscv32 => "virt",
            QemuArch::Ppc => "g3beige",
            QemuArch::Ppc64 => "pseries",
            QemuArch::S390x => "s390-ccw-virtio",
            QemuArch::Mips | QemuArch::Mipsel | QemuArch::Mips64 | QemuArch::Mips64el => "malta",
            QemuArch::Sparc => "SS-5",
            QemuArch::Sparc64 => "sun4u",
            QemuArch::M68k => "virt",
            QemuArch::Alpha => "clipper",
            QemuArch::Hppa => "hppa",
            QemuArch::Loongarch64 => "virt",
            QemuArch::Xtensa => "sim",
            QemuArch::Avr => "mega2560",
            QemuArch::Or1k => "or1k-sim",
            QemuArch::Sh4 => "shix",
            QemuArch::Microblaze => "petalogix-s3adsp1800",
        }
    }

    /// Available machine types for this architecture.
    pub fn machine_types(&self) -> Vec<MachineType> {
        match self {
            QemuArch::X86_64 | QemuArch::I386 => vec![
                MachineType::new("q35", "Standard PC (Q35 + ICH9, 2009)", true),
                MachineType::new("pc", "Standard PC (i440FX + PIIX, 1996)", false),
                MachineType::new("microvm", "Minimal microVM", false),
                MachineType::new("isapc", "ISA-only PC (legacy)", false),
            ],
            QemuArch::Aarch64 => vec![
                MachineType::new("virt", "ARM Virtual Machine (recommended)", true),
                MachineType::new("sbsa-ref", "SBSA Reference Platform", false),
                MachineType::new("raspi3b", "Raspberry Pi 3B", false),
                MachineType::new("raspi4b", "Raspberry Pi 4B", false),
                MachineType::new("vexpress-a15", "ARM Versatile Express A15", false),
                MachineType::new("vexpress-a9", "ARM Versatile Express A9", false),
            ],
            QemuArch::Arm => vec![
                MachineType::new("virt", "ARM Virtual Machine (recommended)", true),
                MachineType::new("raspi2b", "Raspberry Pi 2B", false),
                MachineType::new("vexpress-a15", "ARM Versatile Express A15", false),
                MachineType::new("vexpress-a9", "ARM Versatile Express A9", false),
                MachineType::new("integratorcp", "ARM Integrator/CP", false),
                MachineType::new("versatilepb", "ARM Versatile Platform Board", false),
            ],
            QemuArch::Riscv64 => vec![
                MachineType::new("virt", "RISC-V Virtual Platform (recommended)", true),
                MachineType::new("spike", "RISC-V Spike Simulator", false),
                MachineType::new("sifive_u", "SiFive HiFive Unleashed", false),
                MachineType::new("sifive_e", "SiFive HiFive1 rev B", false),
                MachineType::new("microchip-icicle-kit", "Microchip PolarFire SoC", false),
            ],
            QemuArch::Riscv32 => vec![
                MachineType::new("virt", "RISC-V Virtual Platform (recommended)", true),
                MachineType::new("spike", "RISC-V Spike Simulator", false),
                MachineType::new("sifive_e", "SiFive HiFive1 rev B", false),
                MachineType::new("sifive_u", "SiFive HiFive Unleashed", false),
            ],
            QemuArch::Ppc => vec![
                MachineType::new("g3beige", "Heathrow (Power Mac G3 Beige)", true),
                MachineType::new("mac99", "Mac99 (Power Mac G4)", false),
                MachineType::new("ppce500", "Generic e500 PowerPC", false),
                MachineType::new("sam460ex", "aCube Sam460ex", false),
                MachineType::new("bamboo", "IBM Bamboo (PPC440EP)", false),
            ],
            QemuArch::Ppc64 => vec![
                MachineType::new("pseries", "pSeries (PAPR Logical Partition)", true),
                MachineType::new("powernv", "IBM PowerNV (baremetal)", false),
                MachineType::new("mac99", "Mac99 (Power Mac G5)", false),
                MachineType::new("g3beige", "Heathrow (Power Mac G3 Beige)", false),
                MachineType::new("ppce500", "Generic e500 PowerPC", false),
            ],
            QemuArch::S390x => vec![MachineType::new(
                "s390-ccw-virtio",
                "S390 CCW VirtIO (recommended)",
                true,
            )],
            QemuArch::Mips | QemuArch::Mipsel => vec![
                MachineType::new("malta", "MIPS Malta (recommended)", true),
                MachineType::new("mipssim", "MIPS Simulator", false),
            ],
            QemuArch::Mips64 | QemuArch::Mips64el => vec![
                MachineType::new("malta", "MIPS Malta (recommended)", true),
                MachineType::new("boston", "MIPS Boston (Imagination)", false),
                MachineType::new("loongson3-virt", "Loongson 3A5000 Virtual", false),
                MachineType::new("fuloong2e", "Lemote Fuloong 2E", false),
                MachineType::new("mipssim", "MIPS Simulator", false),
            ],
            QemuArch::Sparc => vec![
                MachineType::new("SS-5", "Sun SPARCstation 5", true),
                MachineType::new("SS-10", "Sun SPARCstation 10", false),
                MachineType::new("SS-20", "Sun SPARCstation 20", false),
                MachineType::new("leon3_generic", "LEON3 (space-grade)", false),
            ],
            QemuArch::Sparc64 => vec![
                MachineType::new("sun4u", "Sun UltraSPARC T1 (sun4u)", true),
                MachineType::new("sun4v", "Sun UltraSPARC T2 (sun4v)", false),
                MachineType::new("niagara", "Sun Niagara", false),
            ],
            QemuArch::M68k => vec![
                MachineType::new("virt", "M68k Virtual Machine (recommended)", true),
                MachineType::new("q800", "Apple Macintosh Quadra 800", false),
                MachineType::new("next-cube", "NeXT Cube", false),
                MachineType::new("mcf5208evb", "Freescale ColdFire 5208", false),
                MachineType::new("an5206", "Arnewsh 5206 Board", false),
            ],
            QemuArch::Alpha => vec![MachineType::new(
                "clipper",
                "DEC Clipper (recommended)",
                true,
            )],
            QemuArch::Hppa => vec![MachineType::new("hppa", "HP PA-RISC (recommended)", true)],
            QemuArch::Loongarch64 => {
                vec![MachineType::new("virt", "LoongArch Virtual Machine", true)]
            },
            QemuArch::Xtensa => vec![
                MachineType::new("sim", "Xtensa Simulator", true),
                MachineType::new("virt", "Xtensa Virtual Machine", false),
                MachineType::new("lx60", "Avnet LX60 Board", false),
            ],
            QemuArch::Avr => vec![
                MachineType::new("mega2560", "Arduino Mega 2560", true),
                MachineType::new("mega", "AVR ATmega Simulator", false),
            ],
            QemuArch::Or1k => vec![
                MachineType::new("or1k-sim", "OpenRISC Simulator", true),
                MachineType::new("virt", "OpenRISC Virtual Machine", false),
            ],
            QemuArch::Sh4 => vec![
                MachineType::new("shix", "Shix Board (SH7750)", true),
                MachineType::new("r2d", "Renesas R2D-PLUS SH7751R", false),
            ],
            QemuArch::Microblaze => vec![
                MachineType::new("petalogix-s3adsp1800", "PetaLogix S3ADSP1800", true),
                MachineType::new("petalogix-ml605", "PetaLogix ML605 Reference", false),
            ],
        }
    }

    /// Default CPU model for this architecture.
    pub fn default_cpu(&self) -> &str {
        match self {
            QemuArch::X86_64 => "qemu64",
            QemuArch::I386 => "qemu32",
            QemuArch::Aarch64 => "cortex-a72",
            QemuArch::Arm => "cortex-a15",
            QemuArch::Riscv64 | QemuArch::Riscv32 => "rv64",
            QemuArch::Ppc => "604",
            QemuArch::Ppc64 => "power9_v2.0",
            QemuArch::S390x => "qemu",
            QemuArch::Mips | QemuArch::Mipsel => "mips32r2-generic",
            QemuArch::Mips64 | QemuArch::Mips64el => "mips64r2-generic",
            QemuArch::Sparc => "Fujitsu-MB86907",
            QemuArch::Sparc64 => "Sun-UltraSparc-IIIi",
            QemuArch::M68k => "m68040",
            QemuArch::Alpha => "ev67",
            QemuArch::Hppa => "hppa",
            QemuArch::Loongarch64 => "la464",
            QemuArch::Xtensa => "dc233c",
            QemuArch::Avr => "avr6-avr-cpu",
            QemuArch::Or1k => "any",
            QemuArch::Sh4 => "sh7750r",
            QemuArch::Microblaze => "any",
        }
    }

    /// Available CPU models for this architecture.
    pub fn cpu_models(&self) -> Vec<CpuModel> {
        match self {
            QemuArch::X86_64 => vec![
                CpuModel::new("host", "Host CPU (KVM only, best performance)", true),
                CpuModel::new("max", "Maximum capabilities for TCG", false),
                CpuModel::new("qemu64", "QEMU Virtual CPU v2.5+", false),
                CpuModel::new("Skylake-Client-v4", "Intel Skylake Client", false),
                CpuModel::new("Cascadelake-Server-v5", "Intel Cascadelake Server", false),
                CpuModel::new("EPYC-v4", "AMD EPYC", false),
                CpuModel::new("Westmere-v2", "Intel Westmere", false),
                CpuModel::new("Nehalem-v2", "Intel Nehalem", false),
                CpuModel::new("SandyBridge-v2", "Intel Sandy Bridge", false),
                CpuModel::new("IvyBridge-v2", "Intel Ivy Bridge", false),
                CpuModel::new("Haswell-v4", "Intel Haswell", false),
                CpuModel::new("Broadwell-v4", "Intel Broadwell", false),
                CpuModel::new("Cooperlake-v2", "Intel Cooperlake", false),
                CpuModel::new("Icelake-Server-v6", "Intel Icelake Server", false),
                CpuModel::new("SapphireRapids-v2", "Intel Sapphire Rapids", false),
                CpuModel::new("EPYC-Rome-v3", "AMD EPYC Rome", false),
                CpuModel::new("EPYC-Milan-v2", "AMD EPYC Milan", false),
                CpuModel::new("EPYC-Genoa-v1", "AMD EPYC Genoa", false),
            ],
            QemuArch::I386 => vec![
                CpuModel::new("host", "Host CPU (KVM only)", true),
                CpuModel::new("max", "Maximum capabilities", false),
                CpuModel::new("qemu32", "QEMU Virtual CPU v2.5+", false),
                CpuModel::new("pentium3", "Intel Pentium III", false),
                CpuModel::new("pentium2", "Intel Pentium II", false),
                CpuModel::new("coreduo", "Intel Core Duo", false),
                CpuModel::new("486", "Intel 486", false),
            ],
            QemuArch::Aarch64 => vec![
                CpuModel::new("host", "Host CPU (KVM only)", true),
                CpuModel::new("max", "Maximum capabilities for TCG", false),
                CpuModel::new("cortex-a72", "ARM Cortex-A72 (common server)", false),
                CpuModel::new("cortex-a76", "ARM Cortex-A76", false),
                CpuModel::new("cortex-a57", "ARM Cortex-A57", false),
                CpuModel::new("cortex-a53", "ARM Cortex-A53", false),
                CpuModel::new("neoverse-n1", "ARM Neoverse N1 (server)", false),
                CpuModel::new("neoverse-v1", "ARM Neoverse V1", false),
                CpuModel::new("a64fx", "Fujitsu A64FX (HPC)", false),
            ],
            QemuArch::Arm => vec![
                CpuModel::new("max", "Maximum capabilities", true),
                CpuModel::new("cortex-a15", "ARM Cortex-A15", false),
                CpuModel::new("cortex-a9", "ARM Cortex-A9", false),
                CpuModel::new("cortex-a8", "ARM Cortex-A8", false),
                CpuModel::new("cortex-a7", "ARM Cortex-A7", false),
                CpuModel::new("cortex-m4", "ARM Cortex-M4 (microcontroller)", false),
                CpuModel::new("cortex-m3", "ARM Cortex-M3 (microcontroller)", false),
            ],
            QemuArch::Riscv64 => vec![
                CpuModel::new("rv64", "Generic RISC-V 64-bit", true),
                CpuModel::new("sifive-u54", "SiFive U54", false),
                CpuModel::new("sifive-e51", "SiFive E51", false),
                CpuModel::new("veyron-v1", "T-Head C910 Veyron", false),
            ],
            QemuArch::Riscv32 => vec![
                CpuModel::new("rv32", "Generic RISC-V 32-bit", true),
                CpuModel::new("sifive-e34", "SiFive E34", false),
                CpuModel::new("ibex", "lowRISC Ibex", false),
            ],
            QemuArch::Ppc => vec![
                CpuModel::new("604", "PowerPC 604 (default)", true),
                CpuModel::new("g3", "PowerPC G3 (750)", false),
                CpuModel::new("g4", "PowerPC G4 (7400)", false),
                CpuModel::new("e500v2", "Freescale e500v2", false),
            ],
            QemuArch::Ppc64 => vec![
                CpuModel::new("host", "Host CPU (KVM only)", true),
                CpuModel::new("power9_v2.0", "IBM POWER9 v2.0", false),
                CpuModel::new("power10_v2.0", "IBM POWER10 v2.0", false),
                CpuModel::new("power8_v2.0", "IBM POWER8 v2.0", false),
                CpuModel::new("970mp_v1.0", "PowerPC 970MP (G5)", false),
            ],
            QemuArch::S390x => vec![
                CpuModel::new("host", "Host CPU (KVM only)", true),
                CpuModel::new("qemu", "QEMU Virtual CPU", false),
                CpuModel::new("gen16b-base", "IBM z16 (base)", false),
                CpuModel::new("gen15b-base", "IBM z15 (base)", false),
            ],
            _ => vec![CpuModel::new(self.default_cpu(), "Default CPU", true)],
        }
    }

    /// Default display device for this architecture + machine combination.
    pub fn default_display_device(&self, machine: &str) -> &str {
        match (self, machine) {
            (QemuArch::X86_64 | QemuArch::I386, m) if m.starts_with("q35") || m == "q35" => {
                "virtio-vga"
            },
            (QemuArch::X86_64 | QemuArch::I386, "pc") => "cirrus-vga",
            (QemuArch::X86_64 | QemuArch::I386, "isapc") => "isa-vga",
            (QemuArch::Aarch64 | QemuArch::Arm, "virt") => "virtio-gpu-pci",
            (QemuArch::Aarch64, m) if m.starts_with("raspi") => "bcm2835-fb",
            (QemuArch::Ppc | QemuArch::Ppc64, "mac99") => "ati-vga",
            (QemuArch::Ppc64, "pseries") => "virtio-vga",
            (QemuArch::M68k, "q800") => "nubus-macfb",
            (QemuArch::Sparc, _) => "cg3", // TCX framebuffer
            (QemuArch::Sparc64, _) => "sunhme",
            _ => "none",
        }
    }

    /// Default network device for this architecture + machine combination.
    pub fn default_network_device(&self, machine: &str) -> &str {
        match (self, machine) {
            (QemuArch::X86_64, m) if m.starts_with("q35") || m == "q35" => "e1000",
            (QemuArch::X86_64, "pc") => "rtl8139",
            (QemuArch::X86_64, "isapc") => "ne2k_isa",
            (QemuArch::I386, m) if m.starts_with("q35") || m == "q35" => "e1000",
            (QemuArch::I386, _) => "ne2k_isa",
            (QemuArch::Aarch64 | QemuArch::Arm, "virt") => "virtio-net-pci",
            (QemuArch::Riscv64 | QemuArch::Riscv32, "virt") => "virtio-net-pci",
            (QemuArch::Ppc, "mac99") => "sungem",
            (QemuArch::Ppc | QemuArch::Ppc64, "g3beige") => "e1000",
            (QemuArch::Ppc64, "pseries") => "virtio-net-pci",
            (QemuArch::S390x, _) => "virtio-net-ccw",
            (
                QemuArch::Mips | QemuArch::Mipsel | QemuArch::Mips64 | QemuArch::Mips64el,
                "malta",
            ) => "e1000",
            (QemuArch::Sparc, _) => "lance",
            (QemuArch::Sparc64, _) => "sunhme",
            (QemuArch::M68k, "q800") => "dp8393x",
            (QemuArch::M68k, "virt") => "virtio-net-pci",
            (QemuArch::Alpha, _) => "e1000",
            _ => "e1000",
        }
    }

    /// Default sound model for libvirt `<sound model="...">`.
    /// Returns libvirt-compatible model names (ich9, ac97, sb16, etc.).
    pub fn default_sound_device(&self, machine: &str) -> Option<&str> {
        match (self, machine) {
            // Q35 machines use Intel HD Audio (ICH9)
            (QemuArch::X86_64 | QemuArch::I386, m) if m.starts_with("q35") => Some("ich9"),
            // Legacy PC uses AC97
            (QemuArch::X86_64 | QemuArch::I386, "pc") => Some("ac97"),
            // ISA-only machines use SoundBlaster 16
            (QemuArch::I386, "isapc") => Some("sb16"),
            // AArch64 virt uses ICH9 (Intel HDA emulation)
            (QemuArch::Aarch64, "virt") => Some("ich9"),
            _ => None,
        }
    }

    /// Default disk bus for this architecture + machine combination.
    pub fn default_disk_bus(&self, machine: &str) -> &str {
        match (self, machine) {
            (QemuArch::Aarch64 | QemuArch::Arm, "virt") => "virtio",
            (QemuArch::X86_64 | QemuArch::I386, _) => "virtio",
            (QemuArch::Riscv64 | QemuArch::Riscv32, "virt") => "virtio",
            (QemuArch::Ppc64, "pseries") => "virtio",
            (QemuArch::S390x, _) => "virtio",
            (QemuArch::M68k, "virt") => "virtio",
            (QemuArch::Loongarch64, _) => "virtio",
            // SCSI for legacy machines
            (QemuArch::Ppc | QemuArch::Ppc64, _) => "scsi",
            (QemuArch::Sparc | QemuArch::Sparc64, _) => "scsi",
            (QemuArch::Alpha, _) => "scsi",
            // IDE fallback
            _ => "ide",
        }
    }

    /// UEFI firmware path, if available.
    pub fn uefi_firmware_path(&self) -> Option<(&str, &str)> {
        match self {
            QemuArch::X86_64 | QemuArch::I386 => Some((
                "/usr/share/OVMF/OVMF_CODE_4M.fd",
                "/usr/share/OVMF/OVMF_VARS_4M.fd",
            )),
            QemuArch::Aarch64 => Some((
                "/usr/share/AAVMF/AAVMF_CODE.fd",
                "/usr/share/AAVMF/AAVMF_VARS.fd",
            )),
            _ => None,
        }
    }

    /// UEFI Secure Boot firmware path (secboot variant).
    /// Returns the Secure Boot firmware code path + NVRAM template.
    /// For aarch64, there is no separate secboot variant — returns same as regular.
    pub fn uefi_secboot_firmware_path(&self) -> Option<(&str, &str)> {
        match self {
            QemuArch::X86_64 | QemuArch::I386 => Some((
                "/usr/share/OVMF/OVMF_CODE_4M.secboot.fd",
                "/usr/share/OVMF/OVMF_VARS_4M.fd",
            )),
            QemuArch::Aarch64 => Some((
                "/usr/share/AAVMF/AAVMF_CODE.fd",
                "/usr/share/AAVMF/AAVMF_VARS.fd",
            )),
            _ => None,
        }
    }

    /// Recommended defaults for a VM on this architecture.
    pub fn recommended_defaults(&self) -> ArchDefaults {
        match self {
            QemuArch::X86_64 => ArchDefaults {
                cpus: 2,
                memory_mib: 4096,
                disk_gib: 25,
                uefi: true,
            },
            QemuArch::I386 => ArchDefaults {
                cpus: 1,
                memory_mib: 2048,
                disk_gib: 10,
                uefi: false,
            },
            QemuArch::Aarch64 => ArchDefaults {
                cpus: 2,
                memory_mib: 2048,
                disk_gib: 20,
                uefi: true,
            },
            QemuArch::Arm => ArchDefaults {
                cpus: 1,
                memory_mib: 1024,
                disk_gib: 10,
                uefi: false,
            },
            QemuArch::Riscv64 => ArchDefaults {
                cpus: 2,
                memory_mib: 2048,
                disk_gib: 20,
                uefi: false,
            },
            QemuArch::Riscv32 => ArchDefaults {
                cpus: 1,
                memory_mib: 512,
                disk_gib: 8,
                uefi: false,
            },
            QemuArch::Ppc64 => ArchDefaults {
                cpus: 2,
                memory_mib: 4096,
                disk_gib: 20,
                uefi: false,
            },
            QemuArch::Ppc => ArchDefaults {
                cpus: 1,
                memory_mib: 512,
                disk_gib: 8,
                uefi: false,
            },
            QemuArch::S390x => ArchDefaults {
                cpus: 2,
                memory_mib: 2048,
                disk_gib: 20,
                uefi: false,
            },
            QemuArch::Mips | QemuArch::Mipsel => ArchDefaults {
                cpus: 1,
                memory_mib: 256,
                disk_gib: 4,
                uefi: false,
            },
            QemuArch::Mips64 | QemuArch::Mips64el => ArchDefaults {
                cpus: 1,
                memory_mib: 1024,
                disk_gib: 10,
                uefi: false,
            },
            QemuArch::Sparc => ArchDefaults {
                cpus: 1,
                memory_mib: 256,
                disk_gib: 4,
                uefi: false,
            },
            QemuArch::Sparc64 => ArchDefaults {
                cpus: 1,
                memory_mib: 1024,
                disk_gib: 10,
                uefi: false,
            },
            QemuArch::M68k => ArchDefaults {
                cpus: 1,
                memory_mib: 128,
                disk_gib: 2,
                uefi: false,
            },
            _ => ArchDefaults {
                cpus: 1,
                memory_mib: 256,
                disk_gib: 4,
                uefi: false,
            },
        }
    }

    /// Maximum supported CPUs for this architecture.
    pub fn max_cpus(&self) -> u32 {
        match self {
            QemuArch::X86_64 | QemuArch::I386 => 255,
            QemuArch::Aarch64 => 512,
            QemuArch::Arm => 4,
            QemuArch::Riscv64 => 32,
            QemuArch::Riscv32 => 8,
            QemuArch::Ppc64 => 1024,
            QemuArch::Ppc => 4,
            QemuArch::S390x => 248,
            QemuArch::Mips | QemuArch::Mipsel | QemuArch::Mips64 | QemuArch::Mips64el => 16,
            QemuArch::Sparc => 4,
            QemuArch::Sparc64 => 128,
            _ => 1,
        }
    }

    /// Available CPU feature flags for this architecture.
    /// These are the most commonly used QEMU CPU features that users
    /// might want to toggle for compatibility or testing.
    pub fn cpu_features(&self) -> Vec<CpuFeature> {
        match self {
            QemuArch::X86_64 | QemuArch::I386 => vec![
                // SIMD extensions
                CpuFeature::new("sse4.1", "SSE 4.1 — streaming SIMD", "SIMD", true),
                CpuFeature::new("sse4.2", "SSE 4.2 — string/text processing", "SIMD", true),
                CpuFeature::new("avx", "AVX — 256-bit vector ops", "SIMD", false),
                CpuFeature::new("avx2", "AVX2 — integer 256-bit vector ops", "SIMD", false),
                CpuFeature::new(
                    "avx512f",
                    "AVX-512 Foundation — 512-bit vectors",
                    "SIMD",
                    false,
                ),
                CpuFeature::new("fma", "FMA — fused multiply-add", "SIMD", false),
                CpuFeature::new(
                    "f16c",
                    "F16C — half-precision float conversion",
                    "SIMD",
                    false,
                ),
                // Crypto
                CpuFeature::new("aes", "AES-NI — hardware AES acceleration", "Crypto", false),
                CpuFeature::new(
                    "pclmulqdq",
                    "CLMUL — carry-less multiply (GCM)",
                    "Crypto",
                    false,
                ),
                CpuFeature::new("sha-ni", "SHA-NI — hardware SHA-1/SHA-256", "Crypto", false),
                CpuFeature::new(
                    "rdrand",
                    "RDRAND — hardware random number generator",
                    "Crypto",
                    false,
                ),
                CpuFeature::new("rdseed", "RDSEED — hardware random seeder", "Crypto", false),
                // Bit manipulation
                CpuFeature::new(
                    "popcnt",
                    "POPCNT — population count",
                    "Bit Manipulation",
                    true,
                ),
                CpuFeature::new(
                    "bmi1",
                    "BMI1 — bit manipulation instructions",
                    "Bit Manipulation",
                    false,
                ),
                CpuFeature::new(
                    "bmi2",
                    "BMI2 — enhanced bit manipulation",
                    "Bit Manipulation",
                    false,
                ),
                CpuFeature::new(
                    "adx",
                    "ADX — multi-precision add-carry",
                    "Bit Manipulation",
                    false,
                ),
                CpuFeature::new(
                    "lzcnt",
                    "LZCNT — leading zero count",
                    "Bit Manipulation",
                    false,
                ),
                // Virtualization
                CpuFeature::new(
                    "vmx",
                    "VMX — Intel VT-x nested virtualization",
                    "Virtualization",
                    false,
                ),
                CpuFeature::new(
                    "svm",
                    "SVM — AMD-V nested virtualization",
                    "Virtualization",
                    false,
                ),
                // Memory/Performance
                CpuFeature::new(
                    "xsave",
                    "XSAVE — extended state save/restore",
                    "Performance",
                    true,
                ),
                CpuFeature::new(
                    "xsaveopt",
                    "XSAVEOPT — optimized state save",
                    "Performance",
                    false,
                ),
                CpuFeature::new("movbe", "MOVBE — move big-endian", "Performance", false),
                CpuFeature::new(
                    "invpcid",
                    "INVPCID — invalidate TLB by PCID",
                    "Performance",
                    false,
                ),
            ],
            QemuArch::Aarch64 => vec![
                // SIMD / Vector
                CpuFeature::new("sve", "SVE — scalable vector extension", "SIMD", false),
                CpuFeature::new("sve128", "SVE 128-bit vector length", "SIMD", false),
                CpuFeature::new("sve256", "SVE 256-bit vector length", "SIMD", false),
                CpuFeature::new("sve512", "SVE 512-bit vector length", "SIMD", false),
                // Crypto
                CpuFeature::new("aes", "AES — hardware AES acceleration", "Crypto", true),
                CpuFeature::new("sha1", "SHA-1 — hardware SHA-1", "Crypto", true),
                CpuFeature::new("sha256", "SHA-256 — hardware SHA-256", "Crypto", true),
                CpuFeature::new("sha512", "SHA-512 — hardware SHA-512", "Crypto", false),
                CpuFeature::new("sha3", "SHA-3 — hardware SHA-3", "Crypto", false),
                CpuFeature::new("sm3", "SM3 — Chinese hash algorithm", "Crypto", false),
                CpuFeature::new("sm4", "SM4 — Chinese block cipher", "Crypto", false),
                // Atomics / Ordering
                CpuFeature::new(
                    "atomics",
                    "LSE Atomics — large system extensions",
                    "Atomics",
                    true,
                ),
                CpuFeature::new("rdm", "RDM — rounding double multiply", "Atomics", false),
                // Security
                CpuFeature::new("pauth", "PAuth — pointer authentication", "Security", false),
                CpuFeature::new("mte", "MTE — memory tagging extension", "Security", false),
                CpuFeature::new(
                    "bti",
                    "BTI — branch target identification",
                    "Security",
                    false,
                ),
                // Performance
                CpuFeature::new(
                    "flagm",
                    "FLAGM — flag manipulation instructions",
                    "Performance",
                    false,
                ),
                CpuFeature::new("dit", "DIT — data-independent timing", "Performance", false),
                CpuFeature::new(
                    "frint",
                    "FRINT — floating-point round to integer",
                    "Performance",
                    false,
                ),
            ],
            QemuArch::Arm => vec![
                CpuFeature::new("neon", "NEON — SIMD for ARM32", "SIMD", true),
                CpuFeature::new("vfpv3", "VFPv3 — vector floating point v3", "FPU", true),
                CpuFeature::new("vfpv4", "VFPv4 — vector floating point v4", "FPU", false),
                CpuFeature::new(
                    "thumb",
                    "Thumb — compact 16-bit instruction set",
                    "ISA",
                    true,
                ),
                CpuFeature::new(
                    "trustzone",
                    "TrustZone — ARM security extension",
                    "Security",
                    false,
                ),
            ],
            QemuArch::Riscv64 | QemuArch::Riscv32 => vec![
                CpuFeature::new("m", "M — integer multiply/divide", "ISA", true),
                CpuFeature::new("a", "A — atomic instructions", "ISA", true),
                CpuFeature::new("f", "F — single-precision float", "ISA", true),
                CpuFeature::new("d", "D — double-precision float", "ISA", true),
                CpuFeature::new("c", "C — compressed instructions (16-bit)", "ISA", true),
                CpuFeature::new("v", "V — vector extension", "SIMD", false),
                CpuFeature::new("h", "H — hypervisor extension", "Virtualization", false),
                CpuFeature::new(
                    "zba",
                    "Zba — address generation bit-manip",
                    "Bit Manipulation",
                    false,
                ),
                CpuFeature::new(
                    "zbb",
                    "Zbb — basic bit manipulation",
                    "Bit Manipulation",
                    false,
                ),
                CpuFeature::new("zbc", "Zbc — carry-less multiplication", "Crypto", false),
                CpuFeature::new(
                    "zbs",
                    "Zbs — single-bit operations",
                    "Bit Manipulation",
                    false,
                ),
                CpuFeature::new("zicsr", "Zicsr — CSR instructions", "ISA", true),
                CpuFeature::new("zifencei", "Zifencei — instruction fence", "ISA", true),
            ],
            QemuArch::Ppc64 => vec![
                CpuFeature::new("vsx", "VSX — vector-scalar extension", "SIMD", true),
                CpuFeature::new("altivec", "AltiVec — SIMD for PowerPC", "SIMD", true),
                CpuFeature::new("dfp", "DFP — decimal floating point", "FPU", false),
                CpuFeature::new(
                    "htm",
                    "HTM — hardware transactional memory",
                    "Performance",
                    false,
                ),
            ],
            QemuArch::S390x => vec![
                CpuFeature::new("vx", "VX — vector extension", "SIMD", true),
                CpuFeature::new("vxe", "VXE — vector enhancements", "SIMD", false),
                CpuFeature::new("msa", "MSA — message-security assist", "Crypto", true),
                CpuFeature::new("msa5", "MSA-5 — advanced message security", "Crypto", false),
                CpuFeature::new("te", "TE — transactional execution", "Performance", false),
            ],
            _ => Vec::new(), // No configurable features for exotic architectures
        }
    }

    /// Whether this architecture has configurable CPU features.
    pub fn has_cpu_features(&self) -> bool {
        !self.cpu_features().is_empty()
    }

    /// Whether this architecture supports CPU topology (SMP).
    /// Most modern architectures do; microcontrollers don't.
    pub fn supports_smp_topology(&self) -> bool {
        match self {
            QemuArch::Avr | QemuArch::Or1k | QemuArch::Microblaze => false,
            _ => self.max_cpus() > 1,
        }
    }
}

impl std::fmt::Display for QemuArch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// CPU feature flags
// ─────────────────────────────────────────────────────────────────────────────

/// A CPU feature flag that can be enabled/disabled in QEMU/libvirt.
/// Maps to `<feature policy='require' name='...'/>` in libvirt XML, or
/// `-cpu model,+feature` on the QEMU command line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuFeature {
    /// QEMU feature name (e.g., "sse4.1", "avx2", "sve").
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Category/group (e.g., "SIMD", "Crypto", "Virtualization").
    pub group: String,
    /// Whether this is commonly enabled by default in the selected CPU model.
    pub default_on: bool,
}

impl CpuFeature {
    pub fn new(name: &str, description: &str, group: &str, default_on: bool) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            group: group.to_string(),
            default_on,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Machine type
// ─────────────────────────────────────────────────────────────────────────────

/// A QEMU machine type for a specific architecture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineType {
    /// QEMU machine ID (passed as -machine <id>).
    pub id: String,
    /// Human-readable description.
    pub description: String,
    /// Whether this is the recommended default.
    pub is_default: bool,
}

impl MachineType {
    pub fn new(id: &str, description: &str, is_default: bool) -> Self {
        Self {
            id: id.to_string(),
            description: description.to_string(),
            is_default,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// CPU model
// ─────────────────────────────────────────────────────────────────────────────

/// A QEMU CPU model for a specific architecture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuModel {
    /// QEMU CPU name (passed as -cpu <name>).
    pub id: String,
    /// Human-readable description.
    pub description: String,
    /// Whether this is the recommended default.
    pub is_default: bool,
}

impl CpuModel {
    pub fn new(id: &str, description: &str, is_default: bool) -> Self {
        Self {
            id: id.to_string(),
            description: description.to_string(),
            is_default,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Architecture defaults
// ─────────────────────────────────────────────────────────────────────────────

/// Recommended defaults for a new VM on a given architecture.
pub struct ArchDefaults {
    pub cpus: u32,
    pub memory_mib: u64,
    pub disk_gib: u64,
    pub uefi: bool,
}

// ─────────────────────────────────────────────────────────────────────────────
// Box types — the three "modes" of Libre VMM
// ─────────────────────────────────────────────────────────────────────────────

/// The three "Box" types that define different VM creation/management modes.
/// Each box tailors the UI, exposed options, and default configurations.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BoxType {
    /// Box 1: Professional Desktop VM Manager
    /// Clean, polished experience for everyday VM use.
    /// x86_64 focused, templates, one-click setup.
    /// Color: Blue (professional)
    Standard,

    /// Box 2: Universal Hardware Lab
    /// Cross-architecture emulation, board-level config, exotic devices.
    /// All QEMU architectures exposed, manual machine/CPU selection.
    /// Color: Green/Teal (engineering/lab)
    HardwareLab,

    /// Box 3: Power User Hypervisor
    /// CPU pinning, NUMA, VFIO, raw QEMU args, resource QoS.
    /// For sysadmins and performance engineers.
    /// Color: Orange/Amber (power/warning)
    PowerUser,
}

impl Default for BoxType {
    fn default() -> Self {
        BoxType::Standard
    }
}

impl BoxType {
    /// All box types.
    pub fn all() -> Vec<BoxType> {
        vec![BoxType::Standard, BoxType::HardwareLab, BoxType::PowerUser]
    }

    /// Display name.
    pub fn display_name(&self) -> &str {
        match self {
            BoxType::Standard => "Standard",
            BoxType::HardwareLab => "Hardware Lab",
            BoxType::PowerUser => "Power User",
        }
    }

    /// Subtitle / description.
    pub fn description(&self) -> &str {
        match self {
            BoxType::Standard => "Polished desktop VM manager for everyday use",
            BoxType::HardwareLab => "Cross-architecture emulation and board-level hardware lab",
            BoxType::PowerUser => {
                "Advanced hypervisor with CPU pinning, VFIO, and performance tuning"
            },
        }
    }

    /// Icon character for the box type.
    pub fn icon(&self) -> &str {
        match self {
            BoxType::Standard => "PC",
            BoxType::HardwareLab => "HW",
            BoxType::PowerUser => "SU",
        }
    }

    /// Which architectures are visible in this box mode.
    pub fn visible_architectures(&self) -> Vec<QemuArch> {
        match self {
            BoxType::Standard => vec![QemuArch::X86_64, QemuArch::I386],
            BoxType::HardwareLab => QemuArch::all(),
            BoxType::PowerUser => vec![QemuArch::X86_64, QemuArch::Aarch64, QemuArch::I386],
        }
    }
}

impl std::fmt::Display for BoxType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arch_defaults() {
        let arch = QemuArch::X86_64;
        assert_eq!(arch.qemu_suffix(), "x86_64");
        assert_eq!(arch.default_machine(), "q35");
        assert!(arch.has_uefi_support());
        assert!(arch.has_virtio_support());
        assert!(arch.can_use_kvm_on_x86());
    }

    #[test]
    fn test_arch_display() {
        assert_eq!(QemuArch::Aarch64.display_name(), "ARM64 (aarch64)");
        assert_eq!(QemuArch::Riscv64.display_name(), "RISC-V 64-bit");
    }

    #[test]
    fn test_box_types() {
        assert_eq!(BoxType::all().len(), 3);
        assert!(BoxType::Standard.visible_architectures().len() <= 3);
        assert!(BoxType::HardwareLab.visible_architectures().len() > 10);
    }

    #[test]
    fn test_machine_types() {
        let machines = QemuArch::X86_64.machine_types();
        assert!(machines.iter().any(|m| m.id == "q35"));
        assert!(machines.iter().any(|m| m.is_default));
    }
}
