//! QEMU Guest Agent integration — query guest OS info via `domain.qemu_agent_command()`.
//!
//! The QEMU Guest Agent (QGA) is a daemon running inside the guest that accepts
//! JSON commands via a virtio-serial channel. This module wraps the most useful
//! queries: IP addresses, hostname, OS info, filesystem usage, and uptime.

use crate::error::{VmmError, VmmResult};
use std::process::Command;
use virt::connect::Connect;

/// Information gathered from the guest agent.
#[derive(Debug, Clone, Default)]
pub struct GuestInfo {
    /// Whether the guest agent is reachable.
    pub agent_available: bool,
    /// Guest hostname.
    pub hostname: Option<String>,
    /// Guest OS name (e.g., "Arch Linux").
    pub os_name: Option<String>,
    /// Guest OS version/kernel.
    pub os_version: Option<String>,
    /// Guest IP addresses.
    pub ip_addresses: Vec<GuestIpAddress>,
    /// Filesystem usage.
    pub filesystems: Vec<GuestFilesystem>,
    /// Guest uptime in seconds (if available).
    pub uptime_secs: Option<u64>,
}

/// A guest IP address.
#[derive(Debug, Clone)]
pub struct GuestIpAddress {
    pub interface: String,
    pub address: String,
    pub prefix: u32,
    pub ip_type: String, // "ipv4" or "ipv6"
}

/// Guest filesystem information.
#[derive(Debug, Clone)]
pub struct GuestFilesystem {
    pub mountpoint: String,
    pub fs_type: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
}

/// Query the QEMU guest agent for all available info.
/// Returns GuestInfo with agent_available=false if agent is not responding.
/// Uses `virsh qemu-agent-command` to communicate, avoiding the need for
/// libvirt-qemu development libraries.
pub fn query_guest_info(_conn: &Connect, vm_name: &str) -> GuestInfo {
    let mut info = GuestInfo::default();

    // Ping the agent first
    if !ping_agent(vm_name) {
        return info;
    }

    info.agent_available = true;

    // Hostname
    info.hostname = agent_command(vm_name, "{\"execute\":\"guest-get-host-name\"}")
        .ok()
        .and_then(|resp| extract_json_str(&resp, "host-name"));

    // OS info
    if let Ok(resp) = agent_command(vm_name, "{\"execute\":\"guest-get-osinfo\"}") {
        info.os_name =
            extract_json_str(&resp, "pretty-name").or_else(|| extract_json_str(&resp, "name"));
        info.os_version = extract_json_str(&resp, "kernel-release")
            .or_else(|| extract_json_str(&resp, "version"));
    }

    // Network interfaces / IP addresses
    if let Ok(resp) = agent_command(vm_name, "{\"execute\":\"guest-network-get-interfaces\"}") {
        info.ip_addresses = parse_network_interfaces(&resp);
    }

    // Filesystems
    if let Ok(resp) = agent_command(vm_name, "{\"execute\":\"guest-get-fsinfo\"}") {
        info.filesystems = parse_filesystems(&resp);
    }

    info
}

/// Check if the guest agent is responding.
fn ping_agent(vm_name: &str) -> bool {
    agent_command(vm_name, "{\"execute\":\"guest-ping\"}").is_ok()
}

/// Send a command to the guest agent via `virsh qemu-agent-command`.
/// This avoids needing libvirt-qemu development libraries.
fn agent_command(vm_name: &str, cmd: &str) -> VmmResult<String> {
    // SECURITY: Validate VM name before passing to virsh (CWE-88)
    if vm_name.starts_with('-') || vm_name.is_empty() {
        return Err(VmmError::Other(format!(
            "Invalid VM name for virsh command: {}",
            vm_name
        )));
    }
    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance to child process.
    let output = Command::new("virsh")
        .args(["qemu-agent-command", "--", vm_name, cmd, "--timeout", "5"])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| VmmError::Other(format!("Failed to run virsh: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VmmError::Other(format!("Guest agent error: {}", stderr)));
    }

    String::from_utf8(output.stdout)
        .map_err(|e| VmmError::Other(format!("Invalid UTF-8 from guest agent: {}", e)))
}

/// Build the QGA JSON payload for `guest-fsfreeze-freeze`.
/// Pure function — extracted so it can be unit-tested without a libvirt connection.
pub(crate) fn freeze_command_json() -> &'static str {
    r#"{"execute":"guest-fsfreeze-freeze"}"#
}

/// Build the QGA JSON payload for `guest-fsfreeze-thaw`.
/// Pure function — extracted so it can be unit-tested without a libvirt connection.
pub(crate) fn thaw_command_json() -> &'static str {
    r#"{"execute":"guest-fsfreeze-thaw"}"#
}

/// Parse the integer returned by guest-fsfreeze-{freeze,thaw}.
/// QGA returns: `{"return": <int>}` — the number of frozen/thawed filesystems.
/// SECURITY (CWE-94): Use serde_json to safely parse instead of string matching,
/// preventing a malicious guest from spoofing the count via crafted output.
pub(crate) fn parse_fsfreeze_count(json: &str) -> Option<u32> {
    let parsed: serde_json::Value = serde_json::from_str(json).ok()?;
    let n = parsed.get("return")?.as_u64()?;
    // SECURITY (CWE-681): Clamp untrusted u64 from guest before narrowing to u32.
    // No realistic guest has more than u32::MAX mounted filesystems.
    Some(n.min(u32::MAX as u64) as u32)
}

/// Freeze guest filesystems via qemu-ga (CWE-78: VM name validated).
///
/// Calls `guest-fsfreeze-freeze` through `virsh qemu-agent-command`. The guest
/// agent flushes all dirty pages and pauses filesystem I/O, so any snapshot
/// taken while frozen is filesystem-consistent ("quiesced", in VMware speak).
///
/// Returns the number of frozen filesystems on success.
/// Returns an error if the agent isn't running, doesn't support freeze, or the
/// call failed for any reason. Callers MUST pair every successful freeze with
/// a thaw — see `crate::snapshot::create_snapshot_quiesced` for the safe pattern.
pub fn freeze_filesystems(vm_name: &str) -> VmmResult<u32> {
    // SECURITY (CWE-78): VM name flows to a virsh subprocess via agent_command.
    // agent_command does a minimal check, but we apply the stricter snapshot-style
    // validator here too as defense in depth.
    validate_vm_name_for_agent(vm_name)?;
    let resp = agent_command(vm_name, freeze_command_json())?;
    parse_fsfreeze_count(&resp).ok_or_else(|| {
        VmmError::Other(format!(
            "Guest agent returned unparseable response to guest-fsfreeze-freeze: {}",
            resp
        ))
    })
}

/// Thaw guest filesystems via qemu-ga. Idempotent — safe to call even if not
/// frozen (qemu-ga returns 0 frozen-now in that case).
///
/// Returns the number of thawed filesystems. Always attempt this after a
/// successful `freeze_filesystems` call, even on error paths; a frozen guest
/// cannot respond to anything else until it is thawed.
pub fn thaw_filesystems(vm_name: &str) -> VmmResult<u32> {
    // SECURITY (CWE-78): VM name flows to virsh subprocess.
    validate_vm_name_for_agent(vm_name)?;
    let resp = agent_command(vm_name, thaw_command_json())?;
    parse_fsfreeze_count(&resp).ok_or_else(|| {
        VmmError::Other(format!(
            "Guest agent returned unparseable response to guest-fsfreeze-thaw: {}",
            resp
        ))
    })
}

/// SECURITY (CWE-78, CWE-88): Strict VM-name validator for guest-agent
/// subprocess calls. Mirrors `validate_vm_name_for_command` in connection.rs
/// (kept local because that one is private to its module).
fn validate_vm_name_for_agent(name: &str) -> VmmResult<()> {
    if name.is_empty() {
        return Err(VmmError::Other(
            "VM name must not be empty (CWE-78)".to_string(),
        ));
    }
    if name.len() > 128 {
        return Err(VmmError::Other(
            "VM name too long for command use (max 128 chars) (CWE-78)".to_string(),
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || " -_.".contains(c))
    {
        return Err(VmmError::Other(format!(
            "VM name '{}' contains unsafe characters for subprocess use (CWE-78)",
            name
        )));
    }
    if name.starts_with('-') || name.starts_with('.') {
        return Err(VmmError::Other(format!(
            "VM name '{}' must not start with '-' or '.' (CWE-88)",
            name
        )));
    }
    if name.contains('\0') {
        return Err(VmmError::Other(
            "VM name must not contain null bytes (CWE-626)".to_string(),
        ));
    }
    Ok(())
}

/// SECURITY: Extract a string value from a QGA JSON response using serde_json (CWE-94).
/// The previous naive string-matching parser was vulnerable to crafted QGA responses
/// that embed fake key-value pairs inside string values, allowing a malicious guest
/// to spoof data returned to the host.
fn extract_json_str(json: &str, key: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(json).ok()?;
    // Check top-level
    if let Some(val) = parsed.get(key).and_then(|v| v.as_str()) {
        return Some(val.to_string());
    }
    // Check inside "return" object (QGA wraps responses)
    if let Some(ret) = parsed.get("return") {
        if let Some(val) = ret.get(key).and_then(|v| v.as_str()) {
            return Some(val.to_string());
        }
    }
    None
}

/// SECURITY: Parse network interface response using serde_json (CWE-94).
/// QGA returns: {"return": [{"name": "eth0", "ip-addresses": [{"ip-address": "...", "ip-address-type": "ipv4", "prefix": 24}]}]}
fn parse_network_interfaces(json: &str) -> Vec<GuestIpAddress> {
    let mut addrs = Vec::new();

    let parsed: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return addrs,
    };

    let interfaces = match parsed.get("return").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return addrs,
    };

    for iface in interfaces {
        let iface_name = iface
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        let ip_addresses = match iface.get("ip-addresses").and_then(|v| v.as_array()) {
            Some(arr) => arr,
            None => continue,
        };

        for ip_entry in ip_addresses {
            let addr = match ip_entry.get("ip-address").and_then(|v| v.as_str()) {
                Some(a) => a,
                None => continue,
            };

            // Skip loopback
            if addr == "127.0.0.1" || addr == "::1" {
                continue;
            }

            let ip_type = ip_entry
                .get("ip-address-type")
                .and_then(|v| v.as_str())
                .unwrap_or("ipv4");

            // SECURITY: CWE-681 — QGA prefix is external data (u64). Clamp to valid
            // network prefix range (0-128) before narrowing to u32.
            let prefix = ip_entry
                .get("prefix")
                .and_then(|v| v.as_u64())
                .unwrap_or(24)
                .min(128) as u32;

            addrs.push(GuestIpAddress {
                interface: iface_name.to_string(),
                address: addr.to_string(),
                prefix,
                ip_type: ip_type.to_string(),
            });
        }
    }

    addrs
}

/// SECURITY: Parse filesystem info response using serde_json (CWE-94).
/// QGA returns: {"return": [{"mountpoint": "/", "type": "ext4", "disk": [...], "total-bytes": N, "used-bytes": N}]}
fn parse_filesystems(json: &str) -> Vec<GuestFilesystem> {
    let mut fses = Vec::new();

    let parsed: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return fses,
    };

    let filesystems = match parsed.get("return").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return fses,
    };

    for fs in filesystems {
        let mountpoint = match fs.get("mountpoint").and_then(|v| v.as_str()) {
            Some(m) => m,
            None => continue,
        };

        let fs_type = fs.get("type").and_then(|v| v.as_str()).unwrap_or("unknown");

        let total_bytes = fs.get("total-bytes").and_then(|v| v.as_u64()).unwrap_or(0);

        let used_bytes = fs.get("used-bytes").and_then(|v| v.as_u64()).unwrap_or(0);

        fses.push(GuestFilesystem {
            mountpoint: mountpoint.to_string(),
            fs_type: fs_type.to_string(),
            total_bytes,
            used_bytes,
        });
    }

    fses
}

// ============================================================================
// Wave 12.6: Guest listening-port probe (Lima-style auto-forwarding).
// ============================================================================

/// Information about a TCP socket listening inside the guest.
#[derive(Debug, Clone, PartialEq)]
pub struct GuestTcpListener {
    pub port: u16,
    pub bind_addr: String,            // "0.0.0.0", "127.0.0.1", "::", etc.
    pub process_name: Option<String>, // best-effort
}

/// Maximum length of any single line we'll consider when parsing `ss`/`netstat`
/// output. Anything longer is silently skipped (CWE-787 defense — a malicious
/// guest cannot force unbounded allocations through a single oversized line).
const MAX_LISTENER_LINE_LEN: usize = 1024;

/// Maximum number of base64 characters we'll accept from a single guest-exec
/// `out-data` field. 64 KiB is far more than `ss -tlnp` can plausibly emit on
/// any real guest, and far less than what a malicious guest could DoS with.
const MAX_LISTENER_OUTPUT_B64: usize = 64 * 1024;

/// Query the guest agent for listening TCP ports.
///
/// Runs `ss -tlnp` (or `netstat -tln` as a fallback) inside the guest via
/// `guest-exec`, decodes the captured base64 stdout, and parses the listener
/// table. Returns an empty Vec if the guest agent isn't reachable, if neither
/// tool is installed, or if the output is empty/unparseable.
///
/// SECURITY:
/// - VM name validated via `validate_vm_name_for_agent` (CWE-78, CWE-88).
/// - Each line capped at MAX_LISTENER_LINE_LEN (CWE-787, CWE-400).
/// - Total base64 output capped at MAX_LISTENER_OUTPUT_B64 (CWE-400).
/// - Ports validated to be in 1..=65535 (CWE-20).
pub fn list_guest_listeners(vm_name: &str) -> VmmResult<Vec<GuestTcpListener>> {
    validate_vm_name_for_agent(vm_name)?;

    if !ping_agent(vm_name) {
        // Agent not available — return empty rather than error (caller may
        // simply be polling for opportunistic auto-forward sync).
        return Ok(Vec::new());
    }

    // Try `ss -tlnp` first (modern Linux).
    if let Ok(output) = run_guest_exec(vm_name, "ss", &["-tlnp"]) {
        if !output.is_empty() {
            return Ok(parse_listener_output(&output, ListenerFormat::Ss));
        }
    }

    // Fall back to `netstat -tln` (older systems or BSD).
    if let Ok(output) = run_guest_exec(vm_name, "netstat", &["-tln"]) {
        if !output.is_empty() {
            return Ok(parse_listener_output(&output, ListenerFormat::Netstat));
        }
    }

    Ok(Vec::new())
}

/// Which command produced the output we're parsing — affects column layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ListenerFormat {
    /// `ss -tlnp` output. Columns: State Recv-Q Send-Q Local-Address:Port Peer-Address:Port [Process].
    Ss,
    /// `netstat -tln` output. Columns: Proto Recv-Q Send-Q Local-Address Foreign-Address State.
    Netstat,
}

/// Run a command inside the guest via QGA `guest-exec` and return the captured
/// stdout (decoded from base64). Returns an empty string if the command didn't
/// produce output or the agent isn't available.
fn run_guest_exec(vm_name: &str, path: &str, args: &[&str]) -> VmmResult<String> {
    // Build the JSON manually. Both `path` and `args` are constants chosen by
    // this module — they never come from user input — but we still use
    // serde_json to guarantee well-formed escaping.
    let payload = serde_json::json!({
        "execute": "guest-exec",
        "arguments": {
            "path": path,
            "arg": args,
            "capture-output": true,
        }
    });
    let cmd = payload.to_string();

    let resp = agent_command(vm_name, &cmd)?;

    // Pull the pid out of {"return":{"pid":N}} using serde_json (CWE-94).
    let parsed: serde_json::Value = serde_json::from_str(&resp)
        .map_err(|e| VmmError::Other(format!("Invalid JSON from guest-exec: {}", e)))?;
    let pid = parsed
        .get("return")
        .and_then(|r| r.get("pid"))
        .and_then(|v| v.as_i64())
        .ok_or_else(|| VmmError::Other("guest-exec response missing pid".into()))?;

    // Poll for completion. Use a short bounded loop — we don't want to block
    // indefinitely if the guest is wedged.
    let mut delay_ms = 50u64;
    for _ in 0..6 {
        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        let status_cmd = format!(
            r#"{{"execute":"guest-exec-status","arguments":{{"pid":{}}}}}"#,
            pid
        );
        let status_resp = match agent_command(vm_name, &status_cmd) {
            Ok(s) => s,
            Err(_) => return Ok(String::new()),
        };
        let status: serde_json::Value = match serde_json::from_str(&status_resp) {
            Ok(v) => v,
            Err(_) => return Ok(String::new()),
        };
        let ret = match status.get("return") {
            Some(r) => r,
            None => return Ok(String::new()),
        };
        let exited = ret.get("exited").and_then(|v| v.as_bool()).unwrap_or(false);
        if exited {
            // out-data is optional and base64-encoded.
            if let Some(b64) = ret.get("out-data").and_then(|v| v.as_str()) {
                if b64.len() > MAX_LISTENER_OUTPUT_B64 {
                    // CWE-400: refuse pathologically large output rather than
                    // allocating a huge decode buffer.
                    return Ok(String::new());
                }
                let bytes = base64_decode_simple(b64);
                return Ok(String::from_utf8_lossy(&bytes).into_owned());
            }
            return Ok(String::new());
        }
        delay_ms = (delay_ms * 2).min(400);
    }

    // Timed out waiting — treat as empty rather than erroring.
    Ok(String::new())
}

/// Minimal base64 decoder used only for QGA `out-data`. Skips characters not in
/// the standard alphabet so stray whitespace / line wraps don't break parsing.
fn base64_decode_simple(input: &str) -> Vec<u8> {
    static TABLE: [u8; 128] = {
        let mut t = [0xFFu8; 128];
        let chars = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut i = 0;
        while i < 64 {
            t[chars[i] as usize] = i as u8;
            i += 1;
        }
        t
    };

    let cleaned: Vec<u8> = input
        .bytes()
        .filter(|b| {
            let v = *b as usize;
            v == b'=' as usize || (v < 128 && TABLE[v] != 0xFF)
        })
        .collect();

    let mut out = Vec::with_capacity(cleaned.len() * 3 / 4);
    let mut i = 0;
    while i + 3 < cleaned.len() {
        let lookup = |b: u8| -> u32 {
            if b == b'=' {
                0
            } else {
                TABLE[(b & 0x7F) as usize] as u32
            }
        };
        let b0 = lookup(cleaned[i]);
        let b1 = lookup(cleaned[i + 1]);
        let b2 = lookup(cleaned[i + 2]);
        let b3 = lookup(cleaned[i + 3]);
        let triple = (b0 << 18) | (b1 << 12) | (b2 << 6) | b3;
        out.push((triple >> 16) as u8);
        if cleaned[i + 2] != b'=' {
            out.push((triple >> 8) as u8);
        }
        if cleaned[i + 3] != b'=' {
            out.push(triple as u8);
        }
        i += 4;
    }
    out
}

/// Parse the captured output of `ss -tlnp` or `netstat -tln`.
///
/// Pure function — testable without a guest. Skips header lines, malformed
/// rows, and oversized lines. Returns one entry per LISTEN row.
pub(crate) fn parse_listener_output(output: &str, fmt: ListenerFormat) -> Vec<GuestTcpListener> {
    let mut listeners = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for line in output.lines() {
        // CWE-787: skip any line longer than our cap before doing any work.
        if line.len() > MAX_LISTENER_LINE_LEN {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let parsed = match fmt {
            ListenerFormat::Ss => parse_ss_line(trimmed),
            ListenerFormat::Netstat => parse_netstat_line(trimmed),
        };

        if let Some(listener) = parsed {
            // Deduplicate by (port, bind_addr) — `ss -tlnp` can list the same
            // socket twice with different process info if you're unlucky.
            let key = (listener.port, listener.bind_addr.clone());
            if seen.insert(key) {
                listeners.push(listener);
            }
        }

        // CWE-400: hard cap on the number of listeners we'll surface.
        if listeners.len() >= 1024 {
            break;
        }
    }

    listeners
}

/// Parse a single `ss -tlnp` line. Returns None for headers and malformed input.
///
/// Sample lines:
/// ```text
/// LISTEN 0      128         0.0.0.0:22         0.0.0.0:*    users:(("sshd",pid=1234,fd=3))
/// LISTEN 0      4096        127.0.0.1:631      0.0.0.0:*
/// LISTEN 0      128         [::]:22            [::]:*
/// ```
fn parse_ss_line(line: &str) -> Option<GuestTcpListener> {
    // The header line starts with "State" — skip it (and anything that doesn't
    // start with "LISTEN").
    if !line.starts_with("LISTEN") {
        return None;
    }

    // Split into whitespace columns. The 4th column (index 3) is Local-Address:Port.
    let cols: Vec<&str> = line.split_whitespace().collect();
    if cols.len() < 4 {
        return None;
    }

    let local = cols[3];
    let (bind_addr, port) = split_addr_port(local)?;

    // Process info, if present, lives after "users:" — strip down to first name.
    let process_name = line
        .find("users:((")
        .and_then(|idx| line[idx + 8..].split('"').nth(1).map(|s| s.to_string()))
        .filter(|s| !s.is_empty());

    Some(GuestTcpListener {
        port,
        bind_addr,
        process_name,
    })
}

/// Parse a single `netstat -tln` line. Returns None for headers and malformed input.
///
/// Sample lines:
/// ```text
/// tcp        0      0 0.0.0.0:22              0.0.0.0:*               LISTEN
/// tcp6       0      0 :::22                   :::*                    LISTEN
/// ```
fn parse_netstat_line(line: &str) -> Option<GuestTcpListener> {
    // Header lines start with "Active" or "Proto" — easier to filter by
    // requiring the LISTEN state marker.
    if !line.contains("LISTEN") {
        return None;
    }

    let cols: Vec<&str> = line.split_whitespace().collect();
    if cols.len() < 4 {
        return None;
    }

    // Only TCP. Skip raw/udp lines that somehow contain the substring "LISTEN".
    if !cols[0].starts_with("tcp") {
        return None;
    }

    let local = cols[3];
    let (bind_addr, port) = split_addr_port(local)?;

    Some(GuestTcpListener {
        port,
        bind_addr,
        process_name: None,
    })
}

/// Split a `Local Address:Port` field into (addr, port). Handles IPv6 forms
/// like `[::]:22`, `[::ffff:0.0.0.0]:80`, and bare `:::22` (netstat-style).
fn split_addr_port(s: &str) -> Option<(String, u16)> {
    // Find the *last* colon — port is always after it, and IPv6 addresses have
    // internal colons we mustn't split on.
    let last = s.rfind(':')?;
    let addr_raw = &s[..last];
    let port_str = &s[last + 1..];

    // Reject wildcard ports like `*` — those aren't real listening sockets.
    let port: u32 = port_str.parse().ok()?;
    if !(1..=65535).contains(&port) {
        return None;
    }
    let port = port as u16;

    // Strip surrounding brackets from IPv6 forms: `[::]` -> `::`.
    let addr = addr_raw
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(addr_raw);

    if addr.is_empty() {
        return None;
    }

    Some((addr.to_string(), port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freeze_command_json_is_exact_qga_payload() {
        // The JSON must match qemu-ga's expected wire format exactly; a typo
        // (e.g. extra whitespace, wrong key) is silently rejected by the agent.
        assert_eq!(
            freeze_command_json(),
            r#"{"execute":"guest-fsfreeze-freeze"}"#
        );
    }

    #[test]
    fn thaw_command_json_is_exact_qga_payload() {
        assert_eq!(thaw_command_json(), r#"{"execute":"guest-fsfreeze-thaw"}"#);
    }

    #[test]
    fn parse_fsfreeze_count_normal_response() {
        // Typical qemu-ga reply for a Linux guest with 3 mounted filesystems.
        assert_eq!(parse_fsfreeze_count(r#"{"return":3}"#), Some(3));
        assert_eq!(parse_fsfreeze_count(r#"{"return":0}"#), Some(0));
    }

    #[test]
    fn parse_fsfreeze_count_handles_garbage() {
        assert_eq!(parse_fsfreeze_count("not json at all"), None);
        assert_eq!(parse_fsfreeze_count("{}"), None);
        assert_eq!(parse_fsfreeze_count(r#"{"return":"oops"}"#), None);
    }

    #[test]
    fn parse_fsfreeze_count_clamps_huge_values() {
        // SECURITY (CWE-681): if a malicious guest reports an absurdly large count,
        // we must not panic on narrowing — clamp to u32::MAX.
        let big = format!(r#"{{"return":{}}}"#, u64::MAX);
        assert_eq!(parse_fsfreeze_count(&big), Some(u32::MAX));
    }

    #[test]
    fn validate_vm_name_for_agent_rejects_injection() {
        // Empty
        assert!(validate_vm_name_for_agent("").is_err());
        // Shell metacharacters
        assert!(validate_vm_name_for_agent("vm;rm -rf /").is_err());
        assert!(validate_vm_name_for_agent("vm$(whoami)").is_err());
        assert!(validate_vm_name_for_agent("vm`id`").is_err());
        // Argument-flag prefix
        assert!(validate_vm_name_for_agent("-vm").is_err());
        // Null byte
        assert!(validate_vm_name_for_agent("vm\0name").is_err());
        // Valid names
        assert!(validate_vm_name_for_agent("my-vm").is_ok());
        assert!(validate_vm_name_for_agent("Win11_Test.01").is_ok());
    }

    // ───── Wave 12.6 — guest listener parser tests ─────

    #[test]
    fn parse_ss_ipv4_listener_with_process() {
        let line = r#"LISTEN 0      128         0.0.0.0:22         0.0.0.0:*    users:(("sshd",pid=1234,fd=3))"#;
        let got = parse_ss_line(line).expect("should parse");
        assert_eq!(got.port, 22);
        assert_eq!(got.bind_addr, "0.0.0.0");
        assert_eq!(got.process_name.as_deref(), Some("sshd"));
    }

    #[test]
    fn parse_ss_ipv4_listener_without_process() {
        let line = r#"LISTEN 0      4096        127.0.0.1:631      0.0.0.0:*"#;
        let got = parse_ss_line(line).expect("should parse");
        assert_eq!(got.port, 631);
        assert_eq!(got.bind_addr, "127.0.0.1");
        assert!(got.process_name.is_none());
    }

    #[test]
    fn parse_ss_ipv6_listener() {
        let line = r#"LISTEN 0      128         [::]:22            [::]:*"#;
        let got = parse_ss_line(line).expect("should parse");
        assert_eq!(got.port, 22);
        assert_eq!(got.bind_addr, "::");
    }

    #[test]
    fn parse_ss_skips_header_and_garbage() {
        assert!(parse_ss_line("State Recv-Q Send-Q Local Address:Port").is_none());
        assert!(parse_ss_line("").is_none());
        assert!(parse_ss_line("LISTEN").is_none()); // too few columns
        assert!(parse_ss_line("LISTEN 0 128 not-an-address").is_none());
    }

    #[test]
    fn parse_netstat_ipv4_listener() {
        let line = "tcp        0      0 0.0.0.0:22              0.0.0.0:*               LISTEN";
        let got = parse_netstat_line(line).expect("should parse");
        assert_eq!(got.port, 22);
        assert_eq!(got.bind_addr, "0.0.0.0");
    }

    #[test]
    fn parse_netstat_ipv6_listener() {
        let line = "tcp6       0      0 :::22                   :::*                    LISTEN";
        let got = parse_netstat_line(line).expect("should parse");
        assert_eq!(got.port, 22);
        // `:::22` rsplits on last colon -> addr "::" port 22.
        assert_eq!(got.bind_addr, "::");
    }

    #[test]
    fn parse_listener_output_mixed_v4_v6() {
        let out = "\
State  Recv-Q Send-Q Local Address:Port  Peer Address:Port
LISTEN 0      128    0.0.0.0:22          0.0.0.0:*    users:((\"sshd\",pid=1,fd=3))
LISTEN 0      4096   127.0.0.1:631       0.0.0.0:*
LISTEN 0      128    [::]:22             [::]:*
";
        let got = parse_listener_output(out, ListenerFormat::Ss);
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].port, 22);
        assert_eq!(got[0].bind_addr, "0.0.0.0");
        assert_eq!(got[0].process_name.as_deref(), Some("sshd"));
        assert_eq!(got[1].port, 631);
        assert_eq!(got[1].bind_addr, "127.0.0.1");
        assert_eq!(got[2].port, 22);
        assert_eq!(got[2].bind_addr, "::");
    }

    #[test]
    fn parse_listener_output_empty_input() {
        assert!(parse_listener_output("", ListenerFormat::Ss).is_empty());
        assert!(parse_listener_output("", ListenerFormat::Netstat).is_empty());
    }

    #[test]
    fn parse_listener_output_malformed_lines_dont_panic() {
        // Random garbage, no LISTEN, no columns, exotic characters — must not panic.
        let out = "\
junk junk junk
LISTEN
LISTEN  \t \t
LISTEN 0 128 foo:bar:baz
LISTEN 0 128 0.0.0.0:99999
LISTEN 0 128 0.0.0.0:0
";
        let got = parse_listener_output(out, ListenerFormat::Ss);
        // All lines should be rejected: bad port (99999, 0) and bad address ("foo:bar:baz" → port "baz" not numeric).
        assert!(got.is_empty(), "expected empty, got {:?}", got);
    }

    #[test]
    fn parse_listener_output_skips_oversized_lines() {
        // CWE-787: a line longer than the cap must be skipped, not parsed.
        let huge_addr = "1".repeat(MAX_LISTENER_LINE_LEN + 50);
        let bad_line = format!("LISTEN 0 128 {}:22 0.0.0.0:*", huge_addr);
        let mut out = String::new();
        out.push_str(&bad_line);
        out.push('\n');
        out.push_str("LISTEN 0 128 0.0.0.0:22 0.0.0.0:*\n");
        let got = parse_listener_output(&out, ListenerFormat::Ss);
        // Only the second (short, valid) line should survive.
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].port, 22);
        assert_eq!(got[0].bind_addr, "0.0.0.0");
    }

    #[test]
    fn parse_listener_output_dedupes_repeats() {
        let out = "\
LISTEN 0 128 0.0.0.0:22 0.0.0.0:*  users:((\"sshd\",pid=1,fd=3))
LISTEN 0 128 0.0.0.0:22 0.0.0.0:*  users:((\"sshd\",pid=1,fd=4))
";
        let got = parse_listener_output(out, ListenerFormat::Ss);
        assert_eq!(got.len(), 1);
    }

    #[test]
    fn split_addr_port_rejects_invalid_ports() {
        assert!(split_addr_port("0.0.0.0:0").is_none());
        assert!(split_addr_port("0.0.0.0:65536").is_none());
        assert!(split_addr_port("0.0.0.0:*").is_none());
        assert!(split_addr_port(":22").is_none());
    }

    #[test]
    fn split_addr_port_handles_bracketed_ipv6() {
        let (a, p) = split_addr_port("[::1]:8080").unwrap();
        assert_eq!(a, "::1");
        assert_eq!(p, 8080);
    }
}
