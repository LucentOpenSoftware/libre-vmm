//! Pure data types from `vmm-core/src/resource_limits.rs`. No I/O, no platform code.
//!
//! ## Wave 16.A1 (Windows port foundation)
//! Extracted from vmm-core so vmm-gui can compile on any platform.
//!
//! Resource Limits / QoS — CPU pinning, memory tuning, disk I/O throttle, network bandwidth.
//!
//! Maps to libvirt XML elements: `<cputune>`, `<memtune>`, `<iotune>`, `<bandwidth>`.
//! These allow fine-grained control over VM resource usage.
//!
//! The XML emission (`to_xml`, `disk_iotune_xml`, `network_bandwidth_xml`) and
//! the host CPU discovery helpers stay in `vmm-core::resource_limits` because
//! they read `/sys/devices/system/cpu/possible` (Linux-specific I/O). Only the
//! pure data structs and their `has_any` / `summary` helpers move here.

use serde::{Deserialize, Serialize};

/// Resource limits for a VM. All fields are optional — only set limits are applied.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourceLimits {
    /// CPU tuning options.
    #[serde(default)]
    pub cpu: CpuLimits,

    /// Memory tuning options.
    #[serde(default)]
    pub memory: MemoryLimits,

    /// Disk I/O throttle options.
    #[serde(default)]
    pub disk_io: DiskIoLimits,

    /// Network bandwidth limits.
    #[serde(default)]
    pub network: NetworkLimits,
}

/// CPU tuning: pinning, shares, and quota.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CpuLimits {
    /// CPU shares (relative weight, default 1024). Higher = more CPU time.
    /// Maps to `<shares>` in `<cputune>`.
    #[serde(default)]
    pub shares: Option<u64>,

    /// Hard CPU time limit: microseconds of CPU time per period.
    /// Maps to `<quota>` in `<cputune>`. -1 = no limit.
    #[serde(default)]
    pub quota: Option<i64>,

    /// Length of the scheduling period in microseconds (default: 100000 = 100ms).
    /// Maps to `<period>` in `<cputune>`.
    #[serde(default)]
    pub period: Option<u64>,

    /// Pin vCPUs to specific host CPUs.
    /// Index = vCPU number, value = host CPU set (e.g., "0-3", "0,2,4").
    /// Maps to `<vcpupin>` elements in `<cputune>`.
    #[serde(default)]
    pub pinning: Vec<CpuPin>,
}

/// A single vCPU → host CPU pinning rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuPin {
    /// vCPU number (0-based).
    pub vcpu: u32,
    /// Host CPU set string (e.g., "0-3", "0,2,4,6").
    pub cpuset: String,
}

/// Memory tuning: limits and balloon settings.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryLimits {
    /// Hard memory limit in KiB. VM cannot use more than this.
    /// Maps to `<hard_limit>` in `<memtune>`.
    #[serde(default)]
    pub hard_limit_kib: Option<u64>,

    /// Soft memory limit in KiB. Best-effort limit under memory pressure.
    /// Maps to `<soft_limit>` in `<memtune>`.
    #[serde(default)]
    pub soft_limit_kib: Option<u64>,

    /// Minimum guaranteed memory in KiB (balloon floor).
    /// Maps to `<min_guarantee>` in `<memtune>`.
    #[serde(default)]
    pub min_guarantee_kib: Option<u64>,

    /// Swap hard limit in KiB. Total memory+swap limit.
    /// Maps to `<swap_hard_limit>` in `<memtune>`.
    #[serde(default)]
    pub swap_hard_limit_kib: Option<u64>,
}

/// Disk I/O throttle: IOPS and throughput limits.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DiskIoLimits {
    /// Total bytes per second (read + write combined).
    /// Maps to `<total_bytes_sec>` in `<iotune>`.
    #[serde(default)]
    pub total_bytes_sec: Option<u64>,

    /// Read bytes per second limit.
    /// Maps to `<read_bytes_sec>` in `<iotune>`.
    #[serde(default)]
    pub read_bytes_sec: Option<u64>,

    /// Write bytes per second limit.
    /// Maps to `<write_bytes_sec>` in `<iotune>`.
    #[serde(default)]
    pub write_bytes_sec: Option<u64>,

    /// Total IOPS (read + write combined).
    /// Maps to `<total_iops_sec>` in `<iotune>`.
    #[serde(default)]
    pub total_iops_sec: Option<u64>,

    /// Read IOPS limit.
    /// Maps to `<read_iops_sec>` in `<iotune>`.
    #[serde(default)]
    pub read_iops_sec: Option<u64>,

    /// Write IOPS limit.
    /// Maps to `<write_iops_sec>` in `<iotune>`.
    #[serde(default)]
    pub write_iops_sec: Option<u64>,
}

/// Network bandwidth limits.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetworkLimits {
    /// Inbound average bandwidth in KB/s.
    /// Maps to `<inbound average='...'/>` in `<bandwidth>`.
    #[serde(default)]
    pub inbound_average_kbps: Option<u64>,

    /// Inbound peak bandwidth in KB/s.
    #[serde(default)]
    pub inbound_peak_kbps: Option<u64>,

    /// Inbound burst size in KB.
    #[serde(default)]
    pub inbound_burst_kb: Option<u64>,

    /// Outbound average bandwidth in KB/s.
    /// Maps to `<outbound average='...'/>` in `<bandwidth>`.
    #[serde(default)]
    pub outbound_average_kbps: Option<u64>,

    /// Outbound peak bandwidth in KB/s.
    #[serde(default)]
    pub outbound_peak_kbps: Option<u64>,

    /// Outbound burst size in KB.
    #[serde(default)]
    pub outbound_burst_kb: Option<u64>,
}

impl ResourceLimits {
    /// Whether any limits are set at all.
    pub fn has_any(&self) -> bool {
        self.cpu.has_any()
            || self.memory.has_any()
            || self.disk_io.has_any()
            || self.network.has_any()
    }

    /// Get a human-readable summary of active limits.
    pub fn summary(&self) -> Vec<String> {
        let mut parts = Vec::new();

        if let Some(shares) = self.cpu.shares {
            parts.push(format!("CPU shares: {}", shares));
        }
        if let Some(quota) = self.cpu.quota {
            if quota > 0 {
                let period = self.cpu.period.unwrap_or(100_000);
                let pct = (quota as f64 / period as f64) * 100.0;
                parts.push(format!("CPU limit: {:.0}%", pct));
            }
        }
        if !self.cpu.pinning.is_empty() {
            parts.push(format!("{} vCPU pins", self.cpu.pinning.len()));
        }
        if let Some(hard) = self.memory.hard_limit_kib {
            parts.push(format!("Mem hard limit: {} MiB", hard / 1024));
        }
        if let Some(soft) = self.memory.soft_limit_kib {
            parts.push(format!("Mem soft limit: {} MiB", soft / 1024));
        }
        if let Some(total) = self.disk_io.total_bytes_sec {
            parts.push(format!("Disk I/O: {} MB/s", total / 1_048_576));
        }
        if let Some(iops) = self.disk_io.total_iops_sec {
            parts.push(format!("Disk IOPS: {}", iops));
        }
        if let Some(avg) = self.network.inbound_average_kbps {
            parts.push(format!("Net in: {} KB/s", avg));
        }
        if let Some(avg) = self.network.outbound_average_kbps {
            parts.push(format!("Net out: {} KB/s", avg));
        }

        parts
    }
}

impl CpuLimits {
    pub fn has_any(&self) -> bool {
        self.shares.is_some()
            || self.quota.is_some()
            || self.period.is_some()
            || !self.pinning.is_empty()
    }
}

impl MemoryLimits {
    pub fn has_any(&self) -> bool {
        self.hard_limit_kib.is_some()
            || self.soft_limit_kib.is_some()
            || self.min_guarantee_kib.is_some()
            || self.swap_hard_limit_kib.is_some()
    }
}

impl DiskIoLimits {
    pub fn has_any(&self) -> bool {
        self.total_bytes_sec.is_some()
            || self.read_bytes_sec.is_some()
            || self.write_bytes_sec.is_some()
            || self.total_iops_sec.is_some()
            || self.read_iops_sec.is_some()
            || self.write_iops_sec.is_some()
    }
}

impl NetworkLimits {
    pub fn has_any(&self) -> bool {
        self.inbound_average_kbps.is_some() || self.outbound_average_kbps.is_some()
    }
}

/// Validate that all CPU numbers in a cpuset string are within the host's CPU range.
/// cpuset format: comma-separated values or ranges, e.g. "0-3,5,7" or "0,2,4-6".
/// Returns false if any CPU number >= host_cpu_count.
///
/// This is a pure function that takes the host CPU count as input — the actual
/// discovery of host CPUs (reading `/sys/devices/system/cpu/possible`) stays in
/// `vmm-core::resource_limits`.
pub fn validate_cpuset_range(cpuset: &str, host_cpu_count: usize) -> bool {
    if host_cpu_count == 0 {
        return false;
    }
    let max_cpu = host_cpu_count - 1;
    for part in cpuset.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some(dash_pos) = part.find('-') {
            // Range: "start-end"
            let start_str = &part[..dash_pos];
            let end_str = &part[dash_pos + 1..];
            let start: usize = match start_str.parse() {
                Ok(v) => v,
                Err(_) => return false,
            };
            let end: usize = match end_str.parse() {
                Ok(v) => v,
                Err(_) => return false,
            };
            if start > end || end > max_cpu {
                return false;
            }
        } else {
            // Single CPU number
            let cpu: usize = match part.parse() {
                Ok(v) => v,
                Err(_) => return false,
            };
            if cpu > max_cpu {
                return false;
            }
        }
    }
    true
}
