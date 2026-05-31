//! QEMU architecture definitions — machine types, CPU models, and device defaults.
//!
//! Inspired by UTM's approach: architecture → machine type → device defaults.
//! This module provides the data model for Box 2 (Hardware Lab) cross-architecture
//! emulation, supporting all major QEMU target architectures.
//!
//! ## Wave 16.A1 (Windows port foundation)
//! The pure data types (`QemuArch`, `BoxType`, `MachineType`, `CpuModel`,
//! `CpuFeature`, `ArchDefaults`) and their pure pattern-match methods moved
//! to `vmm-types::qemu_archs`. They are re-exported here so existing
//! `use vmm_core::qemu_archs::*` imports keep working.
//!
//! What stays here is the I/O-touching part: the hardcoded Unix QEMU binary
//! path, the binary-availability check (filesystem), and the QEMU command-
//! line probing for installed architectures and queryable machine types.
//! The path/availability helpers are exposed via the `QemuArchIo` extension
//! trait, which is auto-imported when the `vmm_core::qemu_archs` module is
//! brought into scope.

pub use vmm_types::qemu_archs::{
    ArchDefaults, BoxType, CpuFeature, CpuModel, MachineType, QemuArch,
};

/// Extension trait adding host-filesystem helpers to the pure `QemuArch`
/// data type. The path it builds (`/usr/bin/qemu-system-…`) is Linux-specific;
/// on Windows we'll resolve QEMU via the bundled installation directory
/// (Wave B2 of the Windows port plan).
pub trait QemuArchIo {
    /// Path to the QEMU system binary.
    fn qemu_binary(&self) -> String;

    /// Whether the QEMU binary is likely available on this system.
    fn is_binary_available(&self) -> bool;
}

impl QemuArchIo for QemuArch {
    fn qemu_binary(&self) -> String {
        format!("/usr/bin/qemu-system-{}", self.qemu_suffix())
    }

    fn is_binary_available(&self) -> bool {
        std::path::Path::new(&self.qemu_binary()).exists()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// System runtime detection
// ─────────────────────────────────────────────────────────────────────────────

/// Discover which QEMU system emulators are installed.
pub fn detect_installed_architectures() -> Vec<QemuArch> {
    QemuArch::all()
        .into_iter()
        .filter(|a| a.is_binary_available())
        .collect()
}

/// Query QEMU for available machine types for a given architecture.
/// Falls back to the hardcoded list if QEMU query fails.
pub fn query_machine_types(arch: &QemuArch) -> Vec<MachineType> {
    let binary = arch.qemu_binary();
    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance to child process.
    let output = std::process::Command::new(&binary)
        .args(["-machine", "help"])
        .stdin(std::process::Stdio::null())
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let mut machines = Vec::new();
            for line in stdout.lines().skip(1) {
                // Format: "machine_name   description"
                let parts: Vec<&str> = line.splitn(2, char::is_whitespace).collect();
                if let Some(name) = parts.first() {
                    let name = name.trim();
                    if name.is_empty() || name == "none" {
                        continue;
                    }
                    let desc = parts.get(1).map(|s| s.trim()).unwrap_or("");
                    let is_default = name == arch.default_machine();
                    machines.push(MachineType::new(name, desc, is_default));
                }
            }
            if machines.is_empty() {
                arch.machine_types()
            } else {
                machines
            }
        },
        _ => arch.machine_types(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qemu_binary_path() {
        assert_eq!(
            QemuArch::X86_64.qemu_binary(),
            "/usr/bin/qemu-system-x86_64"
        );
    }
}
