//! Host system capability checks for the first-run wizard.
//!
//! Pure, read-only probes that detect whether the host is ready to run libre-vmm:
//! - KVM kernel module loaded
//! - /dev/kvm device node present
//! - libvirtd service running
//! - User in libvirt/kvm groups
//! - qemu-system-x86_64 / qemu-kvm binary available
//! - OVMF UEFI firmware installed
//! - swtpm binary for TPM emulation
//!
//! These checks NEVER modify state. They are used by the first-run wizard to
//! decide what install hints to show the user, and by diagnostic dumps.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Snapshot of host capability detection results.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SystemCheck {
    pub kvm_module_loaded: bool,
    pub kvm_dev_present: bool,
    pub libvirtd_running: bool,
    pub user_in_libvirt_group: bool,
    pub user_in_kvm_group: bool,
    /// Path to qemu-system-x86_64 if found.
    pub qemu_binary_found: Option<String>,
    /// Path to OVMF_CODE.fd if found.
    pub ovmf_present: Option<String>,
    /// Path to swtpm binary if found.
    pub swtpm_binary_found: Option<String>,
    /// Detected linux distro id from /etc/os-release (e.g. "ubuntu", "fedora", "arch").
    pub distro_id: Option<String>,
}

impl SystemCheck {
    /// Returns true when every essential capability is available.
    pub fn all_essentials_ok(&self) -> bool {
        self.kvm_module_loaded
            && self.kvm_dev_present
            && self.libvirtd_running
            && self.user_in_libvirt_group
            && self.qemu_binary_found.is_some()
    }

    /// Returns a list of short reasons describing each missing piece (empty when OK).
    pub fn missing_pieces(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if !self.kvm_module_loaded {
            out.push("kvm-module");
        }
        if !self.kvm_dev_present {
            out.push("kvm-dev");
        }
        if !self.libvirtd_running {
            out.push("libvirtd");
        }
        if !self.user_in_libvirt_group {
            out.push("libvirt-group");
        }
        if !self.user_in_kvm_group {
            out.push("kvm-group");
        }
        if self.qemu_binary_found.is_none() {
            out.push("qemu");
        }
        if self.ovmf_present.is_none() {
            out.push("ovmf");
        }
        if self.swtpm_binary_found.is_none() {
            out.push("swtpm");
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Individual probes
// ---------------------------------------------------------------------------

/// True if `/proc/modules` lists a `kvm` module.
pub fn check_kvm_module() -> bool {
    match std::fs::read_to_string("/proc/modules") {
        Ok(contents) => parse_kvm_module_present(&contents),
        Err(_) => false,
    }
}

/// Parser for `/proc/modules` lines. Exposed for testing.
pub fn parse_kvm_module_present(proc_modules: &str) -> bool {
    for line in proc_modules.lines() {
        // Each line: `<name> <size> <usecount> ...`
        let name = line.split_whitespace().next().unwrap_or("");
        if name == "kvm" || name == "kvm_intel" || name == "kvm_amd" {
            return true;
        }
    }
    false
}

/// True if `/dev/kvm` exists.
pub fn check_kvm_dev() -> bool {
    Path::new("/dev/kvm").exists()
}

/// True if libvirtd appears to be running.
///
/// Checks for the well-known socket first (works without sudo). Falls back to
/// `systemctl is-active libvirtd`.
pub fn check_libvirtd_running() -> bool {
    let sockets = ["/var/run/libvirt/libvirt-sock", "/run/libvirt/libvirt-sock"];
    for s in &sockets {
        if Path::new(s).exists() {
            return true;
        }
    }
    // Fallback — try systemctl. Don't error if missing.
    match std::process::Command::new("systemctl")
        .args(["is-active", "libvirtd"])
        .output()
    {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            stdout.trim() == "active"
        },
        Err(_) => false,
    }
}

/// True if the current user belongs to `group`.
pub fn user_in_group(group: &str) -> bool {
    // First try `id -nG` which lists current process supplementary groups.
    if let Ok(out) = std::process::Command::new("id").arg("-nG").output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            if parse_id_output(&s, group) {
                return true;
            }
        }
    }
    // Fallback: read /etc/group and check membership directly.
    if let Ok(contents) = std::fs::read_to_string("/etc/group") {
        if let Some(user) = current_username() {
            return parse_etc_group_membership(&contents, group, &user);
        }
    }
    false
}

/// Determine username via `$USER` or `$LOGNAME` env vars.
fn current_username() -> Option<String> {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .ok()
        .filter(|s| !s.is_empty())
}

/// Parse `id -nG` output (whitespace-separated group names). Exposed for testing.
pub fn parse_id_output(id_output: &str, target: &str) -> bool {
    id_output
        .split_whitespace()
        .any(|g| g.trim_end_matches('\n') == target)
}

/// Parse `/etc/group` and check membership. Format: `name:x:gid:user1,user2,...`.
pub fn parse_etc_group_membership(etc_group: &str, target_group: &str, user: &str) -> bool {
    for line in etc_group.lines() {
        let parts: Vec<&str> = line.splitn(4, ':').collect();
        if parts.len() < 4 {
            continue;
        }
        if parts[0] != target_group {
            continue;
        }
        let members = parts[3];
        for m in members.split(',') {
            if m.trim() == user {
                return true;
            }
        }
    }
    false
}

/// Find a QEMU binary on PATH. Prefers `qemu-system-x86_64`, falls back to `qemu-kvm`.
pub fn find_qemu_binary() -> Option<String> {
    for candidate in ["qemu-system-x86_64", "qemu-kvm"] {
        if let Some(p) = which(candidate) {
            return Some(p);
        }
    }
    None
}

/// Find an OVMF firmware image. Returns the path to `OVMF_CODE.fd` or equivalent.
pub fn find_ovmf() -> Option<String> {
    let candidates = [
        "/usr/share/OVMF/OVMF_CODE.fd",
        "/usr/share/OVMF/OVMF_CODE_4M.fd",
        "/usr/share/edk2/x64/OVMF_CODE.fd",
        "/usr/share/edk2-ovmf/x64/OVMF_CODE.fd",
        "/usr/share/edk2/ovmf/OVMF_CODE.fd",
        "/usr/share/qemu/OVMF.fd",
        "/usr/share/qemu/ovmf-x86_64-code.bin",
        "/usr/share/qemu-efi-aarch64/QEMU_EFI.fd",
    ];
    for c in &candidates {
        if Path::new(c).exists() {
            return Some((*c).to_string());
        }
    }
    None
}

/// Find the `swtpm` binary on PATH.
pub fn find_swtpm() -> Option<String> {
    which("swtpm")
}

/// Lightweight `which`: scan `$PATH` for an executable.
fn which(name: &str) -> Option<String> {
    if let Ok(path_env) = std::env::var("PATH") {
        for dir in path_env.split(':') {
            if dir.is_empty() {
                continue;
            }
            let candidate = Path::new(dir).join(name);
            if candidate.is_file() {
                return Some(candidate.to_string_lossy().to_string());
            }
        }
    }
    None
}

/// Distro id from `/etc/os-release` (e.g. "ubuntu", "fedora", "arch").
pub fn detect_distro() -> Option<String> {
    let contents = std::fs::read_to_string("/etc/os-release").ok()?;
    parse_os_release_id(&contents)
}

/// Parse the `ID=` field of an `/etc/os-release` file. Exposed for testing.
pub fn parse_os_release_id(contents: &str) -> Option<String> {
    for line in contents.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("ID=") {
            let val = rest.trim().trim_matches('"').trim_matches('\'').to_string();
            if !val.is_empty() {
                return Some(val);
            }
        }
    }
    None
}

/// Map a distro id to a human-friendly family for install hints.
pub fn distro_family(id: &str) -> DistroFamily {
    let lower = id.to_lowercase();
    if matches!(
        lower.as_str(),
        "debian" | "ubuntu" | "linuxmint" | "pop" | "elementary" | "kali" | "neon" | "zorin"
    ) {
        DistroFamily::Debian
    } else if matches!(
        lower.as_str(),
        "fedora" | "rhel" | "centos" | "rocky" | "almalinux" | "ol" | "nobara"
    ) {
        DistroFamily::Fedora
    } else if matches!(
        lower.as_str(),
        "arch" | "manjaro" | "endeavouros" | "cachyos" | "garuda"
    ) {
        DistroFamily::Arch
    } else if matches!(
        lower.as_str(),
        "opensuse-tumbleweed" | "opensuse-leap" | "sles"
    ) {
        DistroFamily::Suse
    } else if lower.contains("opensuse") {
        DistroFamily::Suse
    } else {
        DistroFamily::Unknown
    }
}

/// Linux distribution family for install-command hints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DistroFamily {
    Debian,
    Fedora,
    Arch,
    Suse,
    Unknown,
}

impl DistroFamily {
    /// One-shot install command for the missing QEMU/libvirt/OVMF/swtpm stack.
    pub fn install_command(&self) -> &'static str {
        match self {
            DistroFamily::Debian => {
                "sudo apt install qemu-system-x86 libvirt-daemon-system ovmf swtpm"
            },
            DistroFamily::Fedora => "sudo dnf install qemu-kvm libvirt edk2-ovmf swtpm",
            DistroFamily::Arch => "sudo pacman -S qemu-full libvirt edk2-ovmf swtpm",
            DistroFamily::Suse => "sudo zypper install qemu libvirt qemu-ovmf-x86_64 swtpm",
            DistroFamily::Unknown => {
                "# Install: qemu-system-x86_64, libvirt-daemon, OVMF firmware, swtpm"
            },
        }
    }

    /// Command to add the current user to the libvirt+kvm groups.
    pub fn group_command(&self) -> &'static str {
        "sudo usermod -aG libvirt,kvm $USER && newgrp libvirt"
    }

    /// Command to enable + start the libvirtd service.
    pub fn enable_libvirtd_command(&self) -> &'static str {
        "sudo systemctl enable --now libvirtd"
    }

    pub fn label(&self) -> &'static str {
        match self {
            DistroFamily::Debian => "Debian / Ubuntu",
            DistroFamily::Fedora => "Fedora / RHEL",
            DistroFamily::Arch => "Arch / Manjaro",
            DistroFamily::Suse => "openSUSE / SLES",
            DistroFamily::Unknown => "Unknown distro",
        }
    }
}

/// Run every probe and return a populated SystemCheck.
///
/// Pure read-only — never modifies anything. Safe to call on every wizard tick.
pub fn run_system_check() -> SystemCheck {
    SystemCheck {
        kvm_module_loaded: check_kvm_module(),
        kvm_dev_present: check_kvm_dev(),
        libvirtd_running: check_libvirtd_running(),
        user_in_libvirt_group: user_in_group("libvirt"),
        user_in_kvm_group: user_in_group("kvm"),
        qemu_binary_found: find_qemu_binary(),
        ovmf_present: find_ovmf(),
        swtpm_binary_found: find_swtpm(),
        distro_id: detect_distro(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_kvm_module_found() {
        let modules = "\
kvm_intel 425984 0 - Live 0x0000000000000000
kvm 1085440 1 kvm_intel, Live 0x0000000000000000
something_else 12345 0 - Live 0x0000000000000000
";
        assert!(parse_kvm_module_present(modules));
    }

    #[test]
    fn parse_kvm_module_amd_variant() {
        let modules = "kvm_amd 167936 0 - Live 0x0000000000000000\n";
        assert!(parse_kvm_module_present(modules));
    }

    #[test]
    fn parse_kvm_module_missing() {
        let modules = "ext4 1048576 1 - Live 0x0000000000000000\n";
        assert!(!parse_kvm_module_present(modules));
    }

    #[test]
    fn parse_kvm_module_empty() {
        assert!(!parse_kvm_module_present(""));
    }

    #[test]
    fn parse_id_output_match() {
        let s = "neindev8 wheel libvirt kvm docker\n";
        assert!(parse_id_output(s, "libvirt"));
        assert!(parse_id_output(s, "kvm"));
        assert!(parse_id_output(s, "wheel"));
        assert!(!parse_id_output(s, "root"));
    }

    #[test]
    fn parse_id_output_no_match() {
        let s = "user1 user2 user3";
        assert!(!parse_id_output(s, "libvirt"));
        assert!(!parse_id_output(s, ""));
    }

    #[test]
    fn parse_id_output_partial_doesnt_match() {
        let s = "libvirt-qemu kvm-user";
        assert!(!parse_id_output(s, "libvirt"));
        assert!(!parse_id_output(s, "kvm"));
    }

    #[test]
    fn etc_group_membership_found() {
        let group_file = "\
root:x:0:
libvirt:x:128:alice,bob,carol
kvm:x:108:alice
sudo:x:27:alice,carol
";
        assert!(parse_etc_group_membership(group_file, "libvirt", "alice"));
        assert!(parse_etc_group_membership(group_file, "libvirt", "bob"));
        assert!(parse_etc_group_membership(group_file, "kvm", "alice"));
        assert!(!parse_etc_group_membership(group_file, "kvm", "bob"));
        assert!(!parse_etc_group_membership(
            group_file,
            "nonexistent",
            "alice"
        ));
    }

    #[test]
    fn etc_group_membership_empty_members() {
        let group_file = "libvirt:x:128:\n";
        assert!(!parse_etc_group_membership(group_file, "libvirt", "alice"));
    }

    #[test]
    fn etc_group_membership_handles_malformed() {
        let group_file = "this is not a valid line\nmalformed\n";
        assert!(!parse_etc_group_membership(group_file, "libvirt", "alice"));
    }

    #[test]
    fn os_release_id_ubuntu() {
        let content = "\
NAME=\"Ubuntu\"
VERSION=\"22.04.3 LTS (Jammy Jellyfish)\"
ID=ubuntu
ID_LIKE=debian
";
        assert_eq!(parse_os_release_id(content).as_deref(), Some("ubuntu"));
    }

    #[test]
    fn os_release_id_fedora_quoted() {
        let content = "ID=\"fedora\"\nVERSION_ID=39\n";
        assert_eq!(parse_os_release_id(content).as_deref(), Some("fedora"));
    }

    #[test]
    fn os_release_id_missing() {
        let content = "NAME=Foo\nVERSION_ID=1\n";
        assert_eq!(parse_os_release_id(content), None);
    }

    #[test]
    fn distro_family_mapping() {
        assert_eq!(distro_family("ubuntu"), DistroFamily::Debian);
        assert_eq!(distro_family("debian"), DistroFamily::Debian);
        assert_eq!(distro_family("pop"), DistroFamily::Debian);
        assert_eq!(distro_family("fedora"), DistroFamily::Fedora);
        assert_eq!(distro_family("centos"), DistroFamily::Fedora);
        assert_eq!(distro_family("rocky"), DistroFamily::Fedora);
        assert_eq!(distro_family("arch"), DistroFamily::Arch);
        assert_eq!(distro_family("manjaro"), DistroFamily::Arch);
        assert_eq!(distro_family("opensuse-tumbleweed"), DistroFamily::Suse);
        assert_eq!(distro_family("opensuse"), DistroFamily::Suse);
        assert_eq!(distro_family("notarealdistro"), DistroFamily::Unknown);
    }

    #[test]
    fn distro_family_install_commands_nonempty() {
        for fam in [
            DistroFamily::Debian,
            DistroFamily::Fedora,
            DistroFamily::Arch,
            DistroFamily::Suse,
            DistroFamily::Unknown,
        ] {
            assert!(!fam.install_command().is_empty());
            assert!(!fam.group_command().is_empty());
            assert!(!fam.enable_libvirtd_command().is_empty());
            assert!(!fam.label().is_empty());
        }
    }

    #[test]
    fn system_check_all_essentials_logic() {
        let mut sc = SystemCheck::default();
        assert!(!sc.all_essentials_ok());
        sc.kvm_module_loaded = true;
        sc.kvm_dev_present = true;
        sc.libvirtd_running = true;
        sc.user_in_libvirt_group = true;
        sc.qemu_binary_found = Some("/usr/bin/qemu-system-x86_64".into());
        assert!(sc.all_essentials_ok());
    }

    #[test]
    fn system_check_missing_pieces_list() {
        let sc = SystemCheck::default();
        let missing = sc.missing_pieces();
        assert!(missing.contains(&"kvm-module"));
        assert!(missing.contains(&"libvirtd"));
        assert!(missing.contains(&"qemu"));
        assert!(missing.contains(&"ovmf"));
        assert!(missing.contains(&"swtpm"));
    }

    #[test]
    fn run_system_check_is_pure() {
        // Should not panic or mutate state — just call it twice and confirm same outputs.
        let a = run_system_check();
        let b = run_system_check();
        assert_eq!(a.kvm_dev_present, b.kvm_dev_present);
        assert_eq!(a.kvm_module_loaded, b.kvm_module_loaded);
    }
}
