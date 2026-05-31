//! Pure data types for Libre VMM — no I/O, no platform code.
//!
//! These types compile on Linux, Windows, and macOS. They form the contract
//! between the GUI, CLI, API, and the platform-specific hypervisor backends.
//!
//! ## Wave 16.A1 (Windows port foundation)
//!
//! This crate is the first foundation step for native Windows host support.
//! It was extracted from `vmm-core` so that `vmm-gui`, `vmm-cli`, and `vmm-api`
//! can eventually compile on Windows without dragging in libvirt FFI or any
//! Unix-only system calls. See `docs/WINDOWS-PORT.md` for the strategic plan.
//!
//! ## What lives here
//!
//! - Pure data types (structs and enums)
//! - Their `Default`, `Display`, `serde::Serialize`, `serde::Deserialize` impls
//! - Pure helper functions on those types (e.g. `CpuTopology::total_vcpus()`)
//! - Validation helpers that take `&str` or `&Self` and return errors —
//!   but NOT validators that touch the filesystem
//!
//! ## What does NOT live here
//!
//! - `std::fs`, `std::process`, `std::env::*`
//! - `dirs::*` or any path-discovery
//! - `libc::*`, `os::unix::*`, `os::windows::*`
//! - libvirt FFI or any other native bindings
//! - Manager / operator types (TaskManager, AutoSnapshot scheduler, ...)

pub mod auto_snapshot;
pub mod config;
pub mod looking_glass;
pub mod qemu_archs;
pub mod resource_limits;
pub mod tpm;

pub use auto_snapshot::AutoSnapshotConfig;
pub use config::*;
pub use looking_glass::LookingGlassConfig;
pub use qemu_archs::{ArchDefaults, BoxType, CpuFeature, CpuModel, MachineType, QemuArch};
pub use resource_limits::{
    CpuLimits, CpuPin, DiskIoLimits, MemoryLimits, NetworkLimits, ResourceLimits,
};
pub use tpm::TpmVersion;
