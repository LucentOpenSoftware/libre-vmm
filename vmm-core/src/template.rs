use serde::{Deserialize, Serialize};

use crate::config::OsType;

/// Category for grouping OS templates in the wizard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OsCategory {
    LinuxDesktop,
    LinuxServer,
    Windows,
    MacOS,
    BsdOther,
}

impl OsCategory {
    /// User-facing label key (for i18n lookup).
    pub fn label_key(&self) -> &'static str {
        match self {
            OsCategory::LinuxDesktop => "template.cat-linux-desktop",
            OsCategory::LinuxServer => "template.cat-linux-server",
            OsCategory::Windows => "template.cat-windows",
            OsCategory::MacOS => "template.cat-macos",
            OsCategory::BsdOther => "template.cat-bsd-other",
        }
    }

    /// Icon/emoji for category header.
    pub fn icon(&self) -> &'static str {
        match self {
            OsCategory::LinuxDesktop => "\u{1F427}", // penguin
            OsCategory::LinuxServer => "\u{1F5A5}",  // desktop computer (server)
            OsCategory::Windows => "\u{1FA9F}",      // window
            OsCategory::MacOS => "\u{1F34E}",        // red apple
            OsCategory::BsdOther => "\u{2699}",      // gear
        }
    }

    /// Display order.
    pub fn order(&self) -> u8 {
        match self {
            OsCategory::LinuxDesktop => 0,
            OsCategory::LinuxServer => 1,
            OsCategory::Windows => 2,
            OsCategory::MacOS => 3,
            OsCategory::BsdOther => 4,
        }
    }

    /// All categories in display order.
    pub const ALL: &'static [OsCategory] = &[
        OsCategory::LinuxDesktop,
        OsCategory::LinuxServer,
        OsCategory::Windows,
        OsCategory::MacOS,
        OsCategory::BsdOther,
    ];
}

/// Pre-configured template for common operating systems.
/// Gives non-technical users sensible defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsTemplate {
    pub id: &'static str,
    pub label: &'static str,
    pub os_type: OsType,
    pub category: OsCategory,
    pub recommended_cpus: u32,
    pub recommended_memory_mib: u64,
    pub recommended_disk_gib: u64,
    pub uefi: bool,
    pub description: &'static str,
}

/// Static table of built-in OS templates (zero allocation on access).
static BUILTIN_TEMPLATES: &[OsTemplate] = &[
    // ── Linux Desktop ──────────────────────────────────────────────
    OsTemplate {
        id: "ubuntu-desktop",
        label: "Ubuntu Desktop (24.04+)",
        os_type: OsType::Linux,
        category: OsCategory::LinuxDesktop,
        recommended_cpus: 2,
        recommended_memory_mib: 4096,
        recommended_disk_gib: 25,
        uefi: true,
        description: "Popular, user-friendly Linux desktop",
    },
    OsTemplate {
        id: "fedora-workstation",
        label: "Fedora Workstation",
        os_type: OsType::Linux,
        category: OsCategory::LinuxDesktop,
        recommended_cpus: 2,
        recommended_memory_mib: 4096,
        recommended_disk_gib: 25,
        uefi: true,
        description: "Cutting-edge Linux with GNOME desktop",
    },
    OsTemplate {
        id: "linux-mint",
        label: "Linux Mint",
        os_type: OsType::Linux,
        category: OsCategory::LinuxDesktop,
        recommended_cpus: 2,
        recommended_memory_mib: 2048,
        recommended_disk_gib: 20,
        uefi: true,
        description: "Elegant, easy-to-use Linux desktop",
    },
    OsTemplate {
        id: "debian-desktop",
        label: "Debian (Desktop)",
        os_type: OsType::Linux,
        category: OsCategory::LinuxDesktop,
        recommended_cpus: 2,
        recommended_memory_mib: 2048,
        recommended_disk_gib: 20,
        uefi: true,
        description: "Rock-solid stable Linux with desktop",
    },
    OsTemplate {
        id: "arch-linux",
        label: "Arch Linux",
        os_type: OsType::Linux,
        category: OsCategory::LinuxDesktop,
        recommended_cpus: 2,
        recommended_memory_mib: 2048,
        recommended_disk_gib: 20,
        uefi: true,
        description: "Lightweight, rolling-release Linux",
    },
    OsTemplate {
        id: "opensuse-tumbleweed",
        label: "openSUSE Tumbleweed",
        os_type: OsType::Linux,
        category: OsCategory::LinuxDesktop,
        recommended_cpus: 2,
        recommended_memory_mib: 4096,
        recommended_disk_gib: 25,
        uefi: true,
        description: "Rolling-release with YaST management",
    },
    OsTemplate {
        id: "pop-os",
        label: "Pop!_OS",
        os_type: OsType::Linux,
        category: OsCategory::LinuxDesktop,
        recommended_cpus: 2,
        recommended_memory_mib: 4096,
        recommended_disk_gib: 25,
        uefi: true,
        description: "Developer-friendly Ubuntu derivative",
    },
    OsTemplate {
        id: "elementary-os",
        label: "elementary OS",
        os_type: OsType::Linux,
        category: OsCategory::LinuxDesktop,
        recommended_cpus: 2,
        recommended_memory_mib: 4096,
        recommended_disk_gib: 25,
        uefi: true,
        description: "Beautiful, privacy-respecting desktop",
    },
    OsTemplate {
        id: "zorin-os",
        label: "Zorin OS",
        os_type: OsType::Linux,
        category: OsCategory::LinuxDesktop,
        recommended_cpus: 2,
        recommended_memory_mib: 4096,
        recommended_disk_gib: 25,
        uefi: true,
        description: "Familiar desktop for Windows switchers",
    },
    OsTemplate {
        id: "manjaro",
        label: "Manjaro Linux",
        os_type: OsType::Linux,
        category: OsCategory::LinuxDesktop,
        recommended_cpus: 2,
        recommended_memory_mib: 4096,
        recommended_disk_gib: 25,
        uefi: true,
        description: "User-friendly Arch-based rolling-release",
    },
    OsTemplate {
        id: "kde-neon",
        label: "KDE neon",
        os_type: OsType::Linux,
        category: OsCategory::LinuxDesktop,
        recommended_cpus: 2,
        recommended_memory_mib: 4096,
        recommended_disk_gib: 25,
        uefi: true,
        description: "Latest KDE Plasma on Ubuntu LTS base",
    },
    OsTemplate {
        id: "nobara",
        label: "Nobara Linux",
        os_type: OsType::Linux,
        category: OsCategory::LinuxDesktop,
        recommended_cpus: 4,
        recommended_memory_mib: 4096,
        recommended_disk_gib: 40,
        uefi: true,
        description: "Gaming-optimized Fedora variant",
    },
    // ── Linux Server ───────────────────────────────────────────────
    OsTemplate {
        id: "ubuntu-server",
        label: "Ubuntu Server",
        os_type: OsType::Linux,
        category: OsCategory::LinuxServer,
        recommended_cpus: 1,
        recommended_memory_mib: 1024,
        recommended_disk_gib: 10,
        uefi: true,
        description: "Ubuntu LTS for headless workloads",
    },
    OsTemplate {
        id: "debian-server",
        label: "Debian (Server)",
        os_type: OsType::Linux,
        category: OsCategory::LinuxServer,
        recommended_cpus: 1,
        recommended_memory_mib: 1024,
        recommended_disk_gib: 10,
        uefi: true,
        description: "Rock-solid stable server distribution",
    },
    OsTemplate {
        id: "rocky-linux",
        label: "Rocky Linux",
        os_type: OsType::Linux,
        category: OsCategory::LinuxServer,
        recommended_cpus: 2,
        recommended_memory_mib: 2048,
        recommended_disk_gib: 20,
        uefi: true,
        description: "RHEL-compatible enterprise Linux",
    },
    OsTemplate {
        id: "almalinux",
        label: "AlmaLinux",
        os_type: OsType::Linux,
        category: OsCategory::LinuxServer,
        recommended_cpus: 2,
        recommended_memory_mib: 2048,
        recommended_disk_gib: 20,
        uefi: true,
        description: "Community RHEL-compatible distribution",
    },
    OsTemplate {
        id: "opensuse-leap",
        label: "openSUSE Leap",
        os_type: OsType::Linux,
        category: OsCategory::LinuxServer,
        recommended_cpus: 1,
        recommended_memory_mib: 2048,
        recommended_disk_gib: 20,
        uefi: true,
        description: "Stable release with enterprise support",
    },
    OsTemplate {
        id: "alpine-linux",
        label: "Alpine Linux",
        os_type: OsType::Linux,
        category: OsCategory::LinuxServer,
        recommended_cpus: 1,
        recommended_memory_mib: 512,
        recommended_disk_gib: 5,
        uefi: true,
        description: "Ultra-lightweight musl-based Linux",
    },
    OsTemplate {
        id: "nixos",
        label: "NixOS",
        os_type: OsType::Linux,
        category: OsCategory::LinuxServer,
        recommended_cpus: 2,
        recommended_memory_mib: 2048,
        recommended_disk_gib: 20,
        uefi: true,
        description: "Declarative, reproducible Linux system",
    },
    OsTemplate {
        id: "linux-server",
        label: "Linux Server (minimal)",
        os_type: OsType::Linux,
        category: OsCategory::LinuxServer,
        recommended_cpus: 1,
        recommended_memory_mib: 1024,
        recommended_disk_gib: 10,
        uefi: true,
        description: "Generic minimal server without desktop",
    },
    // ── Windows ────────────────────────────────────────────────────
    OsTemplate {
        id: "windows-11",
        label: "Windows 11",
        os_type: OsType::Windows,
        category: OsCategory::Windows,
        recommended_cpus: 4,
        recommended_memory_mib: 8192,
        recommended_disk_gib: 64,
        uefi: true,
        description: "Microsoft Windows 11 (requires license)",
    },
    OsTemplate {
        id: "windows-10",
        label: "Windows 10",
        os_type: OsType::Windows,
        category: OsCategory::Windows,
        recommended_cpus: 2,
        recommended_memory_mib: 4096,
        recommended_disk_gib: 40,
        uefi: true,
        description: "Microsoft Windows 10 (requires license)",
    },
    OsTemplate {
        id: "windows-server-2022",
        label: "Windows Server 2022",
        os_type: OsType::Windows,
        category: OsCategory::Windows,
        recommended_cpus: 4,
        recommended_memory_mib: 8192,
        recommended_disk_gib: 64,
        uefi: true,
        description: "Windows Server for enterprise workloads",
    },
    OsTemplate {
        id: "windows-server-2019",
        label: "Windows Server 2019",
        os_type: OsType::Windows,
        category: OsCategory::Windows,
        recommended_cpus: 2,
        recommended_memory_mib: 4096,
        recommended_disk_gib: 40,
        uefi: true,
        description: "Proven Windows Server LTS release",
    },
    // ── macOS ──────────────────────────────────────────────────────
    OsTemplate {
        id: "macos-sequoia",
        label: "macOS Sequoia (15)",
        os_type: OsType::MacOS,
        category: OsCategory::MacOS,
        recommended_cpus: 4,
        recommended_memory_mib: 8192,
        recommended_disk_gib: 80,
        uefi: true,
        description: "Apple macOS 15 (OpenCore + Intel CPU)",
    },
    OsTemplate {
        id: "macos-sonoma",
        label: "macOS Sonoma (14)",
        os_type: OsType::MacOS,
        category: OsCategory::MacOS,
        recommended_cpus: 4,
        recommended_memory_mib: 8192,
        recommended_disk_gib: 80,
        uefi: true,
        description: "Apple macOS 14 (OpenCore + Intel CPU)",
    },
    OsTemplate {
        id: "macos-ventura",
        label: "macOS Ventura (13)",
        os_type: OsType::MacOS,
        category: OsCategory::MacOS,
        recommended_cpus: 4,
        recommended_memory_mib: 8192,
        recommended_disk_gib: 64,
        uefi: true,
        description: "Apple macOS 13 (OpenCore + Intel CPU)",
    },
    OsTemplate {
        id: "macos-monterey",
        label: "macOS Monterey (12)",
        os_type: OsType::MacOS,
        category: OsCategory::MacOS,
        recommended_cpus: 4,
        recommended_memory_mib: 4096,
        recommended_disk_gib: 64,
        uefi: true,
        description: "Apple macOS 12 (OpenCore + Intel CPU)",
    },
    OsTemplate {
        id: "macos-big-sur",
        label: "macOS Big Sur (11)",
        os_type: OsType::MacOS,
        category: OsCategory::MacOS,
        recommended_cpus: 2,
        recommended_memory_mib: 4096,
        recommended_disk_gib: 64,
        uefi: true,
        description: "Apple macOS 11 (OpenCore + Intel CPU)",
    },
    OsTemplate {
        id: "macos-catalina",
        label: "macOS Catalina (10.15)",
        os_type: OsType::MacOS,
        category: OsCategory::MacOS,
        recommended_cpus: 2,
        recommended_memory_mib: 4096,
        recommended_disk_gib: 64,
        uefi: false,
        description: "Apple macOS 10.15 (OpenCore + Intel CPU)",
    },
    // ── BSD & Other ────────────────────────────────────────────────
    OsTemplate {
        id: "freebsd",
        label: "FreeBSD",
        os_type: OsType::FreeBSD,
        category: OsCategory::BsdOther,
        recommended_cpus: 2,
        recommended_memory_mib: 2048,
        recommended_disk_gib: 20,
        uefi: true,
        description: "FreeBSD operating system",
    },
    OsTemplate {
        id: "openbsd",
        label: "OpenBSD",
        os_type: OsType::FreeBSD,
        category: OsCategory::BsdOther,
        recommended_cpus: 1,
        recommended_memory_mib: 1024,
        recommended_disk_gib: 10,
        uefi: true,
        description: "Security-focused BSD operating system",
    },
    OsTemplate {
        id: "netbsd",
        label: "NetBSD",
        os_type: OsType::FreeBSD,
        category: OsCategory::BsdOther,
        recommended_cpus: 1,
        recommended_memory_mib: 1024,
        recommended_disk_gib: 10,
        uefi: true,
        description: "Portable BSD for any platform",
    },
    OsTemplate {
        id: "haiku",
        label: "Haiku",
        os_type: OsType::Other,
        category: OsCategory::BsdOther,
        recommended_cpus: 2,
        recommended_memory_mib: 2048,
        recommended_disk_gib: 16,
        uefi: false,
        description: "BeOS-inspired open-source OS",
    },
    OsTemplate {
        id: "reactos",
        label: "ReactOS",
        os_type: OsType::Other,
        category: OsCategory::BsdOther,
        recommended_cpus: 1,
        recommended_memory_mib: 1024,
        recommended_disk_gib: 10,
        uefi: false,
        description: "Open-source Windows-compatible OS",
    },
    OsTemplate {
        id: "custom",
        label: "Custom / Other",
        os_type: OsType::Other,
        category: OsCategory::BsdOther,
        recommended_cpus: 2,
        recommended_memory_mib: 2048,
        recommended_disk_gib: 20,
        uefi: false,
        description: "Manual configuration for any OS",
    },
];

/// All built-in OS templates. Returns a static slice (zero allocation).
pub fn builtin_templates() -> &'static [OsTemplate] {
    BUILTIN_TEMPLATES
}

/// Return templates for a given category.
pub fn templates_by_category(cat: OsCategory) -> Vec<(usize, &'static OsTemplate)> {
    BUILTIN_TEMPLATES
        .iter()
        .enumerate()
        .filter(|(_, t)| t.category == cat)
        .collect()
}
