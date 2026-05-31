//! Guest tools detection and installation — qemu-guest-agent + spice-vdagent.
//!
//! This module detects the guest OS family via QGA `guest-get-osinfo`, checks
//! whether guest tools are installed/running, and can install them automatically
//! on Linux guests via QGA `guest-exec`. For Windows guests it mounts the
//! virtio-win ISO so the user can run the installer manually.

use crate::disk::validate_disk_path;
use crate::error::{VmmError, VmmResult};
use std::process::Command;
use tracing::{info, warn};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Detected guest OS type for tools installation.
#[derive(Debug, Clone, PartialEq)]
pub enum GuestOsFamily {
    Linux { distro: LinuxDistro },
    Windows,
    Unknown,
}

/// Linux distribution family — determines which package manager to use.
#[derive(Debug, Clone, PartialEq)]
pub enum LinuxDistro {
    /// Debian, Ubuntu, Mint, Pop!_OS
    Debian,
    /// Fedora, RHEL, CentOS, Rocky, Alma
    RedHat,
    /// Arch, Manjaro, EndeavourOS
    Arch,
    /// openSUSE, SLES
    Suse,
    /// Any other Linux distribution.
    Other(String),
}

/// Status of guest tools installation.
#[derive(Debug, Clone)]
pub struct GuestToolsStatus {
    pub agent_installed: bool,
    pub agent_running: bool,
    pub spice_agent_installed: bool,
    pub os_family: GuestOsFamily,
}

/// Step in the installation process (useful for UI progress reporting).
#[derive(Debug, Clone, PartialEq)]
pub enum InstallStep {
    Detecting,
    Detected(GuestToolsStatus),
    Installing,
    InstallComplete,
    Failed(String),
}

// PartialEq for GuestToolsStatus so InstallStep can derive it.
impl PartialEq for GuestToolsStatus {
    fn eq(&self, other: &Self) -> bool {
        self.agent_installed == other.agent_installed
            && self.agent_running == other.agent_running
            && self.spice_agent_installed == other.spice_agent_installed
            && self.os_family == other.os_family
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// SECURITY (CWE-20, CWE-88): Validate VM name before passing to virsh commands.
/// The previous validation only checked for empty/leading-dash, allowing names with
/// shell metacharacters (`;`, `|`, `$`, etc.) or newlines that could cause unexpected
/// behavior when passed as virsh arguments.
fn validate_vm_name(vm_name: &str) -> VmmResult<()> {
    if vm_name.is_empty() {
        return Err(VmmError::Other("VM name cannot be empty".to_string()));
    }
    if vm_name.len() > 255 {
        return Err(VmmError::Other(
            "VM name too long (max 255 chars)".to_string(),
        ));
    }
    if vm_name.starts_with('-') {
        return Err(VmmError::Other(
            "VM name must not start with '-' (argument injection risk, CWE-88)".to_string(),
        ));
    }
    // SECURITY (CWE-78): Strict allowlist — only allow characters safe for virsh arguments.
    if !vm_name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == ' ')
    {
        return Err(VmmError::Other(format!(
            "Invalid VM name '{}': only alphanumeric, space, hyphen, underscore, and period allowed (CWE-20)",
            vm_name
        )));
    }
    Ok(())
}

/// SECURITY: Escape a string for safe embedding in a JSON string literal (CWE-94).
///
/// Escapes double quotes, backslashes, and control characters (U+0000..U+001F)
/// so that `path` or `arg` values cannot break out of their JSON string context.
fn escape_json_string(s: &str) -> String {
    let mut escaped = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            c if c.is_control() => {
                // JSON requires \u00XX for other control characters.
                escaped.push_str(&format!("\\u{:04x}", c as u32));
            },
            c => escaped.push(c),
        }
    }
    escaped
}

/// Send a QGA command to the guest via `virsh qemu-agent-command`.
/// This is a local helper that mirrors the private `agent_command` in
/// `guest_agent.rs` — we cannot call that one because it is not public.
fn qga_command(vm_name: &str, cmd: &str) -> VmmResult<String> {
    validate_vm_name(vm_name)?;

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

/// Ping the guest agent; returns `true` if it responds.
fn ping_agent(vm_name: &str) -> bool {
    qga_command(vm_name, "{\"execute\":\"guest-ping\"}").is_ok()
}

/// Map an OS `id` string from QGA `guest-get-osinfo` to a [`GuestOsFamily`].
fn map_os_id(id: &str, name: &str) -> GuestOsFamily {
    let id_lower = id.to_lowercase();
    let name_lower = name.to_lowercase();

    // Windows detection
    if id_lower.starts_with("windows") || id_lower == "mswindows" || name_lower.contains("windows")
    {
        return GuestOsFamily::Windows;
    }

    // Linux distro families
    let distro = match id_lower.as_str() {
        "ubuntu" | "debian" | "linuxmint" | "pop" => LinuxDistro::Debian,
        "fedora" | "rhel" | "centos" | "rocky" | "alma" => LinuxDistro::RedHat,
        "arch" | "manjaro" | "endeavouros" => LinuxDistro::Arch,
        "opensuse" | "opensuse-leap" | "opensuse-tumbleweed" | "sles" => LinuxDistro::Suse,
        other => {
            // Heuristic fallback: check name string for known families
            if name_lower.contains("ubuntu")
                || name_lower.contains("debian")
                || name_lower.contains("mint")
            {
                LinuxDistro::Debian
            } else if name_lower.contains("fedora")
                || name_lower.contains("red hat")
                || name_lower.contains("centos")
                || name_lower.contains("rocky")
            {
                LinuxDistro::RedHat
            } else if name_lower.contains("arch") || name_lower.contains("manjaro") {
                LinuxDistro::Arch
            } else if name_lower.contains("suse") {
                LinuxDistro::Suse
            } else if other.is_empty() {
                return GuestOsFamily::Unknown;
            } else {
                LinuxDistro::Other(other.to_string())
            }
        },
    };

    GuestOsFamily::Linux { distro }
}

/// Execute a command inside the guest via QGA `guest-exec` and wait for it to
/// finish by polling `guest-exec-status`. Returns combined stdout+stderr.
fn guest_exec(vm_name: &str, path: &str, args: &[&str]) -> VmmResult<(i64, String)> {
    // Build the guest-exec JSON command.
    // SECURITY: Escape path and args to prevent JSON injection (CWE-94).
    let escaped_path = escape_json_string(path);
    let args_json: Vec<String> = args
        .iter()
        .map(|a| format!("\"{}\"", escape_json_string(a)))
        .collect();
    let cmd = format!(
        "{{\"execute\":\"guest-exec\",\"arguments\":{{\"path\":\"{}\",\"arg\":[{}],\"capture-output\":true}}}}",
        escaped_path,
        args_json.join(",")
    );

    let resp = qga_command(vm_name, &cmd)?;

    // Parse PID from response: {"return": {"pid": 12345}}
    let parsed: serde_json::Value = serde_json::from_str(&resp)
        .map_err(|e| VmmError::Other(format!("Failed to parse guest-exec response: {}", e)))?;

    let pid = parsed
        .get("return")
        .and_then(|r| r.get("pid"))
        .and_then(|p| p.as_i64())
        .ok_or_else(|| VmmError::Other("No PID in guest-exec response".to_string()))?;

    // Poll guest-exec-status until the process exits (max ~60 seconds).
    let status_cmd = format!(
        "{{\"execute\":\"guest-exec-status\",\"arguments\":{{\"pid\":{}}}}}",
        pid
    );

    let mut attempts = 0;
    loop {
        attempts += 1;
        if attempts > 120 {
            return Err(VmmError::Other(
                "Timed out waiting for guest command to finish".to_string(),
            ));
        }

        std::thread::sleep(std::time::Duration::from_millis(500));

        let status_resp = qga_command(vm_name, &status_cmd)?;
        let status: serde_json::Value = serde_json::from_str(&status_resp)
            .map_err(|e| VmmError::Other(format!("Failed to parse exec-status: {}", e)))?;

        let ret = status
            .get("return")
            .ok_or_else(|| VmmError::Other("No return in exec-status".to_string()))?;

        let exited = ret.get("exited").and_then(|v| v.as_bool()).unwrap_or(false);
        if !exited {
            continue;
        }

        let exitcode = ret.get("exitcode").and_then(|v| v.as_i64()).unwrap_or(-1);

        // Decode base64-encoded stdout/stderr.
        let stdout_b64 = ret.get("out-data").and_then(|v| v.as_str()).unwrap_or("");
        let stderr_b64 = ret.get("err-data").and_then(|v| v.as_str()).unwrap_or("");

        let stdout = base64_decode(stdout_b64);
        let stderr = base64_decode(stderr_b64);

        let mut output = stdout;
        if !stderr.is_empty() {
            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str(&stderr);
        }

        return Ok((exitcode, output));
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Detect guest OS family by querying QGA `guest-get-osinfo`.
/// Returns [`GuestOsFamily`] based on the `id` or `name` field.
pub fn detect_guest_os(vm_name: &str) -> VmmResult<GuestOsFamily> {
    validate_vm_name(vm_name)?;

    let resp = qga_command(vm_name, "{\"execute\":\"guest-get-osinfo\"}")?;

    let parsed: serde_json::Value = serde_json::from_str(&resp)
        .map_err(|e| VmmError::Other(format!("Failed to parse osinfo response: {}", e)))?;

    let ret = parsed.get("return").unwrap_or(&parsed);

    let id = ret.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let name = ret.get("name").and_then(|v| v.as_str()).unwrap_or("");

    let family = map_os_id(id, name);
    info!(vm = vm_name, ?family, "Detected guest OS family");

    Ok(family)
}

/// Check the current status of guest tools in the VM.
///
/// Pings QGA, checks if spice-vdagent is running:
/// - Linux: `pgrep spice-vdagent`
/// - Windows: checks for the `vdservice` service via `sc query`
pub fn check_tools_status(vm_name: &str) -> VmmResult<GuestToolsStatus> {
    validate_vm_name(vm_name)?;

    // If the agent is not responding, nothing is installed/running from our POV.
    if !ping_agent(vm_name) {
        return Ok(GuestToolsStatus {
            agent_installed: false,
            agent_running: false,
            spice_agent_installed: false,
            os_family: GuestOsFamily::Unknown,
        });
    }

    // Agent is responding — it is installed and running.
    let os_family = detect_guest_os(vm_name).unwrap_or(GuestOsFamily::Unknown);

    // Check for spice-vdagent.
    let spice_running = match &os_family {
        GuestOsFamily::Linux { .. } => {
            // `pgrep spice-vdagent` exits 0 if at least one process matches.
            match guest_exec(vm_name, "/usr/bin/pgrep", &["spice-vdagent"]) {
                Ok((code, _)) => code == 0,
                Err(_) => false,
            }
        },
        GuestOsFamily::Windows => {
            // Check if the SPICE Agent service is running.
            match guest_exec(
                vm_name,
                "C:\\Windows\\System32\\sc.exe",
                &["query", "vdservice"],
            ) {
                Ok((code, output)) => code == 0 && output.contains("RUNNING"),
                Err(_) => false,
            }
        },
        GuestOsFamily::Unknown => false,
    };

    let status = GuestToolsStatus {
        agent_installed: true,
        agent_running: true,
        spice_agent_installed: spice_running,
        os_family,
    };

    info!(vm = vm_name, ?status, "Guest tools status");

    Ok(status)
}

/// Install guest tools on a Linux guest via QGA `guest-exec`.
///
/// Runs the appropriate package manager command based on distro:
/// - Debian: `apt-get install -y qemu-guest-agent spice-vdagent`
/// - RedHat: `dnf install -y qemu-guest-agent spice-vdagent`
/// - Arch: `pacman -S --noconfirm qemu-guest-agent spice-vdagent`
/// - Suse: `zypper install -y qemu-guest-agent spice-vdagent`
///
/// Returns the command output for display.
pub fn install_linux_tools(vm_name: &str, distro: &LinuxDistro) -> VmmResult<String> {
    validate_vm_name(vm_name)?;

    let (path, args): (&str, Vec<&str>) = match distro {
        LinuxDistro::Debian => (
            "/usr/bin/apt-get",
            vec!["install", "-y", "qemu-guest-agent", "spice-vdagent"],
        ),
        LinuxDistro::RedHat => (
            "/usr/bin/dnf",
            vec!["install", "-y", "qemu-guest-agent", "spice-vdagent"],
        ),
        LinuxDistro::Arch => (
            "/usr/bin/pacman",
            vec!["-S", "--noconfirm", "qemu-guest-agent", "spice-vdagent"],
        ),
        LinuxDistro::Suse => (
            "/usr/bin/zypper",
            vec!["install", "-y", "qemu-guest-agent", "spice-vdagent"],
        ),
        LinuxDistro::Other(name) => {
            warn!(vm = vm_name, distro = %name, "Unknown distro — cannot auto-install");
            return Err(VmmError::Other(format!(
                "Automatic installation not supported for distro: {}",
                name
            )));
        },
    };

    info!(vm = vm_name, path, ?args, "Installing guest tools");

    let (exitcode, output) = guest_exec(vm_name, path, &args)?;

    if exitcode != 0 {
        warn!(vm = vm_name, exitcode, %output, "Guest tools installation failed");
        return Err(VmmError::Other(format!(
            "Installation command exited with code {}: {}",
            exitcode, output
        )));
    }

    info!(vm = vm_name, "Guest tools installed successfully");
    Ok(output)
}

/// Build the standard list of directories to search for a bundled guest-tool
/// artifact, in priority order.
///
/// The order is designed to match how Libre VMM is actually distributed:
///
/// 1. **Executable-relative system layout** (`<bindir>/../share/libre-vmm/guest-tools/...`).
///    Matches the `.deb`/`.rpm` layout: binary in `/usr/bin/`, data in
///    `/usr/share/libre-vmm/guest-tools/`. Also handles non-standard prefixes
///    like `/opt/libre-vmm/bin/` → `/opt/libre-vmm/share/libre-vmm/guest-tools/`.
/// 2. **Executable-relative portable layout** (`<bindir>/../guest-tools/...`).
///    Matches an unpacked tarball or development build where binaries and
///    guest-tools sit side-by-side.
/// 3. **Absolute system data path** (`/usr/share/libre-vmm/guest-tools/...`).
///    Fallback if `current_exe()` is unavailable or has been moved.
/// 4. **User data directory** (`~/.local/share/libre-vmm/guest-tools/`).
///    Per-user install via `scripts/install.sh` or future first-run downloader.
///
/// Returns an iterator of `PathBuf` candidates to check with `.exists()`.
fn guest_tools_search_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();

    if let Ok(exe) = std::env::current_exe() {
        if let Some(bin_dir) = exe.parent() {
            // System layout: <prefix>/bin → <prefix>/share/libre-vmm/guest-tools
            dirs.push(bin_dir.join("../share/libre-vmm/guest-tools"));
            // Portable layout: <bindir>/../guest-tools (tarball / dev build)
            dirs.push(bin_dir.join("../guest-tools"));
        }
    }

    // Absolute system path (matches the `.deb` install).
    dirs.push(std::path::PathBuf::from("/usr/share/libre-vmm/guest-tools"));

    // Per-user data directory.
    if let Some(d) = dirs::data_dir() {
        dirs.push(d.join("libre-vmm/guest-tools"));
    }

    dirs
}

/// Find the bundled guest tools ISO shipped with Libre VMM.
///
/// Prefers the unified `libre-vmm-guest-tools.iso` (VirtIO + SPICE + WinFsp + GPU)
/// when available, falling back to the plain Red Hat `virtio-win.iso` if not.
///
/// The Windows-specific files live under `<search_dir>/windows/`. As a final
/// fallback this also checks the Debian/Fedora system path
/// `/usr/share/virtio-win/virtio-win.iso` for systems where the user has
/// installed the upstream `virtio-win` package directly.
pub fn find_bundled_virtio_win_iso() -> Option<String> {
    // Unified ISO first, then plain virtio-win, across every search directory.
    let search_dirs = guest_tools_search_dirs();
    for filename in ["libre-vmm-guest-tools.iso", "virtio-win.iso"] {
        for dir in &search_dirs {
            let candidate = dir.join("windows").join(filename);
            if candidate.exists() {
                if let Some(s) = candidate.to_str() {
                    info!(path = s, "Found bundled virtio-win ISO");
                    return Some(s.to_string());
                }
            }
        }
    }
    // Final fallback: upstream Debian/Fedora virtio-win package path.
    let upstream = std::path::PathBuf::from("/usr/share/virtio-win/virtio-win.iso");
    if upstream.exists() {
        if let Some(s) = upstream.to_str() {
            info!(path = s, "Found bundled virtio-win ISO");
            return Some(s.to_string());
        }
    }
    None
}

/// Find the bundled SPICE guest tools installer (Windows `.exe`).
pub fn find_bundled_spice_tools() -> Option<String> {
    for dir in guest_tools_search_dirs() {
        let candidate = dir.join("windows/spice-guest-tools.exe");
        if candidate.exists() {
            if let Some(s) = candidate.to_str() {
                return Some(s.to_string());
            }
        }
    }
    None
}

/// Find the bundled WinFsp installer (Windows `.msi`).
pub fn find_bundled_winfsp() -> Option<String> {
    for dir in guest_tools_search_dirs() {
        let candidate = dir.join("windows/winfsp.msi");
        if candidate.exists() {
            if let Some(s) = candidate.to_str() {
                return Some(s.to_string());
            }
        }
    }
    None
}

/// For Windows guests, mount the virtio-win ISO via `virsh change-media`.
///
/// The user then runs the installer manually from the mounted drive.
/// `cdrom_target` should be the correct device name (e.g., "sdb" for Windows
/// VMs where the primary disk is on SATA "sda").
pub fn mount_virtio_win_iso(vm_name: &str, iso_path: &str, cdrom_target: &str) -> VmmResult<()> {
    validate_vm_name(vm_name)?;

    if iso_path.is_empty() {
        return Err(VmmError::Other("ISO path must not be empty".to_string()));
    }

    let target = if cdrom_target.is_empty() {
        "sdb"
    } else {
        cdrom_target
    };

    // SECURITY: Validate ISO path to prevent path traversal (CWE-22).
    validate_disk_path(iso_path)?;

    info!(
        vm = vm_name,
        iso = iso_path,
        target = target,
        "Mounting virtio-win ISO"
    );

    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance.
    let output = Command::new("virsh")
        .args(["change-media", "--", vm_name, target, iso_path, "--insert"])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| VmmError::Other(format!("Failed to run virsh change-media: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VmmError::Other(format!("Failed to mount ISO: {}", stderr)));
    }

    info!(vm = vm_name, "virtio-win ISO mounted successfully");
    Ok(())
}

/// Eject the virtio-win ISO after installation.
pub fn eject_virtio_win_iso(vm_name: &str, cdrom_target: &str) -> VmmResult<()> {
    validate_vm_name(vm_name)?;

    let target = if cdrom_target.is_empty() {
        "sdb"
    } else {
        cdrom_target
    };

    info!(vm = vm_name, target = target, "Ejecting virtio-win ISO");

    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance.
    let output = Command::new("virsh")
        .args(["change-media", "--", vm_name, target, "--eject"])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| VmmError::Other(format!("Failed to run virsh change-media: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VmmError::Other(format!("Failed to eject ISO: {}", stderr)));
    }

    info!(vm = vm_name, "virtio-win ISO ejected");
    Ok(())
}

// ---------------------------------------------------------------------------
// Base64 decoder (no external dependency)
// ---------------------------------------------------------------------------

/// Simple base64 decoder for QGA output.
fn base64_decode(input: &str) -> String {
    const DECODE: [u8; 128] = {
        let mut table = [255u8; 128];
        let chars = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut i = 0;
        while i < 64 {
            table[chars[i] as usize] = i as u8;
            i += 1;
        }
        table
    };

    let bytes: Vec<u8> = input.bytes().filter(|&b| b != b'=').collect();
    let mut output = Vec::with_capacity(bytes.len() * 3 / 4);

    let mut i = 0;
    while i + 3 < bytes.len() {
        let a = DECODE.get(bytes[i] as usize).copied().unwrap_or(0) as u32;
        let b = DECODE.get(bytes[i + 1] as usize).copied().unwrap_or(0) as u32;
        let c = DECODE.get(bytes[i + 2] as usize).copied().unwrap_or(0) as u32;
        let d = DECODE.get(bytes[i + 3] as usize).copied().unwrap_or(0) as u32;

        let n = (a << 18) | (b << 12) | (c << 6) | d;
        output.push((n >> 16) as u8);
        output.push(((n >> 8) & 0xFF) as u8);
        output.push((n & 0xFF) as u8);
        i += 4;
    }

    // Handle remaining bytes
    let remaining = bytes.len() - i;
    if remaining >= 2 {
        let a = DECODE.get(bytes[i] as usize).copied().unwrap_or(0) as u32;
        let b = DECODE.get(bytes[i + 1] as usize).copied().unwrap_or(0) as u32;
        let n = (a << 18) | (b << 12);
        output.push((n >> 16) as u8);
        if remaining >= 3 {
            let c = DECODE.get(bytes[i + 2] as usize).copied().unwrap_or(0) as u32;
            let n = (a << 18) | (b << 12) | (c << 6);
            if !output.is_empty() {
                output.pop();
            }
            output.push((n >> 16) as u8);
            output.push(((n >> 8) & 0xFF) as u8);
        }
    }

    String::from_utf8_lossy(&output).to_string()
}
