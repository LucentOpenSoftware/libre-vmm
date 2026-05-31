//! vmm-core — Core VM management library for Libre VMM.
//!
//! Wraps libvirt to provide a simple, high-level API for creating and
//! managing QEMU/KVM virtual machines.

pub mod auto_snapshot;
pub mod backup;
pub mod balloon;
pub mod clone;
pub mod config;
pub mod connection;
pub mod container;
pub mod disk;
pub mod disk_manage;
pub mod domain;
pub mod encryption;
pub mod error;
pub mod gpu;
pub mod guest_agent;
pub mod guest_file_manager;
pub mod guest_tools;
pub mod host_guest;
pub mod host_inhibitor;
pub mod import;
pub mod iso_detect;
pub mod iso_library;
pub mod looking_glass;
pub mod migration;
pub mod monitor;
pub mod network;
pub mod network_conditioner;
pub mod network_editor;
pub mod notifications;
pub mod novnc;
pub mod ova;
pub mod pci;
pub mod port_forward;
pub mod preferences;
pub mod qemu_archs;
pub mod remote;
pub mod resource_limits;
pub mod restricted;
pub mod rollback;
pub mod screen_recording;
pub mod snapshot;
pub mod storage;
pub mod system_check;
pub mod task;
pub mod template;
pub mod template_library;
pub mod tpm;
pub mod unattended;
pub mod usb;
pub mod vfio;
pub mod xml_builder;

pub use clone::CloneType;
pub use config::PortForwardRule;
pub use config::{CpuTopology, VfioDeviceConfig};
pub use config::{VmConfig, VmConfigIo};
pub use connection::HypervisorConnection;
pub use container::{
    dispatch as dispatch_container_backend, validate_container_name,
    Backend as ContainerBackendKind, Container, ContainerBackend, ContainerConfig, ContainerState,
    DockerBackend, NspawnBackend, PodmanBackend,
};
pub use disk_manage::DiskUsageInfo;
pub use domain::{VmInfo, VmState};
pub use error::{VmmError, VmmResult};
pub use gpu::GpuCapabilities;
pub use guest_agent::GuestInfo;
pub use guest_agent::GuestTcpListener;
pub use guest_tools::{GuestOsFamily, GuestToolsStatus, InstallStep, LinuxDistro};
pub use host_guest::HostGuestCapabilities;
pub use iso_detect::DetectedOs;
pub use iso_library::IsoEntry;
pub use migration::{MigrationHandle, MigrationOptions, MigrationProgress, MigrationType};
pub use monitor::VmMonitor;
pub use network_conditioner::NetworkCondition;
pub use notifications::NotificationSettings;
pub use port_forward::AutoForwardReport;
pub use preferences::Preferences;
pub use qemu_archs::QemuArchIo;
pub use qemu_archs::{ArchDefaults, BoxType, CpuFeature, CpuModel, MachineType, QemuArch};
pub use remote::{RemoteHost, RemoteHostsConfig, SshTunnel};
pub use resource_limits::ResourceLimits;
pub use resource_limits::ResourceLimitsXml;
pub use restricted::{Operation as RestrictionOperation, RestrictionPolicy};
pub use rollback::RollbackConfig;
pub use snapshot::SnapshotInfo;
pub use task::{TaskHandle, TaskInfo, TaskManager, TaskProgress, TaskStatus};
pub use template::OsTemplate;
pub use template_library::VmTemplate;
pub use tpm::TpmVersion;
pub use usb::UsbDevice;
