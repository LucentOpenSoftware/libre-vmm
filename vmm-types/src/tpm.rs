//! Pure data types from `vmm-core/src/tpm.rs`. No I/O, no platform code.
//!
//! ## Wave 16.A1 (Windows port foundation)
//! Extracted from vmm-core so vmm-gui can compile on any platform.
//!
//! The swtpm process spawning, state directory management, and the libvirt
//! TPM XML helper remain in `vmm-core::tpm`. Only the `TpmVersion` enum that
//! lives inside `VmConfig` moves here.

use serde::{Deserialize, Serialize};

/// TPM version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TpmVersion {
    /// TPM 1.2 (legacy, rarely needed)
    V1_2,
    /// TPM 2.0 (modern, required for Windows 11)
    V2_0,
}

impl std::fmt::Display for TpmVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TpmVersion::V1_2 => write!(f, "1.2"),
            TpmVersion::V2_0 => write!(f, "2.0"),
        }
    }
}

impl Default for TpmVersion {
    fn default() -> Self {
        TpmVersion::V2_0
    }
}
