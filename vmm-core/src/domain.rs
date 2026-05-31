//! VM state and information types.

use serde::{Deserialize, Serialize};

/// Runtime information about a VM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmInfo {
    pub name: String,
    pub uuid: String,
    pub state: VmState,
    pub vcpus: u32,
    pub memory_mib: u64,
    pub cpu_time_ns: u64,
}

/// VM power state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum VmState {
    Running,
    Paused,
    ShuttingDown,
    Off,
    Crashed,
    Suspended,
    Unknown,
}

impl std::fmt::Display for VmState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VmState::Running => write!(f, "Running"),
            VmState::Paused => write!(f, "Paused"),
            VmState::ShuttingDown => write!(f, "Shutting Down"),
            VmState::Off => write!(f, "Off"),
            VmState::Crashed => write!(f, "Crashed"),
            VmState::Suspended => write!(f, "Suspended"),
            VmState::Unknown => write!(f, "Unknown"),
        }
    }
}

impl VmState {
    /// Zero-allocation string representation (avoids format!() / Display heap alloc).
    pub fn as_str(&self) -> &'static str {
        match self {
            VmState::Running => "Running",
            VmState::Paused => "Paused",
            VmState::ShuttingDown => "Shutting Down",
            VmState::Off => "Off",
            VmState::Crashed => "Crashed",
            VmState::Suspended => "Suspended",
            VmState::Unknown => "Unknown",
        }
    }

    /// Whether the VM can be started.
    pub fn can_start(&self) -> bool {
        matches!(self, VmState::Off | VmState::Crashed)
    }

    /// Whether the VM can be shut down.
    pub fn can_stop(&self) -> bool {
        matches!(self, VmState::Running | VmState::Paused)
    }

    /// Whether the VM can be paused.
    pub fn can_pause(&self) -> bool {
        matches!(self, VmState::Running)
    }

    /// Whether the VM can be resumed.
    pub fn can_resume(&self) -> bool {
        matches!(self, VmState::Paused | VmState::Suspended)
    }
}
