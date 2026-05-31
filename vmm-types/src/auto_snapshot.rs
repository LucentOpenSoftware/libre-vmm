//! Pure data types from `vmm-core/src/auto_snapshot.rs`. No I/O, no platform code.
//!
//! ## Wave 16.A1 (Windows port foundation)
//! Extracted from vmm-core so vmm-gui can compile on any platform.
//!
//! The scheduler thread, libvirt access, and filesystem snapshot bookkeeping
//! all remain in `vmm-core::auto_snapshot`. Only the `AutoSnapshotConfig`
//! struct that lives inside `VmConfig` moves here.

use serde::{Deserialize, Serialize};

/// Auto-snapshot configuration for a VM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoSnapshotConfig {
    /// Whether auto-snapshots are enabled for this VM.
    #[serde(default)]
    pub enabled: bool,
    /// Interval in hours between auto-snapshots (1, 4, 8, 12, 24, 168).
    #[serde(default = "default_interval")]
    pub interval_hours: u32,
    /// Maximum number of auto-snapshots to retain.
    #[serde(default = "default_retention")]
    pub retention: u32,
}

fn default_interval() -> u32 {
    24
}
fn default_retention() -> u32 {
    5
}

impl Default for AutoSnapshotConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_hours: default_interval(),
            retention: default_retention(),
        }
    }
}
