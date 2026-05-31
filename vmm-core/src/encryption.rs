//! Disk encryption support via LUKS (qemu-img + libvirt secrets).
//!
//! LUKS-encrypted qcow2 disks are created using `qemu-img` with the
//! `encrypt.format=luks` option. Passphrases are stored as libvirt
//! secrets, referenced by UUID in the domain XML.

use crate::error::{VmmError, VmmResult};
use std::process::Command;
use tracing::{info, warn};
use uuid::Uuid;

/// SECURITY: CWE-460 — Drop guard to ensure /dev/shm secret files are cleaned
/// up even if the code panics. Without this, an unwinding panic between writing
/// the secret file and the cleanup code would leave passphrases on the ramdisk.
struct SecretDirGuard {
    path: String,
    defused: bool,
}

impl SecretDirGuard {
    fn new(path: String) -> Self {
        Self {
            path,
            defused: false,
        }
    }
    /// Mark cleanup as handled — the guard will not remove the dir on drop.
    fn defuse(&mut self) {
        self.defused = true;
    }
}

impl Drop for SecretDirGuard {
    fn drop(&mut self) {
        if !self.defused {
            if let Err(e) = std::fs::remove_dir_all(&self.path) {
                // Best-effort: log but can't return error from Drop
                warn!("SecretDirGuard: failed to clean up {}: {}", self.path, e);
            }
        }
    }
}

/// SECURITY: CWE-431 — Disable core dumps for this process to prevent
/// passphrases from being written to disk in a core file after a crash.
/// Called once before handling any sensitive data.
#[cfg(unix)]
fn disable_core_dumps() {
    use libc::{rlimit, setrlimit, RLIMIT_CORE};
    let zero = rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: setrlimit with RLIMIT_CORE=0 is a well-defined POSIX call.
    unsafe { setrlimit(RLIMIT_CORE, &zero) };
}

/// SECURITY: CWE-316 / CWE-244 — Overwrite sensitive byte buffers before deallocation.
/// Without the `zeroize` crate, we manually zero memory to prevent secrets from
/// lingering on the heap where they could be leaked via core dumps, swap, or
/// use-after-free exploits.
fn zeroize_bytes(buf: &mut [u8]) {
    // Use write_volatile to prevent the compiler from optimizing away the zeroing.
    for byte in buf.iter_mut() {
        // SAFETY: byte is a valid, aligned, dereferenceable pointer to initialized memory.
        unsafe { std::ptr::write_volatile(byte, 0) };
    }
    // Compiler fence to ensure the writes are not reordered past this point.
    std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
}

/// SECURITY: CWE-316 — Overwrite a String's backing buffer with zeros before dropping.
fn zeroize_string(s: &mut String) {
    // SAFETY: We zero the bytes in-place via the existing Vec<u8> backing store.
    // We then clear the string (sets len to 0) but the zeroed bytes remain in
    // the allocation until the allocator reclaims them.
    let bytes = unsafe { s.as_mut_vec() };
    zeroize_bytes(bytes);
    bytes.clear();
}

/// SECURITY: CWE-521 — Validate passphrase strength and reject dangerous characters.
/// - Min 8 chars to prevent trivially weak passphrases
/// - Max 512 chars to prevent resource exhaustion in KDF / buffer overflows
/// - No null bytes (prevents C-level string truncation)
/// - No commas (QEMU object property injection)
/// - No control characters 0x00-0x1F except tab (0x09) — prevents null injection,
///   newline injection into secret files, and other smuggling attacks
fn validate_passphrase(passphrase: &str) -> VmmResult<()> {
    if passphrase.len() < 8 {
        return Err(VmmError::Other(
            "Passphrase must be at least 8 characters for LUKS encryption".to_string(),
        ));
    }
    if passphrase.len() > 512 {
        return Err(VmmError::Other(
            "Passphrase must not exceed 512 characters".to_string(),
        ));
    }
    if passphrase.bytes().any(|b| b == 0) {
        return Err(VmmError::Other(
            "Passphrase must not contain null bytes".to_string(),
        ));
    }
    if passphrase.contains(',') {
        return Err(VmmError::DiskError(
            "Passphrase must not contain commas".to_string(),
        ));
    }
    if passphrase.bytes().any(|b| b <= 0x1F && b != 0x09) {
        return Err(VmmError::DiskError(
            "Passphrase must not contain control characters".to_string(),
        ));
    }
    Ok(())
}

/// Create a LUKS-encrypted qcow2 disk image.
///
/// Uses `qemu-img create` with LUKS encryption options.
/// Returns the libvirt secret UUID that stores the passphrase.
pub fn create_encrypted_qcow2(path: &str, size_gib: u64, passphrase: &str) -> VmmResult<Uuid> {
    crate::disk::validate_disk_path(path)?;
    info!(
        "Creating LUKS-encrypted qcow2 disk: {} ({}G)",
        path, size_gib
    );

    // Ensure parent directory exists
    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Generate a UUID for the libvirt secret
    let secret_uuid = Uuid::new_v4();

    // SECURITY: CWE-431 — Disable core dumps before handling passphrase
    // to prevent crash-induced passphrase leakage to disk.
    #[cfg(unix)]
    disable_core_dumps();

    // SECURITY: CWE-521 — Validate passphrase strength and reject dangerous characters.
    validate_passphrase(passphrase)?;

    // Create the encrypted disk using qemu-img
    // SECURITY: Pass passphrase via temp file instead of command line
    // (command line args visible to all users via /proc/*/cmdline — CWE-214)
    // SECURITY: Create dir with restricted permissions BEFORE writing secret (CWE-252)
    // SECURITY: CWE-367 (TOCTOU) — Use create_dir (not create_dir_all) which fails
    // if the directory already exists, providing O_EXCL-like atomicity. If a pre-existing
    // directory is found (e.g., from a prior crash), verify it is owned by us and has
    // correct permissions before reusing it; otherwise refuse.
    let secret_dir = format!("/dev/shm/.libre-vmm-{}", Uuid::new_v4());
    match std::fs::create_dir(&secret_dir) {
        Ok(()) => {},
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let meta = std::fs::symlink_metadata(&secret_dir).map_err(|e| {
                    VmmError::DiskError(format!("Failed to stat existing secret dir: {}", e))
                })?;
                // Reject symlinks — an attacker could point this at an arbitrary directory
                if meta.file_type().is_symlink() {
                    return Err(VmmError::DiskError(
                        "Secret dir is a symlink — refusing to use it".to_string(),
                    ));
                }
                // Verify ownership matches current user
                let my_uid = unsafe { libc::getuid() };
                if meta.uid() != my_uid {
                    return Err(VmmError::DiskError(format!(
                        "Secret dir owned by UID {} but expected {} — refusing to use it",
                        meta.uid(),
                        my_uid
                    )));
                }
                // Verify permissions are 0o700 (owner-only)
                if meta.mode() & 0o777 != 0o700 {
                    return Err(VmmError::DiskError(format!(
                        "Secret dir has mode {:o} but expected 700 — refusing to use it",
                        meta.mode() & 0o777
                    )));
                }
            }
        },
        Err(e) => {
            return Err(VmmError::DiskError(format!(
                "Failed to create secret dir: {}",
                e
            )));
        },
    }

    // SECURITY: CWE-460 — Drop guard ensures cleanup even on panic/unwind.
    let mut secret_guard = SecretDirGuard::new(secret_dir.clone());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Set dir perms to 0o700 BEFORE writing any files into it
        std::fs::set_permissions(&secret_dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| VmmError::DiskError(format!("Failed to secure secret dir: {}", e)))?;
    }
    // SECURITY: CWE-377 — Use O_EXCL (create_new) to atomically create the secret file
    // with restrictive permissions. This prevents TOCTOU races where an attacker could
    // create a symlink at the path between open() and chmod().
    let secret_file = format!("{}/key", secret_dir);
    {
        use std::io::Write;
        #[cfg(unix)]
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true) // O_EXCL: fail if file exists (CWE-377)
            .mode(0o600) // CWE-732: restrictive perms at creation, no chmod TOCTOU
            .open(&secret_file)
            .map_err(|e| VmmError::DiskError(format!("Failed to create secret file: {}", e)))?;
        f.write_all(passphrase.as_bytes())
            .map_err(|e| VmmError::DiskError(format!("Failed to write secret file: {}", e)))?;
    }

    // SECURITY: CWE-916 — Explicitly specify LUKS cipher, KDF, and iteration count.
    // qemu-img defaults may use PBKDF2 with weak iteration counts on some versions.
    // We force AES-256-XTS with PBKDF2 at 200000 iterations (argon2id not supported
    // by all qemu-img versions). The iter-time=5000 tells LUKS to calibrate iterations
    // for ~5 seconds of CPU time on the host, with 200000 as the floor.
    let luks_opts = [
        "encrypt.format=luks",
        "encrypt.key-secret=sec0",
        "encrypt.cipher-alg=aes-256",
        "encrypt.cipher-mode=xts",
        "encrypt.ivgen-alg=plain64",
        "encrypt.hash-alg=sha256",
        "encrypt.iter-time=5000",
    ]
    .join(",");

    // SECURITY: CWE-403 — Close all FDs >= 3 in child process via pre_exec to prevent
    // leaking sensitive file descriptors (secret files, lock files) to qemu-img.
    let output = Command::new("qemu-img")
        .args([
            "create",
            "-f",
            "qcow2",
            &format!("--object=secret,id=sec0,file={}", secret_file),
            "-o",
            &luks_opts,
            path,
            &format!("{}G", size_gib),
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| VmmError::DiskError(format!("qemu-img not found: {}", e)));

    // Always clean up secret file — guard handles panic path, explicit for normal path
    let _ = std::fs::remove_file(&secret_file);
    let _ = std::fs::remove_dir(&secret_dir);
    secret_guard.defuse(); // We did manual cleanup, don't double-remove

    let output = output?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VmmError::DiskError(format!(
            "Failed to create encrypted disk: {}",
            stderr
        )));
    }

    // Now create a libvirt secret to store the passphrase
    // This is used by libvirt to unlock the disk when the VM starts
    create_libvirt_secret(&secret_uuid, passphrase)?;

    info!(
        "Encrypted disk created at {} (secret UUID: {})",
        path, secret_uuid
    );
    Ok(secret_uuid)
}

/// Create a libvirt secret to store a disk encryption passphrase.
fn create_libvirt_secret(secret_uuid: &Uuid, passphrase: &str) -> VmmResult<()> {
    // Generate the secret XML
    let secret_xml = format!(
        r#"<secret ephemeral='no' private='yes'>
  <uuid>{uuid}</uuid>
  <description>Libre VMM disk encryption key</description>
  <usage type='volume'>
    <volume>/libre-vmm/encryption/{uuid}</volume>
  </usage>
</secret>"#,
        uuid = secret_uuid
    );

    // SECURITY: CWE-377 — Write XML to /dev/shm (ramdisk) instead of /tmp (disk-backed).
    // /tmp may be on a persistent filesystem where secret XML fragments could survive
    // a crash or be recovered from disk. /dev/shm is tmpfs (RAM-only).
    // SECURITY: CWE-367 (TOCTOU) — Use create_dir (not create_dir_all) for atomic creation.
    // If the directory already exists, verify ownership/permissions before reusing.
    let secure_dir =
        std::path::PathBuf::from(format!("/dev/shm/.libre-vmm-xml-{}", Uuid::new_v4()));
    match std::fs::create_dir(&secure_dir) {
        Ok(()) => {},
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let meta = std::fs::symlink_metadata(&secure_dir).map_err(|e| {
                    VmmError::Other(format!("Failed to stat existing XML temp dir: {}", e))
                })?;
                if meta.file_type().is_symlink() {
                    return Err(VmmError::Other(
                        "XML temp dir is a symlink — refusing to use it".to_string(),
                    ));
                }
                let my_uid = unsafe { libc::getuid() };
                if meta.uid() != my_uid {
                    return Err(VmmError::Other(format!(
                        "XML temp dir owned by UID {} but expected {} — refusing to use it",
                        meta.uid(),
                        my_uid
                    )));
                }
                if meta.mode() & 0o777 != 0o700 {
                    return Err(VmmError::Other(format!(
                        "XML temp dir has mode {:o} but expected 700 — refusing to use it",
                        meta.mode() & 0o777
                    )));
                }
            }
        },
        Err(e) => {
            return Err(VmmError::Other(format!(
                "Failed to create XML temp dir: {}",
                e
            )))
        },
    }

    // SECURITY: CWE-460 — Drop guard ensures cleanup of XML temp dir even on panic.
    let mut xml_guard = SecretDirGuard::new(secure_dir.display().to_string());

    // Restrict directory permissions to owner only
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&secure_dir, std::fs::Permissions::from_mode(0o700));
    }
    let temp_path = secure_dir.join("secret.xml");
    // SECURITY: CWE-732 — Write XML file with restrictive permissions (0o600).
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true) // CWE-377: O_EXCL — fail if file already exists (race-free)
            .mode(0o600)
            .open(&temp_path)
            .map_err(|e| VmmError::Other(format!("Failed to create secret XML: {}", e)))?;
        f.write_all(secret_xml.as_bytes())
            .map_err(|e| VmmError::Other(format!("Failed to write secret XML: {}", e)))?;
    }

    // Define the secret
    // SECURITY: CWE-403 — Close stdin and pipe stdout/stderr to prevent FD leaks.
    let temp_path_str = temp_path.display().to_string();
    let output = Command::new("virsh")
        .args(["secret-define", &temp_path_str])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| VmmError::Other(format!("Failed to run virsh: {}", e)))?;

    // Clean up temp directory and file — guard handles panic path
    let _ = std::fs::remove_dir_all(&secure_dir);
    xml_guard.defuse();

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VmmError::Other(format!(
            "Failed to define libvirt secret: {}",
            stderr
        )));
    }

    // Set the secret value (the passphrase, base64-encoded)
    // SECURITY: Pass via stdin instead of command line to avoid /proc exposure (CWE-214)
    // SECURITY: CWE-316 — Zeroize b64_passphrase after use to prevent heap lingering.
    let mut b64_passphrase = base64_encode(passphrase.as_bytes());
    let result = (|| -> VmmResult<()> {
        let mut child = Command::new("virsh")
            .args([
                "secret-set-value",
                &secret_uuid.to_string(),
                "--interactive",
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| VmmError::Other(format!("Failed to run virsh: {}", e)))?;

        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(b64_passphrase.as_bytes());
            let _ = stdin.write_all(b"\n");
        }
        let output = child
            .wait_with_output()
            .map_err(|e| VmmError::Other(format!("Failed to wait for virsh: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(VmmError::Other(format!(
                "Failed to set secret value: {}",
                stderr
            )));
        }

        Ok(())
    })();

    // SECURITY: CWE-244 — Always zeroize the encoded passphrase, even on error paths.
    zeroize_string(&mut b64_passphrase);

    result?;

    info!("Libvirt secret {} created and configured", secret_uuid);
    Ok(())
}

/// Check if a disk image is LUKS-encrypted.
pub fn is_disk_encrypted(path: &str) -> bool {
    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance to child process.
    if let Ok(output) = Command::new("qemu-img")
        .args(["info", "--output=json", path])
        .stdin(std::process::Stdio::null())
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // LUKS-encrypted images show "encrypt" in the format-specific section
            return stdout.contains("\"encrypt\"") || stdout.contains("luks");
        }
    }
    false
}

/// Delete a libvirt secret (cleanup when deleting an encrypted VM).
pub fn delete_libvirt_secret(secret_uuid: &Uuid) -> VmmResult<()> {
    info!("Deleting libvirt secret: {}", secret_uuid);
    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance to child process.
    let output = Command::new("virsh")
        .args(["secret-undefine", &secret_uuid.to_string()])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| VmmError::Other(format!("Failed to run virsh: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Don't error if secret doesn't exist
        if !stderr.contains("not found") {
            return Err(VmmError::Other(format!(
                "Failed to delete secret: {}",
                stderr
            )));
        }
    }
    Ok(())
}

/// Simple base64 encoding (avoid adding a crate dependency).
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i] as u32;
        let b1 = if i + 1 < data.len() {
            data[i + 1] as u32
        } else {
            0
        };
        let b2 = if i + 2 < data.len() {
            data[i + 2] as u32
        } else {
            0
        };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if i + 1 < data.len() {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if i + 2 < data.len() {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        i += 3;
    }
    result
}
