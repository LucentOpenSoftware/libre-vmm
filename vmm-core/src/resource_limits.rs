//! Resource Limits / QoS — CPU pinning, memory tuning, disk I/O throttle, network bandwidth.
//!
//! Maps to libvirt XML elements: `<cputune>`, `<memtune>`, `<iotune>`, `<bandwidth>`.
//! These allow fine-grained control over VM resource usage.
//!
//! ## Wave 16.A1 (Windows port foundation)
//! The pure data structs (`ResourceLimits`, `CpuLimits`, `MemoryLimits`,
//! `DiskIoLimits`, `NetworkLimits`, `CpuPin`) and their `has_any` / `summary`
//! helpers moved to `vmm-types::resource_limits`. They are re-exported here so
//! existing `use vmm_core::resource_limits::*` imports keep working.
//!
//! The XML emission (`to_xml`, `disk_iotune_xml`, `network_bandwidth_xml`) and
//! host CPU discovery (`host_cpu_count`, `all_cpus_set`) stay here because
//! they read `/sys/devices/system/cpu/possible` — Linux-specific I/O. They are
//! exposed via the `ResourceLimitsXml` extension trait, which is auto-imported
//! when the `vmm_core::resource_limits` module is brought into scope.

use std::sync::OnceLock;

pub use vmm_types::resource_limits::{
    validate_cpuset_range, CpuLimits, CpuPin, DiskIoLimits, MemoryLimits, NetworkLimits,
    ResourceLimits,
};

/// Extension trait adding libvirt-XML rendering to the pure `ResourceLimits`
/// data type. Implementation lives in vmm-core because it reads the host's
/// `/sys/devices/system/cpu/possible` to clamp cpuset values, which is a
/// Linux-specific filesystem call.
pub trait ResourceLimitsXml {
    fn to_xml(&self) -> String;
    fn disk_iotune_xml(&self) -> String;
    fn network_bandwidth_xml(&self) -> String;
}

impl ResourceLimitsXml for ResourceLimits {
    fn to_xml(&self) -> String {
        let mut xml = String::new();

        // CPU tuning
        if self.cpu.has_any() {
            xml.push_str("  <cputune>\n");
            if let Some(shares) = self.cpu.shares {
                xml.push_str(&format!("    <shares>{}</shares>\n", shares));
            }
            if let Some(period) = self.cpu.period {
                xml.push_str(&format!("    <period>{}</period>\n", period));
            }
            if let Some(quota) = self.cpu.quota {
                xml.push_str(&format!("    <quota>{}</quota>\n", quota));
            }
            let host_cpus = host_cpu_count();
            for pin in &self.cpu.pinning {
                // SECURITY (CWE-20): Validate vcpu is a reasonable number.
                if pin.vcpu > 4096 {
                    tracing::warn!(
                        "Skipping vCPU pin: vcpu ID {} exceeds maximum 4096",
                        pin.vcpu
                    );
                    continue;
                }
                // SECURITY (CWE-91 / XML Injection): Sanitize cpuset by allowlist.
                if !pin
                    .cpuset
                    .chars()
                    .all(|c| c.is_ascii_digit() || c == ',' || c == '-')
                {
                    tracing::warn!(
                        "Skipping vCPU pin {}: cpuset '{}' contains invalid characters",
                        pin.vcpu,
                        pin.cpuset
                    );
                    continue;
                }
                let safe_cpuset = &pin.cpuset;
                if safe_cpuset.is_empty() {
                    continue;
                }
                if !validate_cpuset_range(safe_cpuset, host_cpus) {
                    tracing::warn!(
                        "Skipping vCPU pin {}: cpuset '{}' exceeds host CPU count {}",
                        pin.vcpu,
                        safe_cpuset,
                        host_cpus
                    );
                    continue;
                }
                xml.push_str(&format!(
                    "    <vcpupin vcpu='{}' cpuset='{}'/>\n",
                    pin.vcpu, safe_cpuset
                ));
            }
            xml.push_str("  </cputune>\n");
        }

        // Memory tuning
        if self.memory.has_any() {
            xml.push_str("  <memtune>\n");
            if let Some(hard) = self.memory.hard_limit_kib {
                xml.push_str(&format!(
                    "    <hard_limit unit='KiB'>{}</hard_limit>\n",
                    hard
                ));
            }
            if let Some(soft) = self.memory.soft_limit_kib {
                xml.push_str(&format!(
                    "    <soft_limit unit='KiB'>{}</soft_limit>\n",
                    soft
                ));
            }
            if let Some(min) = self.memory.min_guarantee_kib {
                xml.push_str(&format!(
                    "    <min_guarantee unit='KiB'>{}</min_guarantee>\n",
                    min
                ));
            }
            if let Some(swap) = self.memory.swap_hard_limit_kib {
                xml.push_str(&format!(
                    "    <swap_hard_limit unit='KiB'>{}</swap_hard_limit>\n",
                    swap
                ));
            }
            xml.push_str("  </memtune>\n");
        }

        xml
    }

    fn disk_iotune_xml(&self) -> String {
        if !self.disk_io.has_any() {
            return String::new();
        }

        let mut xml = String::from("      <iotune>\n");
        if let Some(v) = self.disk_io.total_bytes_sec {
            xml.push_str(&format!(
                "        <total_bytes_sec>{}</total_bytes_sec>\n",
                v
            ));
        }
        if let Some(v) = self.disk_io.read_bytes_sec {
            xml.push_str(&format!("        <read_bytes_sec>{}</read_bytes_sec>\n", v));
        }
        if let Some(v) = self.disk_io.write_bytes_sec {
            xml.push_str(&format!(
                "        <write_bytes_sec>{}</write_bytes_sec>\n",
                v
            ));
        }
        if let Some(v) = self.disk_io.total_iops_sec {
            xml.push_str(&format!("        <total_iops_sec>{}</total_iops_sec>\n", v));
        }
        if let Some(v) = self.disk_io.read_iops_sec {
            xml.push_str(&format!("        <read_iops_sec>{}</read_iops_sec>\n", v));
        }
        if let Some(v) = self.disk_io.write_iops_sec {
            xml.push_str(&format!("        <write_iops_sec>{}</write_iops_sec>\n", v));
        }
        xml.push_str("      </iotune>\n");
        xml
    }

    fn network_bandwidth_xml(&self) -> String {
        if !self.network.has_any() {
            return String::new();
        }

        let mut xml = String::from("      <bandwidth>\n");
        if self.network.inbound_average_kbps.is_some() {
            let mut inbound = String::from("        <inbound");
            if let Some(avg) = self.network.inbound_average_kbps {
                inbound.push_str(&format!(" average='{}'", avg));
            }
            if let Some(peak) = self.network.inbound_peak_kbps {
                inbound.push_str(&format!(" peak='{}'", peak));
            }
            if let Some(burst) = self.network.inbound_burst_kb {
                inbound.push_str(&format!(" burst='{}'", burst));
            }
            inbound.push_str("/>\n");
            xml.push_str(&inbound);
        }
        if self.network.outbound_average_kbps.is_some() {
            let mut outbound = String::from("        <outbound");
            if let Some(avg) = self.network.outbound_average_kbps {
                outbound.push_str(&format!(" average='{}'", avg));
            }
            if let Some(peak) = self.network.outbound_peak_kbps {
                outbound.push_str(&format!(" peak='{}'", peak));
            }
            if let Some(burst) = self.network.outbound_burst_kb {
                outbound.push_str(&format!(" burst='{}'", burst));
            }
            outbound.push_str("/>\n");
            xml.push_str(&outbound);
        }
        xml.push_str("      </bandwidth>\n");
        xml
    }
}

/// Get the maximum CPU ID available on the host, plus one (i.e., the count of possible CPUs).
///
/// SECURITY: Uses /sys/devices/system/cpu/possible instead of std::thread::available_parallelism()
/// because the latter returns only CPUs visible to the current cgroup/taskset, which can be
/// artificially restricted. (CWE-20: Improper Input Validation)
pub fn host_cpu_count() -> usize {
    static CPU_COUNT: OnceLock<usize> = OnceLock::new();
    *CPU_COUNT.get_or_init(host_cpu_count_inner)
}

fn host_cpu_count_inner() -> usize {
    // Try /sys/devices/system/cpu/possible first (format: "0-N" or "0")
    if let Ok(contents) = std::fs::read_to_string("/sys/devices/system/cpu/possible") {
        let trimmed = contents.trim();
        if let Some(dash_pos) = trimmed.find('-') {
            if let Ok(max_id) = trimmed[dash_pos + 1..].parse::<usize>() {
                return max_id + 1;
            }
        }
        if let Ok(single) = trimmed.parse::<usize>() {
            return single + 1;
        }
    }

    if let Ok(entries) = std::fs::read_dir("/sys/devices/system/cpu") {
        let count = entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name();
                let name = name.to_string_lossy();
                name.starts_with("cpu") && name[3..].chars().all(|c| c.is_ascii_digit())
            })
            .count();
        if count > 0 {
            return count;
        }
    }

    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

/// Generate a CPU set string for all host CPUs (e.g., "0-7").
pub fn all_cpus_set() -> String {
    let count = host_cpu_count();
    if count <= 1 {
        "0".to_string()
    } else {
        format!("0-{}", count - 1)
    }
}
