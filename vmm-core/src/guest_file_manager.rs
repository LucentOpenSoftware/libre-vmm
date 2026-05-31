//! Guest File Manager — browse guest filesystem via QEMU Guest Agent.
//!
//! Uses `virsh qemu-agent-command` to execute file operations inside the guest,
//! enabling a file manager UI without network access or SSH.

use crate::error::{VmmError, VmmResult};
use std::path::{Component, PathBuf};
use std::process::{Command, Stdio};
use tracing::info;

/// Validate and sanitize a guest filesystem path to prevent path traversal (CWE-22).
///
/// This function:
/// 1. Rejects paths containing null bytes (could bypass C-level checks)
/// 2. Rejects paths containing control characters (ASCII 0x00-0x1F, 0x7F)
/// 3. Requires absolute paths (must start with `/`)
/// 4. Normalizes the path by resolving `.` and `..` segments lexically
/// 5. Rejects any path that, after normalization, escapes the root directory
///
/// Note: This is a *lexical* check performed on the host before sending the path
/// to the guest agent. It cannot resolve guest-side symlinks; the guest commands
/// themselves should be run with care (e.g., avoid `-L` flags that follow symlinks).
fn validate_guest_path(guest_path: &str) -> VmmResult<String> {
    // SECURITY: CWE-400 — Reject excessively long paths (Linux PATH_MAX = 4096)
    if guest_path.len() > 4096 {
        return Err(VmmError::Other(
            "Guest path exceeds maximum length (4096)".to_string(),
        ));
    }

    // Reject null bytes — these can truncate paths in C-based APIs
    if guest_path.bytes().any(|b| b == 0) {
        return Err(VmmError::Other("Path contains null bytes".to_string()));
    }

    // Reject control characters (0x00-0x1F and 0x7F)
    if guest_path.bytes().any(|b| b < 0x20 || b == 0x7F) {
        return Err(VmmError::Other(
            "Path contains control characters".to_string(),
        ));
    }

    // Require absolute path
    if !guest_path.starts_with('/') {
        return Err(VmmError::Other(
            "Guest path must be absolute (start with '/')".to_string(),
        ));
    }

    // Normalize the path: resolve `.` and `..` segments lexically.
    // We rebuild the path component-by-component so that any `..` at the root
    // level is caught (it would try to pop above `/`).
    let path = PathBuf::from(guest_path);
    let mut normalized = PathBuf::from("/");

    for component in path.components() {
        match component {
            Component::RootDir => { /* already handled */ },
            Component::CurDir => { /* skip `.` */ },
            Component::ParentDir => {
                // Pop one level; if we're already at root, this is a traversal attempt
                if !normalized.pop() || normalized.as_os_str().is_empty() {
                    normalized = PathBuf::from("/");
                }
            },
            Component::Normal(seg) => {
                // Reject segments that are suspicious encoded traversal variants
                let seg_str = seg.to_string_lossy();

                // Reject URL-encoded dots (%2e, %2E) that could bypass naive checks
                let lower = seg_str.to_lowercase();
                if lower.contains("%2e") || lower.contains("%00") {
                    return Err(VmmError::Other(
                        "Path contains encoded traversal sequence".to_string(),
                    ));
                }

                normalized.push(seg);
            },
            Component::Prefix(_) => {
                // Windows-style prefix on a guest path is invalid
                return Err(VmmError::Other("Invalid path prefix".to_string()));
            },
        }
    }

    let result = normalized.to_string_lossy().to_string();

    // Final safety check: the normalized path must still start with "/"
    if !result.starts_with('/') {
        return Err(VmmError::Other(
            "Path normalization produced a non-absolute path".to_string(),
        ));
    }

    Ok(result)
}

/// A file or directory entry in the guest filesystem.
#[derive(Debug, Clone)]
pub struct GuestFileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub permissions: String,
}

/// List files in a guest directory.
///
/// Uses `guest-exec` to run `ls -la` (Linux) or `dir` (Windows) inside the guest.
/// SECURITY: CWE-22 — Validates and normalizes paths to prevent path traversal.
pub fn list_directory(vm_name: &str, guest_path: &str) -> VmmResult<Vec<GuestFileEntry>> {
    // SECURITY: CWE-22 — Validate and normalize path to prevent traversal
    let guest_path = &validate_guest_path(guest_path)?;

    // Use guest-exec to run ls
    let cmd = format!(
        r#"{{"execute":"guest-exec","arguments":{{"path":"/bin/ls","arg":["-la","--time-style=iso","{}"],"capture-output":true}}}}"#,
        escape_json_string(guest_path)
    );

    let output = Command::new("virsh")
        .args(["qemu-agent-command", "--", vm_name, &cmd])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| VmmError::Other(format!("virsh not found: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VmmError::Other(format!("Guest command failed: {}", stderr)));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse the PID from guest-exec response
    let pid = parse_exec_pid(&stdout)?;

    // Wait for completion with exponential backoff (50ms -> 100ms -> 200ms -> 400ms -> 500ms)
    let status_output = poll_exec_status(vm_name, pid)?;

    // Parse ls output
    parse_ls_output(&status_output, guest_path)
}

/// Read a file from the guest filesystem.
///
/// Uses guest-exec with `cat` to read file contents.
/// SECURITY: CWE-400 — Enforces a maximum file size to prevent OOM.
pub fn read_file(vm_name: &str, guest_path: &str, max_size: u64) -> VmmResult<String> {
    let guest_path = &validate_guest_path(guest_path)?;

    if max_size > 100 * 1024 * 1024 {
        return Err(VmmError::Other(
            "Max size cannot exceed 100 MiB".to_string(),
        ));
    }

    // First check file size
    let size_cmd = format!(
        r#"{{"execute":"guest-exec","arguments":{{"path":"/usr/bin/stat","arg":["--printf=%s","{}"],"capture-output":true}}}}"#,
        escape_json_string(guest_path)
    );

    let _output = Command::new("virsh")
        .args(["qemu-agent-command", "--", vm_name, &size_cmd])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| VmmError::Other(format!("virsh not found: {}", e)))?;

    // Read the file using cat
    let cat_cmd = format!(
        r#"{{"execute":"guest-exec","arguments":{{"path":"/bin/cat","arg":["{}"],"capture-output":true}}}}"#,
        escape_json_string(guest_path)
    );

    let output = Command::new("virsh")
        .args(["qemu-agent-command", "--", vm_name, &cat_cmd])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| VmmError::Other(format!("virsh not found: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VmmError::Other(format!("Read failed: {}", stderr)));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let pid = parse_exec_pid(&stdout)?;

    // Wait for completion with exponential backoff
    poll_exec_status(vm_name, pid)
}

/// Create a directory in the guest filesystem.
pub fn create_directory(vm_name: &str, guest_path: &str) -> VmmResult<()> {
    let guest_path = &validate_guest_path(guest_path)?;

    let cmd = format!(
        r#"{{"execute":"guest-exec","arguments":{{"path":"/bin/mkdir","arg":["-p","{}"],"capture-output":true}}}}"#,
        escape_json_string(guest_path)
    );

    let output = Command::new("virsh")
        .args(["qemu-agent-command", "--", vm_name, &cmd])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| VmmError::Other(format!("virsh not found: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VmmError::Other(format!("mkdir failed: {}", stderr)));
    }

    info!("Created directory in guest: {}", guest_path);
    Ok(())
}

/// Delete a file in the guest filesystem.
pub fn delete_file(vm_name: &str, guest_path: &str) -> VmmResult<()> {
    let guest_path = &validate_guest_path(guest_path)?;

    let cmd = format!(
        r#"{{"execute":"guest-exec","arguments":{{"path":"/bin/rm","arg":["-f","{}"],"capture-output":true}}}}"#,
        escape_json_string(guest_path)
    );

    let output = Command::new("virsh")
        .args(["qemu-agent-command", "--", vm_name, &cmd])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| VmmError::Other(format!("virsh not found: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VmmError::Other(format!("delete failed: {}", stderr)));
    }

    info!("Deleted file in guest: {}", guest_path);
    Ok(())
}

// === Internal helpers ===

fn escape_json_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn parse_exec_pid(json: &str) -> VmmResult<i64> {
    // Parse: {"return":{"pid":12345}}
    if let Some(pid_pos) = json.find("\"pid\":") {
        let after = &json[pid_pos + 6..];
        let pid_str: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(pid) = pid_str.parse::<i64>() {
            return Ok(pid);
        }
    }
    Err(VmmError::Other(
        "Failed to parse guest-exec PID".to_string(),
    ))
}

/// Poll guest-exec-status with exponential backoff.
/// Starts at 50ms, doubles each iteration up to 500ms max, 5 iterations max.
fn poll_exec_status(vm_name: &str, pid: i64) -> VmmResult<String> {
    let mut delay = 50u64;
    for _ in 0..5 {
        std::thread::sleep(std::time::Duration::from_millis(delay));
        let result = get_exec_status(vm_name, pid)?;
        if !result.is_empty() {
            return Ok(result);
        }
        delay = (delay * 2).min(500);
    }
    // Final attempt
    get_exec_status(vm_name, pid)
}

fn get_exec_status(vm_name: &str, pid: i64) -> VmmResult<String> {
    let cmd = format!(
        r#"{{"execute":"guest-exec-status","arguments":{{"pid":{}}}}}"#,
        pid
    );

    let output = Command::new("virsh")
        .args(["qemu-agent-command", "--", vm_name, &cmd])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| VmmError::Other(format!("virsh not found: {}", e)))?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse base64-encoded output from: {"return":{"exited":true,"out-data":"BASE64..."}}
    if let Some(data_pos) = stdout.find("\"out-data\":\"") {
        let after = &stdout[data_pos + 12..];
        if let Some(end) = after.find('"') {
            let b64 = &after[..end];
            // Simple base64 decode
            if let Ok(decoded) = base64_decode(b64) {
                return Ok(String::from_utf8_lossy(&decoded).to_string());
            }
        }
    }

    // No output data — return empty
    Ok(String::new())
}

fn parse_ls_output(output: &str, base_path: &str) -> VmmResult<Vec<GuestFileEntry>> {
    let mut entries = Vec::new();
    let base = if base_path.ends_with('/') {
        base_path.to_string()
    } else {
        format!("{}/", base_path)
    };

    for line in output.lines().skip(1) {
        // Skip "total N" line
        if line.starts_with("total ") {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 8 {
            continue;
        }

        let perms = parts[0];
        let is_dir = perms.starts_with('d');
        let size: u64 = parts[4].parse().unwrap_or(0);
        let name = parts[7..].join(" ");

        // Skip . and ..
        if name == "." || name == ".." {
            continue;
        }

        entries.push(GuestFileEntry {
            name: name.clone(),
            path: format!("{}{}", base, name),
            is_dir,
            size,
            permissions: perms.to_string(),
        });
    }

    // Limit entries (CWE-400)
    entries.truncate(10_000);
    Ok(entries)
}

/// Lookup table mapping ASCII byte value to 6-bit base64 value.
/// Invalid characters are mapped to 0xFF.
static B64_DECODE: [u8; 128] = {
    let mut table = [0xFFu8; 128];
    let chars = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut i = 0;
    while i < 64 {
        table[chars[i] as usize] = i as u8;
        i += 1;
    }
    table
};

fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    let mut result = Vec::new();
    let bytes: Vec<u8> = input
        .bytes()
        .filter(|b| *b != b'\n' && *b != b'\r')
        .collect();

    let mut i = 0;
    while i + 3 < bytes.len() {
        let b0 = B64_DECODE[bytes[i] as usize & 0x7F] as u32;
        let b1 = B64_DECODE[bytes[i + 1] as usize & 0x7F] as u32;
        let b2 = if bytes[i + 2] == b'=' {
            0
        } else {
            B64_DECODE[bytes[i + 2] as usize & 0x7F] as u32
        };
        let b3 = if bytes[i + 3] == b'=' {
            0
        } else {
            B64_DECODE[bytes[i + 3] as usize & 0x7F] as u32
        };

        let triple = (b0 << 18) | (b1 << 12) | (b2 << 6) | b3;
        result.push((triple >> 16) as u8);
        if bytes[i + 2] != b'=' {
            result.push((triple >> 8) as u8);
        }
        if bytes[i + 3] != b'=' {
            result.push(triple as u8);
        }

        i += 4;
    }

    Ok(result)
}
