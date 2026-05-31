//! Host-Guest Integration — shared applications and cross-OS interaction.
//!
//! Parallels-inspired: open host files in guest apps, launch guest apps from host,
//! share clipboard, drag-and-drop file transfer.
//!
//! Architecture:
//! - Uses QEMU Guest Agent (QGA) for file transfer and command execution
//! - Uses virtiofs shared folder for seamless file sharing
//! - D-Bus service (future) for native desktop integration
//!
//! Phase 1 (current): File transfer + command execution via QGA
//! Phase 2 (future): D-Bus service, MIME type registration, desktop integration

use crate::error::{VmmError, VmmResult};
use std::path::Path;
use tracing::{info, warn};

/// SECURITY: CWE-552 — Blocklist of sensitive host paths that must never be
/// transferred to a guest VM. A compromised or malicious guest could request
/// files containing credentials, private keys, or system secrets.
const SENSITIVE_HOST_PATHS: &[&str] = &[
    "/etc/shadow",
    "/etc/gshadow",
    "/etc/master.passwd", // BSD
    "/etc/sudoers",
    "/etc/crypttab",
    "/etc/ssl/private",
    "/root/.ssh",
    "/root/.gnupg",
    "/root/.bash_history",
    "/root/.local/share/keyrings",
    "/proc/",
    "/sys/",
    "/dev/",
];

/// SECURITY: CWE-552 — Blocklist patterns matched against any path component.
const SENSITIVE_HOST_PATTERNS: &[&str] = &[
    ".ssh/",
    ".gnupg/",
    "id_rsa",
    "id_ed25519",
    "id_ecdsa",
    "id_dsa",
    ".pem",
    ".key",
    "authorized_keys",
    "known_hosts",
    ".bash_history",
    ".zsh_history",
    "private_key",
    ".env",
    "credentials",
    "secret",
    "vault",
    "keyring",
];

/// SECURITY: CWE-552 — Validate that a host file path does not point to
/// sensitive system files before allowing transfer to a guest VM.
fn validate_host_source_path(path: &Path) -> VmmResult<()> {
    let canonical = path.canonicalize().map_err(|e| {
        VmmError::Other(format!(
            "Cannot resolve host path '{}': {} (symlink attack or missing file?)",
            path.display(),
            e
        ))
    })?;
    let path_str = canonical.display().to_string();

    // Check against absolute sensitive paths
    for sensitive in SENSITIVE_HOST_PATHS {
        if path_str.starts_with(sensitive) {
            return Err(VmmError::Other(format!(
                "SECURITY: Refusing to transfer sensitive host file '{}' to guest \
                 (matches blocked path '{}') — CWE-552",
                path.display(),
                sensitive
            )));
        }
    }

    // Check against sensitive filename/directory patterns
    let path_lower = path_str.to_lowercase();
    for pattern in SENSITIVE_HOST_PATTERNS {
        if path_lower.contains(pattern) {
            return Err(VmmError::Other(format!(
                "SECURITY: Refusing to transfer host file '{}' to guest \
                 (matches sensitive pattern '{}') — CWE-552",
                path.display(),
                pattern
            )));
        }
    }

    // Also block paths matching ~/.ssh, ~/.gnupg for any user
    if let Some(home_dir) = dirs::home_dir() {
        let home_str = home_dir.display().to_string();
        let sensitive_home_dirs = [".ssh", ".gnupg", ".local/share/keyrings"];
        for dir in &sensitive_home_dirs {
            let blocked = format!("{}/{}", home_str, dir);
            if path_str.starts_with(&blocked) {
                return Err(VmmError::Other(format!(
                    "SECURITY: Refusing to transfer sensitive home file '{}' to guest \
                     (matches ~/{}) — CWE-552",
                    path.display(),
                    dir
                )));
            }
        }
    }

    Ok(())
}

/// Host-guest integration capabilities for a running VM.
#[derive(Debug, Clone, Default)]
pub struct HostGuestCapabilities {
    /// Whether the QEMU Guest Agent is available.
    pub agent_available: bool,
    /// Whether the guest supports file transfer via QGA.
    pub file_transfer: bool,
    /// Whether the guest supports command execution via QGA.
    pub command_exec: bool,
    /// Whether virtiofs shared folder is mounted.
    pub shared_folder_mounted: bool,
    /// Guest OS type detected.
    pub guest_os: GuestOsType,
}

/// Detected guest OS type.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum GuestOsType {
    #[default]
    Unknown,
    Linux,
    Windows,
}

/// Result of a file transfer operation.
#[derive(Debug, Clone)]
pub struct FileTransferResult {
    /// Path on the guest where the file was placed.
    pub guest_path: String,
    /// Bytes transferred.
    pub bytes_transferred: u64,
    /// Whether the transfer succeeded.
    pub success: bool,
    /// Error message if failed.
    pub error: Option<String>,
}

/// Result of a command execution in the guest.
#[derive(Debug, Clone)]
pub struct GuestExecResult {
    /// Exit code (0 = success).
    pub exit_code: i32,
    /// stdout output.
    pub stdout: String,
    /// stderr output.
    pub stderr: String,
    /// Whether execution succeeded.
    pub success: bool,
}

/// Detect host-guest integration capabilities for a VM.
pub fn detect_capabilities(vm_name: &str) -> HostGuestCapabilities {
    let mut caps = HostGuestCapabilities::default();

    // Check if guest agent is available via ping
    if ping_agent(vm_name) {
        caps.agent_available = true;

        // Check guest OS type
        caps.guest_os = detect_guest_os(vm_name);

        // QGA file transfer is available if agent responds
        caps.file_transfer = true;

        // Command execution support
        caps.command_exec = check_exec_support(vm_name);
    }

    // Check if virtiofs shared folder is accessible
    caps.shared_folder_mounted = check_shared_folder(vm_name);

    caps
}

/// Transfer a file from the host to the guest via QEMU Guest Agent.
/// Uses guest-file-open, guest-file-write, guest-file-close protocol.
pub fn transfer_file_to_guest(
    vm_name: &str,
    host_path: &Path,
    guest_path: &str,
) -> VmmResult<FileTransferResult> {
    // SECURITY: CWE-552 — Block transfer of sensitive host files to guest.
    // A malicious guest or compromised UI could trick the host into sending
    // /etc/shadow, SSH private keys, etc. into the VM where they are exposed.
    validate_host_source_path(host_path)?;

    // SECURITY: CWE-400 — Read the host file with size limit to prevent DoS/OOM
    const MAX_TRANSFER_SIZE: u64 = 4 * 1024 * 1024 * 1024; // 4 GiB
    let file_size = std::fs::metadata(host_path)
        .map_err(|e| {
            VmmError::Other(format!(
                "Failed to stat host file '{}': {}",
                host_path.display(),
                e
            ))
        })?
        .len();
    if file_size > MAX_TRANSFER_SIZE {
        return Err(VmmError::Other(format!(
            "File '{}' is too large for guest transfer ({} bytes, max {} bytes)",
            host_path.display(),
            file_size,
            MAX_TRANSFER_SIZE
        )));
    }
    let data = std::fs::read(host_path).map_err(|e| {
        VmmError::Other(format!(
            "Failed to read host file '{}': {}",
            host_path.display(),
            e
        ))
    })?;

    let file_size = data.len() as u64;

    // Open file on guest
    let open_cmd = format!(
        "{{\"execute\":\"guest-file-open\",\"arguments\":{{\"path\":\"{}\",\"mode\":\"wb\"}}}}",
        escape_json_str(guest_path)
    );

    let handle_resp = agent_command(vm_name, &open_cmd)?;
    let handle = extract_json_number(&handle_resp, "return")
        .ok_or_else(|| VmmError::Other("Failed to open file on guest".to_string()))?;

    // SECURITY: CWE-404 — Ensure the guest file handle is always closed,
    // even if a write chunk fails mid-transfer. Leaked handles can exhaust
    // the guest agent's file descriptor table.
    let write_result = (|| -> VmmResult<()> {
        // Write data in chunks (QGA has a size limit per write, typically 48MB)
        let chunk_size = 1024 * 1024; // 1MB chunks
        let mut offset = 0;
        while offset < data.len() {
            let end = (offset + chunk_size).min(data.len());
            let chunk = &data[offset..end];
            let chunk_b64 = base64_encode(chunk);

            let write_cmd = format!(
                "{{\"execute\":\"guest-file-write\",\"arguments\":{{\"handle\":{},\"buf-b64\":\"{}\"}}}}",
                handle, chunk_b64
            );

            agent_command(vm_name, &write_cmd)?;
            offset = end;
        }
        Ok(())
    })();

    // Always close the guest file handle, even on write error
    let close_cmd = format!(
        "{{\"execute\":\"guest-file-close\",\"arguments\":{{\"handle\":{}}}}}",
        handle
    );
    let _ = agent_command(vm_name, &close_cmd);

    // Propagate any write error after cleanup
    write_result?;

    info!(
        "File transferred to guest: {} -> {} ({} bytes)",
        host_path.display(),
        guest_path,
        file_size
    );

    Ok(FileTransferResult {
        guest_path: guest_path.to_string(),
        bytes_transferred: file_size,
        success: true,
        error: None,
    })
}

/// Execute a command inside the guest via QEMU Guest Agent.
/// Returns the exit code, stdout, and stderr.
pub fn exec_in_guest(vm_name: &str, command: &str, args: &[&str]) -> VmmResult<GuestExecResult> {
    // Build the arguments JSON array
    let args_json: Vec<String> = args
        .iter()
        .map(|a| format!("\"{}\"", escape_json_str(a)))
        .collect();
    let args_str = args_json.join(",");

    let exec_cmd = format!(
        "{{\"execute\":\"guest-exec\",\"arguments\":{{\"path\":\"{}\",\"arg\":[{}],\"capture-output\":true}}}}",
        escape_json_str(command),
        args_str
    );

    let resp = agent_command(vm_name, &exec_cmd)?;
    let pid = extract_json_number(&resp, "pid")
        .ok_or_else(|| VmmError::Other("Failed to execute command in guest".to_string()))?;

    // Poll for completion (up to 30 seconds)
    for _ in 0..60 {
        std::thread::sleep(std::time::Duration::from_millis(500));

        let status_cmd = format!(
            "{{\"execute\":\"guest-exec-status\",\"arguments\":{{\"pid\":{}}}}}",
            pid
        );

        let status_resp = agent_command(vm_name, &status_cmd)?;

        // Parse JSON once and extract all fields from the parsed value.
        let status_parsed = extract_json_value(&status_resp);
        if let Some(exited) = status_parsed
            .as_ref()
            .and_then(|v| extract_json_bool_from_value(v, "exited"))
        {
            if exited {
                let parsed = status_parsed.as_ref().unwrap(); // safe: exited was extracted from it
                                                              // SECURITY: CWE-681/CWE-190 — Exit code from QGA is u64; truncating to i32
                                                              // could flip sign on values > i32::MAX. Clamp to valid POSIX range.
                let raw_exit = extract_json_number_from_value(parsed, "exitcode").unwrap_or(0);
                let exit_code = i32::try_from(raw_exit.min(i32::MAX as u64)).unwrap_or(i32::MAX);

                let stdout = extract_json_str_from_value(parsed, "out-data")
                    .map(|b64| base64_decode(&b64))
                    .unwrap_or_default();

                let stderr = extract_json_str_from_value(parsed, "err-data")
                    .map(|b64| base64_decode(&b64))
                    .unwrap_or_default();

                return Ok(GuestExecResult {
                    exit_code,
                    stdout,
                    stderr,
                    success: exit_code == 0,
                });
            }
        }
    }

    // SECURITY: CWE-400 — Attempt to kill the hung guest process to prevent
    // resource exhaustion inside the VM. QGA does not have a native kill-by-PID
    // command, but we can try to terminate via guest-exec of kill.
    warn!(
        "Guest command timed out after 30s (pid {}), attempting kill",
        pid
    );
    let kill_cmd = format!(
        "{{\"execute\":\"guest-exec\",\"arguments\":{{\"path\":\"kill\",\"arg\":[\"-9\",\"{}\"]}}}}",
        pid
    );
    let _ = agent_command(vm_name, &kill_cmd);

    Err(VmmError::Other(format!(
        "Command execution timed out (30s), guest pid {} may still be running",
        pid
    )))
}

/// Validate a filename is safe for use in guest commands.
/// Prevents command injection via shell metacharacters (CWE-78).
fn validate_guest_filename(name: &str) -> VmmResult<()> {
    // Block shell metacharacters that could enable command injection
    // in cmd.exe (& | ; ` $ ( ) < > ^ !) or sh/bash
    const DANGEROUS_CHARS: &[char] = &[
        '&', '|', ';', '`', '$', '(', ')', '<', '>', '^', '!', '\n', '\r', '\0', '"', '\'', '%',
    ];
    if name.chars().any(|c| DANGEROUS_CHARS.contains(&c)) {
        return Err(VmmError::Other(format!(
            "Filename contains dangerous characters (blocked for security): {}",
            name
        )));
    }
    if name.contains("..") {
        return Err(VmmError::Other(
            "Filename must not contain '..' (path traversal)".to_string(),
        ));
    }
    Ok(())
}

/// Open a file in the guest using the default application.
/// This is the "Shared Applications" feature — host file opens in guest app.
pub fn open_file_in_guest(
    vm_name: &str,
    host_path: &Path,
    shared_folder: Option<&str>,
) -> VmmResult<()> {
    // SECURITY: CWE-552 — Block opening sensitive host files in guest apps.
    validate_host_source_path(host_path)?;

    let guest_path: String;

    // SECURITY: Validate filename before using in guest commands (CWE-78)
    let filename = host_path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("file");
    validate_guest_filename(filename)?;

    // If shared folder is available, construct the guest path directly
    if let Some(share_path) = shared_folder {
        // Check if the file is already inside the shared folder
        if let Ok(rel) = host_path.strip_prefix(share_path) {
            let rel_str = rel.to_str().unwrap_or(filename);
            validate_guest_filename(rel_str)?;
            // File is in shared folder — construct guest mount path
            let os = detect_guest_os(vm_name);
            guest_path = match os {
                GuestOsType::Windows => format!("Z:\\{}", rel_str.replace('/', "\\")),
                GuestOsType::Linux => format!("/mnt/shared/{}", rel_str),
                GuestOsType::Unknown => format!("/mnt/shared/{}", rel_str),
            };
        } else {
            // File is NOT in shared folder — transfer it first
            let os = detect_guest_os(vm_name);
            let temp_dir = match os {
                GuestOsType::Windows => "C:\\Users\\Public\\Documents",
                _ => "/tmp",
            };
            guest_path = format!("{}/{}", temp_dir, filename);
            transfer_file_to_guest(vm_name, host_path, &guest_path)?;
        }
    } else {
        // No shared folder — transfer file to guest temp dir
        let os = detect_guest_os(vm_name);
        let temp_dir = match os {
            GuestOsType::Windows => "C:\\Users\\Public\\Documents",
            _ => "/tmp",
        };
        guest_path = format!("{}/{}", temp_dir, filename);
        transfer_file_to_guest(vm_name, host_path, &guest_path)?;
    }

    // Open the file with the default application
    // SECURITY: On Windows, use powershell Start-Process instead of cmd.exe /c start
    // to prevent command injection via shell metacharacters (CWE-78).
    // cmd.exe /c start treats & | ; etc. as metacharacters even inside arguments.
    let os = detect_guest_os(vm_name);
    match os {
        GuestOsType::Windows => {
            // SECURITY: CWE-78 — Pass the guest path via an environment variable
            // using -EncodedCommand with a Base64-encoded script block. This avoids
            // any possibility of breaking out of string interpolation in PowerShell.
            // Previously used single-quote escaping (.replace('\'', "''")), but
            // PowerShell single-quoted strings still can't contain newlines, and
            // specially crafted paths could inject commands via line breaks or
            // null bytes that survive validation.
            let ps_script = format!(
                "Start-Process -FilePath ([System.Text.Encoding]::UTF8.GetString(\
                [System.Convert]::FromBase64String('{}')))",
                base64_encode(guest_path.as_bytes())
            );
            // Encode the entire script as UTF-16LE base64 for -EncodedCommand
            let utf16le: Vec<u8> = ps_script
                .encode_utf16()
                .flat_map(|c| c.to_le_bytes())
                .collect();
            let encoded_cmd = base64_encode(&utf16le);
            exec_in_guest(
                vm_name,
                "powershell.exe",
                &[
                    "-NoProfile",
                    "-NonInteractive",
                    "-EncodedCommand",
                    &encoded_cmd,
                ],
            )?;
        },
        GuestOsType::Linux => {
            exec_in_guest(vm_name, "xdg-open", &[&guest_path])?;
        },
        GuestOsType::Unknown => {
            warn!("Unknown guest OS, trying xdg-open");
            exec_in_guest(vm_name, "xdg-open", &[&guest_path])?;
        },
    }

    info!(
        "Opened file '{}' in guest default application",
        host_path.display()
    );
    Ok(())
}

/// Open a URL in the guest's default browser.
/// SECURITY: Validates URL scheme to prevent command injection (CWE-78).
pub fn open_url_in_guest(vm_name: &str, url: &str) -> VmmResult<()> {
    // SECURITY: Validate URL to prevent injection (CWE-78)
    // Only allow http/https/ftp schemes — block file://, javascript:, data:, etc.
    let url_lower = url.to_lowercase();
    if !url_lower.starts_with("http://")
        && !url_lower.starts_with("https://")
        && !url_lower.starts_with("ftp://")
    {
        return Err(VmmError::Other(format!(
            "URL scheme not allowed (only http/https/ftp): {}",
            url
        )));
    }
    // Block shell metacharacters in URLs
    const DANGEROUS_CHARS: &[char] = &[
        '&', '|', ';', '`', '$', '(', ')', '<', '>', '\n', '\r', '\0',
    ];
    if url.chars().any(|c| DANGEROUS_CHARS.contains(&c)) {
        return Err(VmmError::Other(format!(
            "URL contains dangerous characters (blocked for security): {}",
            url
        )));
    }

    let os = detect_guest_os(vm_name);
    match os {
        GuestOsType::Windows => {
            // SECURITY: CWE-78 — Encode the URL as base64 and decode inside PowerShell
            // to prevent any injection via string interpolation. Single-quote escaping
            // is insufficient because newlines and null bytes can break out.
            let ps_script = format!(
                "Start-Process -FilePath ([System.Text.Encoding]::UTF8.GetString(\
                [System.Convert]::FromBase64String('{}')))",
                base64_encode(url.as_bytes())
            );
            let utf16le: Vec<u8> = ps_script
                .encode_utf16()
                .flat_map(|c| c.to_le_bytes())
                .collect();
            let encoded_cmd = base64_encode(&utf16le);
            exec_in_guest(
                vm_name,
                "powershell.exe",
                &[
                    "-NoProfile",
                    "-NonInteractive",
                    "-EncodedCommand",
                    &encoded_cmd,
                ],
            )?;
        },
        GuestOsType::Linux => {
            exec_in_guest(vm_name, "xdg-open", &[url])?;
        },
        GuestOsType::Unknown => {
            exec_in_guest(vm_name, "xdg-open", &[url])?;
        },
    }
    info!("Opened URL '{}' in guest browser", url);
    Ok(())
}

/// Get the shared folder path on the guest side.
pub fn guest_shared_folder_path(vm_name: &str) -> String {
    let os = detect_guest_os(vm_name);
    match os {
        GuestOsType::Windows => "Z:\\".to_string(),
        _ => "/mnt/shared".to_string(),
    }
}

/// List files in the guest's shared folder.
pub fn list_guest_shared_files(vm_name: &str) -> VmmResult<Vec<String>> {
    let os = detect_guest_os(vm_name);
    let path = guest_shared_folder_path(vm_name);

    // SECURITY: Use PowerShell Get-ChildItem instead of cmd.exe /c dir (CWE-78).
    // cmd.exe /c interprets shell metacharacters in the path argument,
    // while PowerShell's -LiteralPath treats the path as a literal string.
    let result = match os {
        GuestOsType::Windows => {
            // SECURITY: CWE-78 — Encode the path as base64 and decode inside PowerShell
            // to prevent injection via single-quote breakout or newline injection.
            let ps_script = format!(
                "Get-ChildItem -LiteralPath ([System.Text.Encoding]::UTF8.GetString(\
                [System.Convert]::FromBase64String('{}'))) -Name",
                base64_encode(path.as_bytes())
            );
            let utf16le: Vec<u8> = ps_script
                .encode_utf16()
                .flat_map(|c| c.to_le_bytes())
                .collect();
            let encoded_cmd = base64_encode(&utf16le);
            exec_in_guest(
                vm_name,
                "powershell.exe",
                &[
                    "-NoProfile",
                    "-NonInteractive",
                    "-EncodedCommand",
                    &encoded_cmd,
                ],
            )?
        },
        _ => {
            // ls with "--" to prevent path injection as flags
            exec_in_guest(vm_name, "ls", &["-1", "--", &path])?
        },
    };

    if result.success {
        Ok(result
            .stdout
            .lines()
            .map(|l| l.to_string())
            .filter(|l| !l.is_empty())
            .collect())
    } else {
        Err(VmmError::Other(format!(
            "Failed to list files: {}",
            result.stderr
        )))
    }
}

/// Auto-mount the virtiofs shared folder inside the guest.
/// Linux: creates /mnt/shared and runs `mount -t virtiofs shared0 /mnt/shared`
/// Windows: virtiofs is typically auto-mapped by WinFsp/VirtioFS driver.
pub fn auto_mount_shared_folder(vm_name: &str) -> VmmResult<()> {
    let os = detect_guest_os(vm_name);
    match os {
        GuestOsType::Windows => {
            // Windows + WinFsp typically auto-mounts; nothing to do
            info!("Windows guest: virtiofs auto-mount handled by WinFsp driver");
            Ok(())
        },
        GuestOsType::Linux | GuestOsType::Unknown => {
            // Create mount point
            let mkdir_result = exec_in_guest(vm_name, "mkdir", &["-p", "--", "/mnt/shared"])?;
            if !mkdir_result.success {
                warn!("mkdir /mnt/shared failed: {}", mkdir_result.stderr);
            }

            // Mount virtiofs
            let mount_result = exec_in_guest(
                vm_name,
                "mount",
                &["-t", "virtiofs", "shared0", "/mnt/shared"],
            )?;

            if mount_result.success {
                info!(
                    "Shared folder auto-mounted at /mnt/shared for VM '{}'",
                    vm_name
                );
                Ok(())
            } else {
                // May already be mounted
                if mount_result.stderr.contains("already mounted") {
                    info!("Shared folder already mounted at /mnt/shared");
                    Ok(())
                } else {
                    Err(VmmError::Other(format!(
                        "Failed to mount shared folder: {}",
                        mount_result.stderr
                    )))
                }
            }
        },
    }
}

// ===== Internal helpers =====

fn ping_agent(vm_name: &str) -> bool {
    agent_command(vm_name, "{\"execute\":\"guest-ping\"}").is_ok()
}

fn detect_guest_os(vm_name: &str) -> GuestOsType {
    if let Ok(resp) = agent_command(vm_name, "{\"execute\":\"guest-get-osinfo\"}") {
        if resp.contains("Windows") || resp.contains("windows") || resp.contains("mswindows") {
            return GuestOsType::Windows;
        }
        if resp.contains("Linux") || resp.contains("linux") {
            return GuestOsType::Linux;
        }
    }
    GuestOsType::Unknown
}

fn check_exec_support(vm_name: &str) -> bool {
    // Try to get supported commands list
    if let Ok(resp) = agent_command(vm_name, "{\"execute\":\"guest-info\"}") {
        resp.contains("guest-exec")
    } else {
        false
    }
}

fn check_shared_folder(vm_name: &str) -> bool {
    // Check if virtiofs mount is present in guest
    let os = detect_guest_os(vm_name);
    let result = match os {
        GuestOsType::Windows => {
            // Check if Z: drive exists (common mapping for virtiofs)
            agent_command(vm_name, "{\"execute\":\"guest-get-fsinfo\"}")
                .map(|r| r.contains("virtiofs") || r.contains("Z:"))
                .unwrap_or(false)
        },
        _ => {
            // Check if /mnt/shared is mounted
            agent_command(vm_name, "{\"execute\":\"guest-get-fsinfo\"}")
                .map(|r| r.contains("virtiofs") || r.contains("/mnt/shared"))
                .unwrap_or(false)
        },
    };
    result
}

fn agent_command(vm_name: &str, cmd: &str) -> VmmResult<String> {
    // SECURITY: Validate VM name before passing to virsh to prevent argument injection (CWE-88)
    // Names starting with '-' would be interpreted as virsh flags
    if vm_name.starts_with('-') || vm_name.is_empty() {
        return Err(VmmError::Other(format!(
            "Invalid VM name for virsh command: {}",
            vm_name
        )));
    }
    // SECURITY: CWE-88 — Use "--" to prevent VM name from being interpreted as a flag.
    // SECURITY: CWE-400 — Place --timeout BEFORE positional args so virsh actually
    // enforces it. Previously --timeout was after the cmd string, causing virsh to
    // ignore it — guest commands could hang the host thread indefinitely.
    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance to child process.
    let output = std::process::Command::new("virsh")
        .args(["qemu-agent-command", "--timeout", "10", "--", vm_name, cmd])
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

/// SECURITY: Parse JSON responses from the QEMU Guest Agent using serde_json
/// instead of naive string matching. Naive string matching is vulnerable to
/// crafted QGA responses that embed fake keys in string values (CWE-94).
///
/// For example, a malicious guest could return:
///   {"return": {"fake": "value\",\"exited\":true,\"exitcode\":0,\"real_exited\":false"}}
/// which would trick naive parsers into seeing exited=true with exitcode=0.

/// Parse a JSON string once, returning the parsed Value for reuse.
/// Avoids redundant parsing when multiple fields are extracted from the same response.
fn extract_json_value(json: &str) -> Option<serde_json::Value> {
    serde_json::from_str(json).ok()
}

#[allow(dead_code)]
fn extract_json_str(json: &str, key: &str) -> Option<String> {
    extract_json_str_from_value(&extract_json_value(json)?, key)
}

fn extract_json_str_from_value(parsed: &serde_json::Value, key: &str) -> Option<String> {
    // Check top-level, then inside "return" object (QGA wraps responses)
    if let Some(val) = parsed.get(key).and_then(|v| v.as_str()) {
        return Some(val.to_string());
    }
    if let Some(ret) = parsed.get("return") {
        if let Some(val) = ret.get(key).and_then(|v| v.as_str()) {
            return Some(val.to_string());
        }
    }
    None
}

fn extract_json_number(json: &str, key: &str) -> Option<u64> {
    extract_json_number_from_value(&extract_json_value(json)?, key)
}

fn extract_json_number_from_value(parsed: &serde_json::Value, key: &str) -> Option<u64> {
    // Check top-level, then inside "return" object
    if let Some(val) = parsed.get(key).and_then(|v| v.as_u64()) {
        return Some(val);
    }
    if let Some(ret) = parsed.get("return") {
        // "return" can be a bare number (e.g., file handle) or an object
        if key == "return" {
            return ret.as_u64();
        }
        if let Some(val) = ret.get(key).and_then(|v| v.as_u64()) {
            return Some(val);
        }
    }
    None
}

#[allow(dead_code)]
fn extract_json_bool(json: &str, key: &str) -> Option<bool> {
    extract_json_bool_from_value(&extract_json_value(json)?, key)
}

fn extract_json_bool_from_value(parsed: &serde_json::Value, key: &str) -> Option<bool> {
    // Check top-level, then inside "return" object
    if let Some(val) = parsed.get(key).and_then(|v| v.as_bool()) {
        return Some(val);
    }
    if let Some(ret) = parsed.get("return") {
        if let Some(val) = ret.get(key).and_then(|v| v.as_bool()) {
            return Some(val);
        }
    }
    None
}

/// SECURITY: Escape all JSON special characters and control characters (CWE-94).
/// Control characters (U+0000–U+001F) in JSON strings can cause parsing issues
/// or inject unexpected content.
fn escape_json_str(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 16);
    for c in s.chars() {
        match c {
            '\\' => result.push_str("\\\\"),
            '"' => result.push_str("\\\""),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            '\0' => result.push_str("\\u0000"),
            c if c.is_control() => {
                // Escape other control characters as \uXXXX
                result.push_str(&format!("\\u{:04x}", c as u32));
            },
            c => result.push(c),
        }
    }
    result
}

/// Simple base64 encoder (no external dependency).
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity((data.len() * 4 / 3) + 4);
    let mut i = 0;
    while i + 2 < data.len() {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8) | (data[i + 2] as u32);
        result.push(CHARS[((n >> 18) & 63) as usize] as char);
        result.push(CHARS[((n >> 12) & 63) as usize] as char);
        result.push(CHARS[((n >> 6) & 63) as usize] as char);
        result.push(CHARS[(n & 63) as usize] as char);
        i += 3;
    }
    match data.len() - i {
        1 => {
            let n = (data[i] as u32) << 16;
            result.push(CHARS[((n >> 18) & 63) as usize] as char);
            result.push(CHARS[((n >> 12) & 63) as usize] as char);
            result.push('=');
            result.push('=');
        },
        2 => {
            let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8);
            result.push(CHARS[((n >> 18) & 63) as usize] as char);
            result.push(CHARS[((n >> 12) & 63) as usize] as char);
            result.push(CHARS[((n >> 6) & 63) as usize] as char);
            result.push('=');
        },
        _ => {},
    }
    result
}

/// Simple base64 decoder.
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

    // SECURITY: CWE-125 — Handle remaining base64 characters after the main
    // 4-char loop. Padding '=' chars were stripped earlier, so remaining count
    // tells us how many output bytes to emit:
    //   2 remaining chars -> 1 output byte  (original had 1 byte, 2 padding)
    //   3 remaining chars -> 2 output bytes (original had 2 bytes, 1 padding)
    //   1 remaining char  -> invalid base64, skip gracefully
    //   0 remaining       -> nothing to do
    // Previously this code used a fragile pop-then-re-push pattern that could
    // produce incorrect output and risked index confusion.
    let remaining = bytes.len() - i;
    match remaining {
        2 => {
            let a = DECODE.get(bytes[i] as usize).copied().unwrap_or(0) as u32;
            let b = DECODE.get(bytes[i + 1] as usize).copied().unwrap_or(0) as u32;
            let n = (a << 18) | (b << 12);
            output.push((n >> 16) as u8);
        },
        3 => {
            let a = DECODE.get(bytes[i] as usize).copied().unwrap_or(0) as u32;
            let b = DECODE.get(bytes[i + 1] as usize).copied().unwrap_or(0) as u32;
            let c = DECODE.get(bytes[i + 2] as usize).copied().unwrap_or(0) as u32;
            let n = (a << 18) | (b << 12) | (c << 6);
            output.push((n >> 16) as u8);
            output.push(((n >> 8) & 0xFF) as u8);
        },
        _ => {
            // 0 remaining = clean end; 1 remaining = malformed input, skip
        },
    }

    String::from_utf8_lossy(&output).to_string()
}
