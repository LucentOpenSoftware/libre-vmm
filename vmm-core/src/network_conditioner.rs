//! Network Conditioner — simulate network conditions (latency, packet loss, bandwidth).
//!
//! Uses Linux `tc` (traffic control) with the `netem` qdisc to apply
//! network impairments to a VM's tap interface. Useful for testing
//! applications under degraded network conditions.

use crate::error::{VmmError, VmmResult};
use std::process::Command;
use tracing::info;
use virt::connect::Connect;
use virt::domain::Domain;

/// A network condition profile describing simulated impairments.
#[derive(Debug, Clone)]
pub struct NetworkCondition {
    /// Human-readable name (e.g., "3G", "High Latency", "Packet Loss").
    pub name: String,
    /// Added latency in milliseconds.
    pub delay_ms: u32,
    /// Latency variance (jitter) in milliseconds.
    pub jitter_ms: u32,
    /// Packet loss percentage (0.0 - 100.0).
    pub loss_percent: f32,
    /// Optional bandwidth limit in kbps. None means unlimited.
    pub bandwidth_kbps: Option<u32>,
}

impl NetworkCondition {
    /// Built-in presets for common network conditions.
    pub fn presets() -> Vec<Self> {
        vec![
            NetworkCondition {
                name: "No limit".to_string(),
                delay_ms: 0,
                jitter_ms: 0,
                loss_percent: 0.0,
                bandwidth_kbps: None,
            },
            NetworkCondition {
                name: "3G".to_string(),
                delay_ms: 100,
                jitter_ms: 30,
                loss_percent: 1.5,
                bandwidth_kbps: Some(1500),
            },
            NetworkCondition {
                name: "LTE".to_string(),
                delay_ms: 30,
                jitter_ms: 10,
                loss_percent: 0.1,
                bandwidth_kbps: Some(50000),
            },
            NetworkCondition {
                name: "WiFi (poor)".to_string(),
                delay_ms: 50,
                jitter_ms: 40,
                loss_percent: 5.0,
                bandwidth_kbps: Some(5000),
            },
            NetworkCondition {
                name: "Satellite".to_string(),
                delay_ms: 600,
                jitter_ms: 50,
                loss_percent: 2.0,
                bandwidth_kbps: Some(10000),
            },
            NetworkCondition {
                name: "100% Loss".to_string(),
                delay_ms: 0,
                jitter_ms: 0,
                loss_percent: 100.0,
                bandwidth_kbps: None,
            },
        ]
    }

    /// Find a preset by name (case-insensitive).
    pub fn preset_by_name(name: &str) -> Option<Self> {
        Self::presets()
            .into_iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
    }
}

/// Apply a network condition to a VM's tap interface using `tc` (traffic control).
///
/// This first clears any existing qdisc on the interface, then applies the
/// `netem` qdisc with the specified parameters. If bandwidth limiting is
/// requested, a `tbf` (token bucket filter) qdisc is chained after netem.
pub fn apply_condition(vm_name: &str, condition: &NetworkCondition) -> VmmResult<()> {
    // SECURITY: Validate VM name before passing to virsh (CWE-88)
    if vm_name.starts_with('-') || vm_name.is_empty() {
        return Err(VmmError::NetworkError(format!(
            "Invalid VM name for network conditioning: {}",
            vm_name
        )));
    }
    // SECURITY: Validate parameters before passing to `tc` (CWE-20)
    if condition.loss_percent < 0.0 || condition.loss_percent > 100.0 {
        return Err(VmmError::NetworkError(format!(
            "Packet loss must be 0-100%, got {:.1}%",
            condition.loss_percent
        )));
    }
    if condition.delay_ms > 60000 {
        return Err(VmmError::NetworkError(format!(
            "Delay must be ≤60000ms, got {}ms",
            condition.delay_ms
        )));
    }
    if condition.jitter_ms > condition.delay_ms {
        return Err(VmmError::NetworkError(format!(
            "Jitter ({}) must not exceed delay ({})",
            condition.jitter_ms, condition.delay_ms
        )));
    }
    // SECURITY: CWE-20 — Validate bandwidth_kbps to prevent absurd values
    // that could cause integer overflow in burst calculation or tc misbehavior.
    if let Some(bw) = condition.bandwidth_kbps {
        if bw == 0 {
            return Err(VmmError::NetworkError(
                "Bandwidth must be greater than 0 kbps".to_string(),
            ));
        }
        if bw > 10_000_000 {
            return Err(VmmError::NetworkError(format!(
                "Bandwidth must be ≤10,000,000 kbps (10 Gbps), got {} kbps",
                bw
            )));
        }
    }

    let iface = find_vm_tap_interface(vm_name)?;

    info!(
        "Applying network condition '{}' to VM '{}' (iface: {})",
        condition.name, vm_name, iface
    );

    // Step 1: Clear any existing qdisc (ignore errors if none exists)
    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance to child process.
    let _ = Command::new("tc")
        .args(["qdisc", "del", "dev", &iface, "root"])
        .stdin(std::process::Stdio::null())
        .output();

    // Step 2: If this is "No limit" (all zeros), we're done after clearing
    if condition.delay_ms == 0
        && condition.jitter_ms == 0
        && condition.loss_percent == 0.0
        && condition.bandwidth_kbps.is_none()
    {
        info!("Network conditions cleared for VM '{}' (No limit)", vm_name);
        return Ok(());
    }

    // Step 3: Build the netem qdisc command
    let mut args: Vec<String> = vec![
        "qdisc".to_string(),
        "add".to_string(),
        "dev".to_string(),
        iface.clone(),
        "root".to_string(),
    ];

    // If we need bandwidth limiting, netem must be a child of a classful qdisc.
    // For simplicity, if only netem params are set (no bandwidth), apply netem directly.
    // If bandwidth is also set, use handle/parent chaining.
    if condition.bandwidth_kbps.is_some() {
        args.push("handle".to_string());
        args.push("1:".to_string());
    }

    args.push("netem".to_string());

    if condition.delay_ms > 0 {
        args.push("delay".to_string());
        args.push(format!("{}ms", condition.delay_ms));
        if condition.jitter_ms > 0 {
            args.push(format!("{}ms", condition.jitter_ms));
        }
    }

    if condition.loss_percent > 0.0 {
        args.push("loss".to_string());
        args.push(format!("{:.1}%", condition.loss_percent));
    }

    let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance to child process.
    let output = Command::new("tc")
        .args(&args_ref)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| VmmError::NetworkError(format!("tc command not found: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VmmError::NetworkError(format!(
            "Failed to apply netem qdisc: {}",
            stderr
        )));
    }

    // Step 4: If bandwidth limiting is requested, add a tbf qdisc as child
    if let Some(bw_kbps) = condition.bandwidth_kbps {
        let rate = format!("{}kbit", bw_kbps);
        // burst = rate / 8 / 10 (100ms of traffic), minimum 1600 bytes
        let burst_bytes = std::cmp::max((bw_kbps as u64 * 1000 / 8 / 10) as u64, 1600);
        let burst = format!("{}", burst_bytes);
        // latency for tbf queue — how long packets can wait
        let latency = "50ms";

        // SECURITY: CWE-403 — Close stdin to prevent FD inheritance.
        let tbf_output = Command::new("tc")
            .args([
                "qdisc", "add", "dev", &iface, "parent", "1:1", "handle", "10:", "tbf", "rate",
                &rate, "burst", &burst, "latency", latency,
            ])
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| VmmError::NetworkError(format!("tc command not found: {}", e)))?;

        if !tbf_output.status.success() {
            let stderr = String::from_utf8_lossy(&tbf_output.stderr);
            // Non-fatal: netem is already applied, bandwidth limiting just failed
            tracing::warn!(
                "Failed to apply bandwidth limit (netem still active): {}",
                stderr
            );
        }
    }

    info!(
        "Network condition '{}' applied to VM '{}': delay={}ms jitter={}ms loss={:.1}% bw={:?}kbps",
        condition.name,
        vm_name,
        condition.delay_ms,
        condition.jitter_ms,
        condition.loss_percent,
        condition.bandwidth_kbps,
    );

    Ok(())
}

/// Remove all network conditions from a VM's tap interface (restore normal).
///
/// Deletes the root qdisc, which removes all child qdiscs as well.
pub fn clear_condition(vm_name: &str) -> VmmResult<()> {
    // SECURITY: Validate VM name before passing to virsh (CWE-88)
    if vm_name.starts_with('-') || vm_name.is_empty() {
        return Err(VmmError::NetworkError(format!(
            "Invalid VM name for network conditioning: {}",
            vm_name
        )));
    }
    let iface = find_vm_tap_interface(vm_name)?;

    info!(
        "Clearing network conditions for VM '{}' (iface: {})",
        vm_name, iface
    );

    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance to child process.
    let output = Command::new("tc")
        .args(["qdisc", "del", "dev", &iface, "root"])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| VmmError::NetworkError(format!("tc command not found: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // "RTNETLINK answers: No such file or directory" means no qdisc was set — that's fine
        if !stderr.contains("No such file or directory") {
            return Err(VmmError::NetworkError(format!(
                "Failed to clear network conditions: {}",
                stderr
            )));
        }
    }

    info!("Network conditions cleared for VM '{}'", vm_name);
    Ok(())
}

/// Get the tap interface name for a VM from libvirt.
///
/// Parses the domain XML looking for `<target dev='vnetX'/>` inside
/// `<interface>` elements.
pub fn get_vm_tap_interface(conn: &Connect, vm_name: &str) -> VmmResult<Option<String>> {
    let domain = Domain::lookup_by_name(conn, vm_name).map_err(|_| VmmError::VmNotFound {
        name: vm_name.to_string(),
    })?;

    let xml = domain
        .get_xml_desc(0)
        .map_err(|e| VmmError::NetworkError(format!("Failed to get domain XML: {}", e)))?;

    Ok(extract_tap_device(&xml))
}

/// Extract the tap device name from libvirt domain XML.
///
/// Looks for `<target dev='...'/>` inside `<interface>` blocks.
fn extract_tap_device(xml: &str) -> Option<String> {
    // Find <interface> sections and look for <target dev='...'/>
    let mut search_pos = 0;
    while let Some(iface_start) = xml[search_pos..].find("<interface") {
        let abs_start = search_pos + iface_start;
        let iface_end = match xml[abs_start..].find("</interface>") {
            Some(pos) => abs_start + pos,
            None => break,
        };
        let iface_block = &xml[abs_start..iface_end];

        // Look for <target dev='vnetX'/> or <target dev="vnetX"/>
        if let Some(target_pos) = iface_block.find("<target") {
            let target_str = &iface_block[target_pos..];
            if let Some(dev) = extract_xml_attr(target_str, "dev") {
                // Only return tap/vnet interfaces
                if dev.starts_with("vnet") || dev.starts_with("tap") {
                    // SECURITY (SVE #24): Validate interface name matches expected
                    // pattern: starts with lowercase letters, ends with digits
                    // (e.g., vnet0, tap0, vnet12, tap99).
                    if validate_interface_name(&dev) {
                        return Some(dev);
                    }
                }
            }
        }

        search_pos = iface_end;
    }
    None
}

/// Extract an XML attribute value: `attr='value'` or `attr="value"`.
/// SECURITY (SVE #24): Bounds-checked to return None if closing delimiter is not found
/// or if computed indices would exceed string bounds.
fn extract_xml_attr(s: &str, attr: &str) -> Option<String> {
    let patterns = [format!("{}='", attr), format!("{}=\"", attr)];

    for pat in &patterns {
        if let Some(start) = s.find(pat.as_str()) {
            let val_start = start + pat.len();
            // Bounds check: ensure val_start doesn't exceed string length
            if val_start >= s.len() {
                continue;
            }
            let delim = pat.chars().last()?;
            let remaining = s.get(val_start..)?;
            if let Some(val_end) = remaining.find(delim) {
                let value = &remaining[..val_end];
                // SECURITY (SVE #24): Validate extracted value is a reasonable
                // length and doesn't contain control characters
                if value.len() > 256 || value.chars().any(|c| c.is_control()) {
                    return None;
                }
                return Some(value.to_string());
            }
        }
    }
    None
}

/// SECURITY (SVE #24): Validate that an interface name matches the expected pattern
/// for Linux network interfaces: starts with lowercase letters, followed by optional
/// lowercase letters/digits, and ends with digits (e.g., vnet0, tap0, vnet12, eth0).
/// Max length 16 (IFNAMSIZ - 1 on Linux).
fn validate_interface_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 16 {
        return false;
    }
    // Must match pattern: ^[a-z][a-z0-9]*[0-9]+$
    let chars: Vec<char> = name.chars().collect();
    // First char must be lowercase letter
    if !chars[0].is_ascii_lowercase() {
        return false;
    }
    // Last char must be a digit
    if !chars[chars.len() - 1].is_ascii_digit() {
        return false;
    }
    // All chars must be lowercase alphanumeric
    chars
        .iter()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

/// Find the tap interface for a VM by scanning /sys/class/net.
///
/// Falls back to naming convention if libvirt is not connected.
/// Looks for interfaces matching common libvirt tap naming patterns.
fn find_vm_tap_interface(vm_name: &str) -> VmmResult<String> {
    // SECURITY: Use ONLY `virsh domiflist` to get the interface for THIS specific VM (CWE-863).
    // The previous fallback strategy (scanning /sys/class/net for any tap interface) was
    // removed because it could apply network conditions to the WRONG VM's interface
    // in multi-VM environments, causing cross-VM DoS.
    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance to child process.
    let output = Command::new("virsh")
        .args(["domiflist", "--", vm_name])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| VmmError::NetworkError(format!("Failed to run virsh: {}", e)))?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Parse virsh domiflist output — format:
        //  Interface  Type       Source     Model       MAC
        //  vnet0      bridge     virbr0    virtio      52:54:00:xx:xx:xx
        for line in stdout.lines().skip(2) {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if let Some(iface) = cols.first() {
                let iface = iface.trim();
                if !iface.is_empty() && iface != "-" {
                    // SECURITY: Validate interface name against strict pattern (CWE-78).
                    // The interface name is passed to `tc` commands — a crafted name from
                    // a compromised virsh could cause tc to operate on wrong interfaces.
                    if iface.len() > 16
                        || !iface
                            .chars()
                            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
                    {
                        return Err(VmmError::NetworkError(format!(
                            "Invalid interface name from virsh: '{}'",
                            iface
                        )));
                    }
                    return Ok(iface.to_string());
                }
            }
        }
    }

    Err(VmmError::NetworkError(format!(
        "Cannot identify tap interface for VM '{}'. \
         Ensure the VM is running and has a network interface.",
        vm_name
    )))
}
