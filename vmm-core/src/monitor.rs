//! Performance monitoring — poll libvirt domain stats (CPU%, memory, disk I/O, net I/O).
//!
//! Provides a ring-buffer time-series that the GUI can read to plot real-time charts.

use crate::error::VmmResult;
use std::collections::VecDeque;
use virt::connect::Connect;
use virt::domain::Domain;

/// Maximum number of data points to keep per VM (one per second ≈ 5 min).
const MAX_SAMPLES: usize = 300;

/// A single performance sample captured at a point in time.
#[derive(Debug, Clone, Copy)]
pub struct PerfSample {
    /// Timestamp (seconds since monitoring started for this VM)
    pub time_secs: f64,
    /// CPU usage percentage (0.0–100.0)
    pub cpu_percent: f64,
    /// Memory used in MiB
    pub memory_used_mib: u64,
    /// Memory total in MiB
    pub memory_total_mib: u64,
    /// Disk read bytes since last sample
    pub disk_read_bytes: u64,
    /// Disk write bytes since last sample
    pub disk_write_bytes: u64,
    /// Network RX bytes since last sample
    pub net_rx_bytes: u64,
    /// Network TX bytes since last sample
    pub net_tx_bytes: u64,
}

/// Accumulated raw counters for computing deltas.
#[derive(Debug, Clone, Default)]
struct RawCounters {
    cpu_time_ns: u64,
    disk_rd_bytes: u64,
    disk_wr_bytes: u64,
    net_rx_bytes: u64,
    net_tx_bytes: u64,
}

/// Monitor state for a single VM.
#[derive(Debug, Clone)]
pub struct VmMonitor {
    pub vm_name: String,
    pub samples: VecDeque<PerfSample>,
    prev: RawCounters,
    start: std::time::Instant,
    last_poll: std::time::Instant,
    num_vcpus: u32,
}

impl VmMonitor {
    pub fn new(vm_name: &str) -> Self {
        let now = std::time::Instant::now();
        Self {
            vm_name: vm_name.to_string(),
            samples: VecDeque::with_capacity(MAX_SAMPLES),
            prev: RawCounters::default(),
            start: now,
            last_poll: now,
            num_vcpus: 1,
        }
    }

    /// Poll libvirt for the latest stats and push a sample.
    pub fn poll(&mut self, conn: &Connect) -> VmmResult<()> {
        let domain = Domain::lookup_by_name(conn, &self.vm_name)
            .map_err(|e| crate::error::VmmError::Other(format!("Domain lookup: {}", e)))?;

        let info = domain
            .get_info()
            .map_err(|e| crate::error::VmmError::Other(format!("Domain info: {}", e)))?;

        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.last_poll).as_secs_f64().max(0.001);
        let time_secs = now.duration_since(self.start).as_secs_f64();

        self.num_vcpus = (info.nr_virt_cpu as u32).max(1);

        // CPU: delta cpu_time_ns / elapsed / vcpus → percentage
        let cpu_delta = info.cpu_time.saturating_sub(self.prev.cpu_time_ns);
        let cpu_percent =
            (cpu_delta as f64 / (elapsed * 1e9) / self.num_vcpus as f64 * 100.0).clamp(0.0, 100.0);

        // Memory
        let memory_total_mib = info.memory / 1024; // libvirt returns KiB
        let memory_used_mib = memory_total_mib; // Without balloon stats, report allocated

        // Disk and Network I/O — try libvirt block/interface stats
        let (disk_rd, disk_wr) = read_block_stats(&domain);
        let (net_rx, net_tx) = read_interface_stats(&domain);

        let disk_read_bytes = disk_rd.saturating_sub(self.prev.disk_rd_bytes);
        let disk_write_bytes = disk_wr.saturating_sub(self.prev.disk_wr_bytes);
        let net_rx_bytes = net_rx.saturating_sub(self.prev.net_rx_bytes);
        let net_tx_bytes = net_tx.saturating_sub(self.prev.net_tx_bytes);

        // Skip the very first delta (it's since boot, not since last poll)
        let is_first = self.prev.cpu_time_ns == 0;

        // Update raw counters
        self.prev = RawCounters {
            cpu_time_ns: info.cpu_time,
            disk_rd_bytes: disk_rd,
            disk_wr_bytes: disk_wr,
            net_rx_bytes: net_rx,
            net_tx_bytes: net_tx,
        };
        self.last_poll = now;

        if is_first {
            return Ok(());
        }

        let sample = PerfSample {
            time_secs,
            cpu_percent,
            memory_used_mib,
            memory_total_mib,
            disk_read_bytes,
            disk_write_bytes,
            net_rx_bytes,
            net_tx_bytes,
        };

        if self.samples.len() >= MAX_SAMPLES {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);

        Ok(())
    }

    /// Latest CPU usage.
    pub fn latest_cpu(&self) -> f64 {
        self.samples.back().map(|s| s.cpu_percent).unwrap_or(0.0)
    }

    /// Latest memory usage (MiB).
    pub fn latest_memory_mib(&self) -> u64 {
        self.samples.back().map(|s| s.memory_used_mib).unwrap_or(0)
    }
}

/// Read aggregate block stats (all block devices).
fn read_block_stats(domain: &Domain) -> (u64, u64) {
    // Try common block device names
    for dev in &["vda", "sda", "hda"] {
        if let Ok(stats) = domain.get_block_stats(dev) {
            return (stats.rd_bytes.max(0) as u64, stats.wr_bytes.max(0) as u64);
        }
    }
    (0, 0)
}

/// Read aggregate interface stats (all network interfaces).
fn read_interface_stats(domain: &Domain) -> (u64, u64) {
    // Try common interface names
    for iface in &["vnet0", "macvtap0", "eth0"] {
        if let Ok(stats) = domain.interface_stats(iface) {
            return (stats.rx_bytes.max(0) as u64, stats.tx_bytes.max(0) as u64);
        }
    }
    (0, 0)
}
