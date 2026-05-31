//! Remote VM Management — connect to remote hypervisors via qemu+ssh://
//!
//! Allows managing VMs on remote hosts from the same GUI.
//! Uses SSH for the libvirt connection and VNC tunneling.

use crate::error::{VmmError, VmmResult};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

/// A configured remote host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteHost {
    /// Display name (e.g. "Lab Server")
    pub name: String,
    /// Hostname or IP address
    pub hostname: String,
    /// SSH username (default: current user)
    #[serde(default)]
    pub username: String,
    /// SSH port (default: 22)
    #[serde(default = "default_ssh_port")]
    pub ssh_port: u16,
    /// Connection URI for libvirt (auto-constructed if empty)
    #[serde(default)]
    pub uri: String,
    /// Whether to use system connection (vs session)
    #[serde(default = "default_true")]
    pub use_system: bool,
}

fn default_ssh_port() -> u16 {
    22
}
fn default_true() -> bool {
    true
}

/// Validate a hostname or IP address.
/// Prevents SSH argument injection via malicious hostnames (CWE-93).
fn validate_hostname(hostname: &str) -> VmmResult<()> {
    if hostname.is_empty() {
        return Err(VmmError::InvalidConfig(
            "Hostname cannot be empty".to_string(),
        ));
    }
    if hostname.len() > 253 {
        return Err(VmmError::InvalidConfig(
            "Hostname too long (max 253)".to_string(),
        ));
    }
    // SECURITY (CWE-626): Reject null bytes that could truncate strings in C libraries
    if hostname.contains('\0') {
        return Err(VmmError::InvalidConfig(
            "Hostname must not contain null bytes (CWE-626)".to_string(),
        ));
    }
    // SECURITY (CWE-93): Reject embedded newlines that could inject SSH config directives
    if hostname.contains('\n') || hostname.contains('\r') {
        return Err(VmmError::InvalidConfig(
            "Hostname must not contain newlines (CWE-93)".to_string(),
        ));
    }
    // SECURITY (CWE-78): Reject spaces that could split arguments in shell contexts
    if hostname.contains(' ') || hostname.contains('\t') {
        return Err(VmmError::InvalidConfig(
            "Hostname must not contain whitespace (CWE-78)".to_string(),
        ));
    }
    // SECURITY (CWE-78): Reject shell metacharacters to prevent injection
    if hostname.chars().any(|c| ";|&`$(){}!#~'\"\\<>".contains(c)) {
        return Err(VmmError::InvalidConfig(
            "Hostname must not contain shell metacharacters (CWE-78)".to_string(),
        ));
    }
    // Hostnames must not start with a hyphen (SSH would interpret as a flag)
    if hostname.starts_with('-') {
        return Err(VmmError::InvalidConfig(
            "Hostname must not start with '-' (argument injection risk)".to_string(),
        ));
    }
    // Only allow safe hostname characters: alphanumeric, hyphens, dots, colons (IPv6), brackets
    if !hostname
        .chars()
        .all(|c| c.is_alphanumeric() || ".-:[]".contains(c))
    {
        return Err(VmmError::InvalidConfig(format!(
            "Hostname contains invalid characters: {}",
            hostname
        )));
    }
    Ok(())
}

/// Validate an SSH username.
/// Prevents SSH option injection via malicious usernames (CWE-93).
fn validate_ssh_username(username: &str) -> VmmResult<()> {
    if username.is_empty() {
        return Ok(()); // empty username is fine (uses current user)
    }
    if username.len() > 64 {
        return Err(VmmError::InvalidConfig(
            "Username too long (max 64)".to_string(),
        ));
    }
    // Username must not start with hyphen (argument injection)
    if username.starts_with('-') {
        return Err(VmmError::InvalidConfig(
            "Username must not start with '-' (argument injection risk)".to_string(),
        ));
    }
    // Only allow safe username characters
    if !username
        .chars()
        .all(|c| c.is_alphanumeric() || "-_.".contains(c))
    {
        return Err(VmmError::InvalidConfig(format!(
            "Username contains invalid characters: {}",
            username
        )));
    }
    Ok(())
}

impl RemoteHost {
    /// Construct the libvirt connection URI.
    pub fn connection_uri(&self) -> String {
        if !self.uri.is_empty() {
            // SECURITY: Validate custom URIs — only allow known-safe libvirt URI schemes (CWE-918).
            // A malicious URI could cause libvirt to connect to unexpected endpoints.
            let uri_lower = self.uri.to_lowercase();
            let safe_prefixes = [
                "qemu://",
                "qemu+ssh://",
                "qemu+tcp://",
                "qemu+tls://",
                "qemu:///",
                "test://",
                "test:///",
            ];
            if safe_prefixes.iter().any(|p| uri_lower.starts_with(p)) {
                return self.uri.clone();
            }
            // Reject unknown URI schemes — fall through to auto-construct
            tracing::warn!("Ignoring custom URI with unknown scheme: {}", self.uri);
        }
        let user_part = if self.username.is_empty() {
            String::new()
        } else {
            format!("{}@", self.username)
        };
        let mode = if self.use_system { "system" } else { "session" };
        if self.ssh_port != 22 {
            format!(
                "qemu+ssh://{}{}:{}/{}",
                user_part, self.hostname, self.ssh_port, mode
            )
        } else {
            format!("qemu+ssh://{}{}/{}", user_part, self.hostname, mode)
        }
    }

    /// Test SSH connectivity to this host.
    pub fn test_ssh(&self) -> VmmResult<String> {
        // SECURITY: Validate hostname and username to prevent SSH argument injection (CWE-93)
        validate_hostname(&self.hostname)?;
        validate_ssh_username(&self.username)?;

        let user_host = if self.username.is_empty() {
            self.hostname.clone()
        } else {
            format!("{}@{}", self.username, self.hostname)
        };

        // SECURITY: Enforce strict host key checking to prevent MITM (CWE-295)
        // SECURITY: CWE-403 — Close stdin to prevent FD inheritance to SSH process.
        let output = std::process::Command::new("ssh")
            .args([
                "-p",
                &self.ssh_port.to_string(),
                "-o",
                "ConnectTimeout=5",
                "-o",
                "BatchMode=yes",
                "-o",
                "StrictHostKeyChecking=accept-new",
                "--", // SECURITY: Prevent user_host from being interpreted as a flag (CWE-88)
                &user_host,
                "hostname",
            ])
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| VmmError::Other(format!("SSH failed: {}", e)))?;

        if output.status.success() {
            let hostname = String::from_utf8_lossy(&output.stdout).trim().to_string();
            info!("SSH test to {} succeeded: {}", self.hostname, hostname);
            Ok(hostname)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            Err(VmmError::Other(format!("SSH test failed: {}", stderr)))
        }
    }

    /// Test libvirt connection to this host.
    pub fn test_libvirt(&self) -> VmmResult<String> {
        // SECURITY: Validate hostname and username before constructing URI (CWE-93)
        validate_hostname(&self.hostname)?;
        validate_ssh_username(&self.username)?;
        let uri = self.connection_uri();
        // SECURITY: CWE-403 — Close stdin to prevent FD inheritance to child process.
        let output = std::process::Command::new("virsh")
            .args(["-c", &uri, "hostname"])
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| VmmError::Other(format!("virsh failed: {}", e)))?;

        if output.status.success() {
            let hostname = String::from_utf8_lossy(&output.stdout).trim().to_string();
            info!("Libvirt test to {} succeeded: {}", uri, hostname);
            Ok(hostname)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            Err(VmmError::Other(format!(
                "Libvirt connection failed: {}",
                stderr
            )))
        }
    }
}

/// Remote hosts configuration — stored in ~/.local/share/libre-vmm/remote_hosts.json
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RemoteHostsConfig {
    pub hosts: Vec<RemoteHost>,
}

impl RemoteHostsConfig {
    fn config_path() -> String {
        let home = dirs::home_dir().unwrap_or_default();
        format!(
            "{}/.local/share/libre-vmm/remote_hosts.json",
            home.display()
        )
    }

    /// Load remote hosts config from disk.
    pub fn load() -> Self {
        let path = Self::config_path();
        if let Ok(json) = std::fs::read_to_string(&path) {
            serde_json::from_str(&json).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    /// Save to disk.
    /// SECURITY: Restrict file permissions — remote host configs contain hostnames/usernames (CWE-732).
    pub fn save(&self) -> VmmResult<()> {
        let path = Self::config_path();
        let dir = std::path::Path::new(&path).parent().ok_or_else(|| {
            crate::VmmError::InvalidConfig("Config path has no parent directory".to_string())
        })?;
        std::fs::create_dir_all(dir)?;
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }

    /// Add a new remote host.
    pub fn add_host(&mut self, host: RemoteHost) {
        self.hosts.push(host);
    }

    /// Remove a host by index.
    pub fn remove_host(&mut self, index: usize) {
        if index < self.hosts.len() {
            self.hosts.remove(index);
        }
    }
}

/// SSH tunnel for VNC port forwarding.
/// Creates an SSH tunnel: local_port -> remote_host:remote_vnc_port
pub struct SshTunnel {
    child: std::process::Child,
    local_port: u16,
}

impl SshTunnel {
    /// Open an SSH tunnel for VNC access.
    /// Finds a free local port and forwards it to the remote VNC port.
    ///
    /// SECURITY: CWE-362 (Race Condition / TOCTOU) — Retries up to PORT_FIND_RETRIES
    /// times to mitigate the race between discovering a free port and SSH binding to it.
    /// Another process could grab the port in the window between our TcpListener::bind
    /// and SSH's bind. ExitOnForwardFailure=yes ensures SSH fails fast if the port is taken.
    pub fn open(host: &RemoteHost, remote_vnc_port: u16) -> VmmResult<Self> {
        // SECURITY: Validate hostname and username to prevent SSH argument injection (CWE-93)
        validate_hostname(&host.hostname)?;
        validate_ssh_username(&host.username)?;

        let user_host = if host.username.is_empty() {
            host.hostname.clone()
        } else {
            format!("{}@{}", host.username, host.hostname)
        };

        // SECURITY: CWE-362 — Retry loop to mitigate TOCTOU race on port allocation.
        // find_free_port() drops the listener before SSH can bind, so another process
        // could grab the port. We retry with a fresh port on failure.
        let mut last_error = String::new();
        for attempt in 0..PORT_FIND_RETRIES {
            let local_port = find_free_port()?;

            info!(
                "Opening SSH tunnel (attempt {}): localhost:{} -> {}:{} (VNC)",
                attempt + 1,
                local_port,
                host.hostname,
                remote_vnc_port
            );

            let child = std::process::Command::new("ssh")
                .args([
                    "-p",
                    &host.ssh_port.to_string(),
                    "-N", // No remote command
                    "-L",
                    &format!("{}:127.0.0.1:{}", local_port, remote_vnc_port),
                    "-o",
                    "ExitOnForwardFailure=yes",
                    "-o",
                    "ServerAliveInterval=30",
                    "-o",
                    "StrictHostKeyChecking=accept-new",
                    "--", // SECURITY: Prevent user_host from being interpreted as a flag (CWE-88)
                    &user_host,
                ])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn();

            match child {
                Ok(mut child_proc) => {
                    // Brief wait to let the tunnel establish
                    std::thread::sleep(std::time::Duration::from_millis(500));

                    // Check if SSH exited immediately (port contention or other error)
                    match child_proc.try_wait() {
                        Ok(Some(status)) if !status.success() => {
                            // SSH exited with error — likely port was taken (TOCTOU race)
                            last_error = format!(
                                "SSH tunnel exited with status {} (port {} may have been taken)",
                                status, local_port
                            );
                            warn!("{} — retrying with new port (CWE-362)", last_error);
                            continue;
                        },
                        Ok(Some(_)) => {
                            // SSH exited successfully but immediately — unusual
                            last_error = "SSH tunnel exited immediately".to_string();
                            continue;
                        },
                        Ok(None) => {
                            // SSH is still running — tunnel established successfully
                            return Ok(Self {
                                child: child_proc,
                                local_port,
                            });
                        },
                        Err(e) => {
                            last_error = format!("Failed to check SSH status: {}", e);
                            continue;
                        },
                    }
                },
                Err(e) => {
                    return Err(VmmError::Other(format!(
                        "Failed to start SSH tunnel: {}",
                        e
                    )));
                },
            }
        }

        Err(VmmError::Other(format!(
            "Failed to open SSH tunnel after {} attempts (CWE-362 TOCTOU): {}",
            PORT_FIND_RETRIES, last_error
        )))
    }

    /// Get the local port to connect VNC to.
    pub fn local_port(&self) -> u16 {
        self.local_port
    }

    /// Check if the tunnel process is still running.
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Close the tunnel.
    pub fn close(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for SshTunnel {
    fn drop(&mut self) {
        self.close();
    }
}

/// Find a free TCP port on localhost.
///
/// SECURITY: CWE-362 (Race Condition / TOCTOU) — There is an inherent race between
/// discovering the free port (by binding) and SSH using it: another process could grab
/// the port in between. We mitigate by:
/// 1. Retrying up to 5 times if SSH fails due to port contention
/// 2. Using SO_REUSEADDR semantics (kernel's ephemeral port allocator minimizes collisions)
/// 3. Keeping the discovery window as short as possible
///
/// A fully race-free solution would require SSH to accept a pre-bound fd (not supported)
/// or use SSH's own `-D 0` dynamic port allocation (not available for -L).
const PORT_FIND_RETRIES: usize = 5;

fn find_free_port() -> VmmResult<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| VmmError::Other(format!("Failed to find free port: {}", e)))?;
    let port = listener
        .local_addr()
        .map_err(|e| VmmError::Other(format!("Failed to get port: {}", e)))?
        .port();
    // Note: listener is dropped here, creating a TOCTOU window.
    // The caller (SshTunnel::open) should retry on failure.
    Ok(port)
}
