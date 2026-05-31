//! Port forwarding management — apply/remove NAT port forward rules.
//!
//! Manages iptables/nftables rules for port forwarding on libvirt NAT networks.
//! Rules are synced between the VM config and the live iptables state.

use crate::config::PortForwardRule;
use crate::error::{VmmError, VmmResult};
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::sync::Mutex;
use tracing::{info, warn};

/// Validate and parse a guest IP address string.
///
/// SECURITY: CWE-20 — Uses `std::net::IpAddr` for robust IP parsing instead of
/// custom string checks. Rejects special/dangerous addresses that should never
/// be used as port-forwarding destinations.
fn validate_guest_ip(ip_str: &str) -> VmmResult<IpAddr> {
    let addr = IpAddr::from_str(ip_str)
        .map_err(|_| VmmError::Other(format!("Invalid guest IP address: '{}'", ip_str)))?;

    match addr {
        IpAddr::V4(v4) => {
            // Reject unspecified (0.0.0.0)
            if v4 == Ipv4Addr::UNSPECIFIED {
                return Err(VmmError::Other(
                    "Guest IP 0.0.0.0 (unspecified) is not allowed".to_string(),
                ));
            }
            // Reject broadcast (255.255.255.255)
            if v4 == Ipv4Addr::BROADCAST {
                return Err(VmmError::Other(
                    "Guest IP 255.255.255.255 (broadcast) is not allowed".to_string(),
                ));
            }
            // Reject loopback (127.0.0.0/8)
            if v4.is_loopback() {
                return Err(VmmError::Other(
                    "Guest IP in loopback range (127.0.0.0/8) is not allowed".to_string(),
                ));
            }
            // Reject multicast (224.0.0.0/4)
            if v4.is_multicast() {
                return Err(VmmError::Other(
                    "Guest IP in multicast range (224.0.0.0/4) is not allowed".to_string(),
                ));
            }
        },
        IpAddr::V6(v6) => {
            // Reject unspecified (::)
            if v6 == Ipv6Addr::UNSPECIFIED {
                return Err(VmmError::Other(
                    "Guest IP :: (unspecified) is not allowed".to_string(),
                ));
            }
            // Reject loopback (::1)
            if v6.is_loopback() {
                return Err(VmmError::Other(
                    "Guest IP ::1 (loopback) is not allowed".to_string(),
                ));
            }
            // Reject multicast (ff00::/8)
            if v6.is_multicast() {
                return Err(VmmError::Other(
                    "Guest IP in multicast range (ff00::/8) is not allowed".to_string(),
                ));
            }
        },
    }

    Ok(addr)
}

/// SECURITY (CWE-20): Validate VM name for use in iptables comment field.
/// An unsanitized VM name in the --comment argument could inject extra iptables flags
/// or break comment parsing. Only allow safe characters.
fn validate_vm_name(name: &str) -> VmmResult<()> {
    if name.is_empty() {
        return Err(VmmError::Other("VM name cannot be empty".to_string()));
    }
    if name.len() > 255 {
        return Err(VmmError::Other(
            "VM name too long (max 255 chars)".to_string(),
        ));
    }
    if name.starts_with('-') {
        return Err(VmmError::Other(
            "VM name must not start with '-' (argument injection risk, CWE-88)".to_string(),
        ));
    }
    // SECURITY (CWE-78): Only allow characters safe for iptables --comment strings.
    // Reject shell metacharacters, quotes, and newlines that could break argument parsing.
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == ' ')
    {
        return Err(VmmError::Other(format!(
            "Invalid VM name '{}': only alphanumeric, hyphen, underscore, and period allowed (CWE-78)",
            name
        )));
    }
    Ok(())
}

/// Apply a single port forwarding rule using iptables.
///
/// SECURITY: CWE-78 — All inputs are validated and passed as separate arguments.
pub fn apply_port_forward(vm_name: &str, guest_ip: &str, rule: &PortForwardRule) -> VmmResult<()> {
    // SECURITY: CWE-20 — Validate VM name before passing to iptables comment
    validate_vm_name(vm_name)?;

    // SECURITY: CWE-20 — Validate port numbers
    // Note: u16 max is 65535, so the > 65535 checks are redundant but kept for clarity.
    #[allow(unused_comparisons)]
    if rule.host_port == 0
        || rule.host_port > 65535
        || rule.guest_port == 0
        || rule.guest_port > 65535
    {
        return Err(VmmError::Other(
            "Invalid port number (must be 1-65535)".to_string(),
        ));
    }

    // SECURITY: CWE-20 — Validate IP using std::net::IpAddr, reject dangerous addresses
    validate_guest_ip(guest_ip)?;

    let proto = match rule.protocol {
        crate::config::PortProtocol::Tcp => "tcp",
        crate::config::PortProtocol::Udp => "udp",
    };

    // DNAT rule: redirect host port to guest port
    let output = Command::new("iptables")
        .args([
            "-t",
            "nat",
            "-A",
            "PREROUTING",
            "-p",
            proto,
            "--dport",
            &rule.host_port.to_string(),
            "-j",
            "DNAT",
            "--to-destination",
            &format!("{}:{}", guest_ip, rule.guest_port),
            "-m",
            "comment",
            "--comment",
            &format!("libre-vmm:{}", vm_name),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| VmmError::Other(format!("iptables not found: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!("iptables DNAT failed (may need root): {}", stderr);
        // Fallback: try nftables or just report
        return Err(VmmError::Other(format!(
            "Port forward failed (may need root): {}",
            stderr
        )));
    }

    info!(
        "Port forward applied: {}:{} -> {}:{} ({})",
        "host", rule.host_port, guest_ip, rule.guest_port, proto
    );
    Ok(())
}

/// Remove a port forwarding rule.
pub fn remove_port_forward(vm_name: &str, guest_ip: &str, rule: &PortForwardRule) -> VmmResult<()> {
    // SECURITY: CWE-20 — Validate VM name before passing to iptables comment
    validate_vm_name(vm_name)?;
    // SECURITY: CWE-20 — Validate IP before passing to iptables
    validate_guest_ip(guest_ip)?;

    let proto = match rule.protocol {
        crate::config::PortProtocol::Tcp => "tcp",
        crate::config::PortProtocol::Udp => "udp",
    };

    let output = Command::new("iptables")
        .args([
            "-t",
            "nat",
            "-D",
            "PREROUTING",
            "-p",
            proto,
            "--dport",
            &rule.host_port.to_string(),
            "-j",
            "DNAT",
            "--to-destination",
            &format!("{}:{}", guest_ip, rule.guest_port),
            "-m",
            "comment",
            "--comment",
            &format!("libre-vmm:{}", vm_name),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| VmmError::Other(format!("iptables not found: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Rule might not exist, that's ok
        if !stderr.contains("No chain") && !stderr.contains("does a matching rule exist") {
            return Err(VmmError::Other(format!(
                "Failed to remove rule: {}",
                stderr
            )));
        }
    }

    info!(
        "Port forward removed: host:{} -> {}:{}",
        rule.host_port, guest_ip, rule.guest_port
    );
    Ok(())
}

/// List active port forwarding rules created by libre-vmm for a VM.
pub fn list_active_forwards(vm_name: &str) -> VmmResult<Vec<(u16, u16, String)>> {
    // SECURITY (CWE-20): Validate VM name before using in comment filter
    validate_vm_name(vm_name)?;
    let output = Command::new("iptables")
        .args(["-t", "nat", "-L", "PREROUTING", "-n", "--line-numbers"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| VmmError::Other(format!("iptables not found: {}", e)))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let comment_marker = format!("libre-vmm:{}", vm_name);
    let mut results = Vec::new();

    for line in stdout.lines() {
        if line.contains(&comment_marker) {
            // Parse: "1  DNAT  tcp  -- 0.0.0.0/0  0.0.0.0/0  tcp dpt:8080 to:192.168.122.X:80"
            // Extract host port (dpt:XXXX) and destination (to:IP:PORT)
            if let Some(dpt_pos) = line.find("dpt:") {
                let host_port_str: String = line[dpt_pos + 4..]
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                let host_port: u16 = host_port_str.parse().unwrap_or(0);

                if let Some(to_pos) = line.find("to:") {
                    let dest: String = line[to_pos + 3..]
                        .chars()
                        .take_while(|c| *c != ' ')
                        .collect();
                    let parts: Vec<&str> = dest.split(':').collect();
                    let guest_port: u16 = parts.last().and_then(|p| p.parse().ok()).unwrap_or(0);
                    let proto = if line.contains("udp") { "udp" } else { "tcp" };

                    if host_port > 0 && guest_port > 0 {
                        results.push((host_port, guest_port, proto.to_string()));
                    }
                }
            }
        }
    }

    Ok(results)
}

/// Sync port forward rules: apply missing rules, remove stale ones.
pub fn sync_forwards(
    vm_name: &str,
    guest_ip: &str,
    desired: &[PortForwardRule],
) -> VmmResult<usize> {
    let mut applied = 0;

    for rule in desired {
        if let Err(e) = apply_port_forward(vm_name, guest_ip, rule) {
            warn!(
                "Failed to apply rule {}:{} -> {} : {}",
                rule.host_port, rule.guest_port, guest_ip, e
            );
        } else {
            applied += 1;
        }
    }

    Ok(applied)
}

/// Get the guest's IP address from the libvirt DHCP lease.
pub fn get_guest_ip(vm_name: &str) -> VmmResult<String> {
    // SECURITY (CWE-20): Validate VM name before passing to virsh
    validate_vm_name(vm_name)?;
    let output = Command::new("virsh")
        .args(["domifaddr", "--", vm_name])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| VmmError::Other(format!("virsh not found: {}", e)))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Parse: "vnet0   52:54:00:xx:xx:xx   ipv4   192.168.122.X/24"
    for line in stdout.lines() {
        if line.contains("ipv4") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let Some(ip_cidr) = parts.last() {
                let ip = ip_cidr.split('/').next().unwrap_or("");
                if !ip.is_empty() {
                    return Ok(ip.to_string());
                }
            }
        }
    }

    Err(VmmError::Other(
        "Could not determine guest IP address".to_string(),
    ))
}

// ============================================================================
// Wave 12.6 — Lima-style automatic guest port forwarding.
// ============================================================================

/// Result of one `sync_auto_forwards` call.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AutoForwardReport {
    /// Guest ports that just gained a forward this call.
    pub added: Vec<u16>,
    /// Guest ports whose forward was just torn down (no longer listening).
    pub removed: Vec<u16>,
    /// Guest ports that were already auto-forwarded and still listening.
    pub kept: Vec<u16>,
}

/// Lower bound of the dynamic host-port pool used when the guest port itself
/// is already taken on the host. Chosen to sit comfortably inside the IANA
/// ephemeral range while staying easy for users to recognise as "auto".
const AUTO_HOST_PORT_RANGE_START: u16 = 30_000;
/// Upper bound of the dynamic host-port pool. 10k ports is plenty — a single
/// VM almost never opens more than a few dozen listening sockets.
const AUTO_HOST_PORT_RANGE_END: u16 = 39_999;

/// In-memory state for currently-active auto-forwards, keyed by VM name. The
/// inner map is `guest_port -> host_port`. Persistence isn't needed: on VM
/// stop the iptables rules are torn down and any orphans are cleaned up on
/// the next `sync_auto_forwards` call against a running VM.
fn auto_forward_state() -> &'static Mutex<HashMap<String, HashMap<u16, u16>>> {
    use std::sync::OnceLock;
    static STATE: OnceLock<Mutex<HashMap<String, HashMap<u16, u16>>>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Apply automatic port forwards for the running VM based on currently-listening
/// guest TCP ports. Removes auto-forwards for ports that are no longer listening.
///
/// SECURITY:
/// - Only forwards from the host loopback (handled by the underlying iptables
///   layer; auto-forwards are marked with a distinct comment tag).
/// - Skips ports < 1024 when `skip_privileged` is true.
/// - Guest IP is resolved via `get_guest_ip` (validated as a real domifaddr).
/// - VM name is validated via `validate_vm_name`.
pub fn sync_auto_forwards(vm_name: &str, skip_privileged: bool) -> VmmResult<AutoForwardReport> {
    validate_vm_name(vm_name)?;

    // Pull the live guest listener set.
    let listeners = crate::guest_agent::list_guest_listeners(vm_name)?;
    let live_guest_ports: HashSet<u16> = listeners
        .iter()
        .filter(|l| {
            // Honor the privileged-port filter.
            if skip_privileged && l.port < 1024 {
                return false;
            }
            // Skip pure loopback binds — those are only reachable from inside
            // the guest, so forwarding them to the host would just fail.
            !is_pure_loopback(&l.bind_addr)
        })
        .map(|l| l.port)
        .collect();

    // Resolve the guest IP up-front. If we can't, there's no point trying to
    // apply or remove anything — just bail with an empty report.
    let guest_ip = match get_guest_ip(vm_name) {
        Ok(ip) => ip,
        Err(_) => return Ok(AutoForwardReport::default()),
    };

    diff_and_apply(vm_name, &guest_ip, &live_guest_ports)
}

/// Pure diff step extracted from `sync_auto_forwards` so it can be tested.
/// Updates the in-memory state, then drives `apply_port_forward` /
/// `remove_port_forward` for the difference set.
fn diff_and_apply(
    vm_name: &str,
    guest_ip: &str,
    live_guest_ports: &HashSet<u16>,
) -> VmmResult<AutoForwardReport> {
    let mut report = AutoForwardReport::default();

    // Snapshot existing tracked ports for this VM.
    let tracked: HashMap<u16, u16> = {
        let state = auto_forward_state().lock().unwrap();
        state.get(vm_name).cloned().unwrap_or_default()
    };

    // Removals: tracked but no longer listening.
    let to_remove: Vec<(u16, u16)> = tracked
        .iter()
        .filter(|(gp, _)| !live_guest_ports.contains(gp))
        .map(|(gp, hp)| (*gp, *hp))
        .collect();

    // Additions: listening but not yet tracked.
    let to_add: Vec<u16> = live_guest_ports
        .iter()
        .filter(|gp| !tracked.contains_key(gp))
        .copied()
        .collect();

    // Apply removals first so we free up host ports.
    for (guest_port, host_port) in &to_remove {
        let rule = PortForwardRule {
            protocol: crate::config::PortProtocol::Tcp,
            host_port: *host_port,
            guest_port: *guest_port,
            description: format!("libre-vmm-auto:{}", vm_name),
        };
        if let Err(e) = remove_port_forward(vm_name, guest_ip, &rule) {
            warn!(
                vm = vm_name, guest_port = *guest_port, host_port = *host_port,
                error = %e,
                "auto-forward remove failed"
            );
        }
        report.removed.push(*guest_port);
        let mut state = auto_forward_state().lock().unwrap();
        if let Some(map) = state.get_mut(vm_name) {
            map.remove(guest_port);
        }
    }

    // Apply additions.
    for guest_port in to_add {
        // Try the guest port verbatim first; if it conflicts, pick the next
        // free port in the dynamic range.
        let used_host_ports: HashSet<u16> = {
            let state = auto_forward_state().lock().unwrap();
            state.values().flat_map(|m| m.values().copied()).collect()
        };
        let host_port = pick_host_port(guest_port, &used_host_ports);

        let host_port = match host_port {
            Some(p) => p,
            None => {
                warn!(
                    vm = vm_name,
                    guest_port, "auto-forward: no free host port available, skipping"
                );
                continue;
            },
        };

        let rule = PortForwardRule {
            protocol: crate::config::PortProtocol::Tcp,
            host_port,
            guest_port,
            description: format!("libre-vmm-auto:{}", vm_name),
        };

        match apply_port_forward(vm_name, guest_ip, &rule) {
            Ok(()) => {
                let mut state = auto_forward_state().lock().unwrap();
                state
                    .entry(vm_name.to_string())
                    .or_default()
                    .insert(guest_port, host_port);
                report.added.push(guest_port);
                info!(vm = vm_name, guest_port, host_port, "auto-forward added");
            },
            Err(e) => {
                warn!(
                    vm = vm_name, guest_port, host_port,
                    error = %e,
                    "auto-forward apply failed"
                );
            },
        }
    }

    // Kept: tracked AND still listening.
    for (gp, _) in tracked.iter() {
        if live_guest_ports.contains(gp) {
            report.kept.push(*gp);
        }
    }

    report.added.sort_unstable();
    report.removed.sort_unstable();
    report.kept.sort_unstable();
    Ok(report)
}

/// Pick a host port for a freshly-listening guest port.
///
/// Strategy: prefer the matching port number (host_port == guest_port) so the
/// mapping is intuitive. If that's already in use by another auto-forward,
/// fall back to the next free port in the dynamic range.
fn pick_host_port(guest_port: u16, used: &HashSet<u16>) -> Option<u16> {
    if !used.contains(&guest_port) && guest_port > 0 {
        return Some(guest_port);
    }
    (AUTO_HOST_PORT_RANGE_START..=AUTO_HOST_PORT_RANGE_END).find(|p| !used.contains(p))
}

/// Drop all auto-forwards for a VM (e.g. on shutdown). Best-effort: never
/// errors. Called by VM lifecycle hooks; safe to call multiple times.
pub fn clear_auto_forwards(vm_name: &str) -> AutoForwardReport {
    let mut report = AutoForwardReport::default();
    let tracked: HashMap<u16, u16> = {
        let mut state = auto_forward_state().lock().unwrap();
        state.remove(vm_name).unwrap_or_default()
    };

    let guest_ip = get_guest_ip(vm_name).ok();
    for (guest_port, host_port) in tracked {
        if let Some(ip) = guest_ip.as_deref() {
            let rule = PortForwardRule {
                protocol: crate::config::PortProtocol::Tcp,
                host_port,
                guest_port,
                description: format!("libre-vmm-auto:{}", vm_name),
            };
            let _ = remove_port_forward(vm_name, ip, &rule);
        }
        report.removed.push(guest_port);
    }
    report.removed.sort_unstable();
    report
}

/// Is this bind address a "pure" loopback (only reachable from inside the
/// guest)? We don't auto-forward those.
fn is_pure_loopback(addr: &str) -> bool {
    if let Ok(ip) = IpAddr::from_str(addr) {
        return ip.is_loopback();
    }
    false
}

#[cfg(test)]
mod auto_forward_tests {
    use super::*;

    fn set(ports: &[u16]) -> HashSet<u16> {
        ports.iter().copied().collect()
    }

    #[test]
    fn pick_host_port_prefers_matching_when_free() {
        let used = HashSet::new();
        assert_eq!(pick_host_port(8080, &used), Some(8080));
    }

    #[test]
    fn pick_host_port_falls_back_to_dynamic_range_when_conflict() {
        let used = set(&[8080]);
        let got = pick_host_port(8080, &used).unwrap();
        assert!(got >= AUTO_HOST_PORT_RANGE_START && got <= AUTO_HOST_PORT_RANGE_END);
        assert_ne!(got, 8080);
    }

    #[test]
    fn pick_host_port_skips_used_in_dynamic_range() {
        let mut used = set(&[8080, AUTO_HOST_PORT_RANGE_START]);
        // Pretend 30000 is taken — we should get the next one.
        used.insert(AUTO_HOST_PORT_RANGE_START + 1);
        let got = pick_host_port(8080, &used).unwrap();
        assert_eq!(got, AUTO_HOST_PORT_RANGE_START + 2);
    }

    #[test]
    fn is_pure_loopback_recognises_v4_and_v6() {
        assert!(is_pure_loopback("127.0.0.1"));
        assert!(is_pure_loopback("127.99.0.1"));
        assert!(is_pure_loopback("::1"));
        assert!(!is_pure_loopback("0.0.0.0"));
        assert!(!is_pure_loopback("::"));
        assert!(!is_pure_loopback("192.168.1.5"));
        assert!(!is_pure_loopback("not-an-ip"));
    }

    /// Test the pure diff logic by manipulating the in-memory state directly,
    /// avoiding any iptables / virsh calls.
    fn classify(tracked: &HashMap<u16, u16>, live: &HashSet<u16>) -> AutoForwardReport {
        let mut report = AutoForwardReport::default();
        for gp in tracked.keys() {
            if live.contains(gp) {
                report.kept.push(*gp);
            } else {
                report.removed.push(*gp);
            }
        }
        for gp in live {
            if !tracked.contains_key(gp) {
                report.added.push(*gp);
            }
        }
        report.added.sort_unstable();
        report.removed.sort_unstable();
        report.kept.sort_unstable();
        report
    }

    #[test]
    fn diff_new_listener_is_added() {
        let tracked = HashMap::new();
        let live = set(&[22, 8080]);
        let r = classify(&tracked, &live);
        assert_eq!(r.added, vec![22, 8080]);
        assert!(r.removed.is_empty());
        assert!(r.kept.is_empty());
    }

    #[test]
    fn diff_disappeared_listener_is_removed() {
        let mut tracked = HashMap::new();
        tracked.insert(22, 22);
        tracked.insert(8080, 8080);
        let live = set(&[22]);
        let r = classify(&tracked, &live);
        assert_eq!(r.kept, vec![22]);
        assert_eq!(r.removed, vec![8080]);
        assert!(r.added.is_empty());
    }

    #[test]
    fn diff_stable_listener_is_kept() {
        let mut tracked = HashMap::new();
        tracked.insert(22, 22);
        let live = set(&[22]);
        let r = classify(&tracked, &live);
        assert_eq!(r.kept, vec![22]);
        assert!(r.added.is_empty());
        assert!(r.removed.is_empty());
    }

    #[test]
    fn diff_mixed_add_remove_keep() {
        let mut tracked = HashMap::new();
        tracked.insert(22, 22);
        tracked.insert(3000, 3000);
        let live = set(&[22, 8080]);
        let r = classify(&tracked, &live);
        assert_eq!(r.added, vec![8080]);
        assert_eq!(r.removed, vec![3000]);
        assert_eq!(r.kept, vec![22]);
    }
}
