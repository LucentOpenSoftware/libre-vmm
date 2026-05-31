use std::fmt;

pub type VmmResult<T> = Result<T, VmmError>;

/// Sanitize a VM name for safe inclusion in error messages.
/// Strips control characters and truncates to prevent log injection (CWE-117)
/// and excessive output.
fn sanitize_name(name: &str) -> String {
    name.chars().filter(|c| !c.is_control()).take(128).collect()
}

/// Redact potentially sensitive information from libvirt error messages
/// to prevent credential and path disclosure (CWE-209).
///
/// Libvirt errors can contain connection URIs with embedded credentials
/// (e.g., `qemu+ssh://user:pass@host/system`) and full filesystem paths.
fn redact_libvirt_message(msg: &str) -> String {
    let mut result = msg.to_string();

    // Redact URIs that may contain credentials
    for scheme in &["qemu+ssh://", "qemu+tcp://", "qemu+tls://"] {
        if let Some(start) = result.find(scheme) {
            // Find the end of the URI (next whitespace or end of string)
            let uri_start = start;
            let uri_end = result[start..]
                .find(|c: char| c.is_whitespace())
                .map(|pos| start + pos)
                .unwrap_or(result.len());
            result.replace_range(uri_start..uri_end, &format!("{}[REDACTED]", scheme));
        }
    }

    result
}

#[derive(Debug)]
pub enum VmmError {
    /// Libvirt error. SECURITY (CWE-209): Display impl redacts connection URIs
    /// and credentials. The original error is preserved in Debug for diagnostics.
    Libvirt(virt::error::Error),

    /// VM not found. SECURITY (CWE-117): name is sanitized in Display.
    VmNotFound {
        name: String,
    },

    VmAlreadyRunning {
        name: String,
    },

    VmNotRunning {
        name: String,
    },

    DiskError(String),

    InvalidConfig(String),

    StorageError(String),

    NetworkError(String),

    SnapshotError(String),

    CloneError(String),

    XmlError(String),

    /// IO error. SECURITY (CWE-209): Display shows only the error kind,
    /// not the full message which may contain filesystem paths.
    Io(std::io::Error),

    Serde(serde_json::Error),

    Other(String),
}

// SECURITY: Manual Display impl to control exactly what is shown to users.
// This prevents information disclosure (CWE-209) and log injection (CWE-117).
impl fmt::Display for VmmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // CWE-209: Redact libvirt errors to strip credentials/paths
            VmmError::Libvirt(e) => {
                write!(
                    f,
                    "Libvirt error: {}",
                    redact_libvirt_message(&e.to_string())
                )
            },
            // CWE-117: Sanitize VM names to prevent control character injection
            VmmError::VmNotFound { name } => {
                write!(f, "VM '{}' not found", sanitize_name(name))
            },
            VmmError::VmAlreadyRunning { name } => {
                write!(f, "VM '{}' is already running", sanitize_name(name))
            },
            VmmError::VmNotRunning { name } => {
                write!(f, "VM '{}' is not running", sanitize_name(name))
            },
            VmmError::DiskError(msg) => write!(f, "Disk image error: {}", msg),
            VmmError::InvalidConfig(msg) => write!(f, "Invalid configuration: {}", msg),
            VmmError::StorageError(msg) => write!(f, "Storage pool error: {}", msg),
            VmmError::NetworkError(msg) => write!(f, "Network error: {}", msg),
            VmmError::SnapshotError(msg) => write!(f, "Snapshot error: {}", msg),
            VmmError::CloneError(msg) => write!(f, "Clone error: {}", msg),
            VmmError::XmlError(msg) => write!(f, "XML generation error: {}", msg),
            // CWE-209: Only show error kind, not full path from IO errors
            VmmError::Io(e) => write!(f, "IO error: {}", e.kind()),
            VmmError::Serde(e) => write!(f, "Serialization error: {}", e),
            VmmError::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for VmmError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            VmmError::Libvirt(e) => Some(e),
            VmmError::Io(e) => Some(e),
            VmmError::Serde(e) => Some(e),
            _ => None,
        }
    }
}

impl From<virt::error::Error> for VmmError {
    fn from(e: virt::error::Error) -> Self {
        VmmError::Libvirt(e)
    }
}

impl From<std::io::Error> for VmmError {
    fn from(e: std::io::Error) -> Self {
        VmmError::Io(e)
    }
}

impl From<serde_json::Error> for VmmError {
    fn from(e: serde_json::Error) -> Self {
        VmmError::Serde(e)
    }
}
