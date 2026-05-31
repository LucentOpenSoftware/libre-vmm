//! Pure data types from `vmm-core/src/looking_glass.rs`. No I/O, no platform code.
//!
//! ## Wave 16.A1 (Windows port foundation)
//! Extracted from vmm-core so vmm-gui can compile on any platform.
//!
//! The Looking Glass client discovery, SHM file creation, IVSHMEM XML emission,
//! and client launching all stay in `vmm-core::looking_glass`. Only the
//! `LookingGlassConfig` struct moves here.

use serde::{Deserialize, Serialize};

/// Looking Glass configuration for a VM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LookingGlassConfig {
    /// Enable Looking Glass for this VM.
    pub enabled: bool,
    /// IVSHMEM shared memory size in MiB (default: 64).
    /// Must be large enough for the VM's display resolution:
    /// 1080p ≈ 32 MiB, 1440p ≈ 64 MiB, 4K ≈ 128 MiB.
    pub ivshmem_size_mib: u32,
    /// Auto-launch Looking Glass client when VM starts.
    pub auto_launch: bool,
    /// Custom path to the Looking Glass client binary.
    pub client_path: Option<String>,
}

impl Default for LookingGlassConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            ivshmem_size_mib: 64,
            auto_launch: true,
            client_path: None,
        }
    }
}
