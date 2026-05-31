//! Default hardware preferences — persisted user defaults for new VMs.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// User preferences for default VM hardware settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preferences {
    /// Default number of CPUs for new VMs.
    #[serde(default = "default_cpus")]
    pub default_cpus: u32,
    /// Default memory in MiB for new VMs.
    #[serde(default = "default_memory")]
    pub default_memory_mib: u64,
    /// Default disk size in GiB for new VMs.
    #[serde(default = "default_disk")]
    pub default_disk_gib: u64,
    /// Default network mode.
    #[serde(default)]
    pub default_network: String,
    /// Whether to enable UEFI by default.
    #[serde(default)]
    pub default_uefi: bool,
    /// Auto-suspend VMs on host shutdown.
    #[serde(default)]
    pub auto_suspend_on_shutdown: bool,
    /// Auto-mount shared folders in guest.
    #[serde(default)]
    pub shared_folder_auto_mount: bool,
    /// Has the user completed (or dismissed) the first-run setup wizard?
    /// When false on launch with no existing VMs, the wizard opens automatically.
    #[serde(default)]
    pub first_run_completed: bool,
}

fn default_cpus() -> u32 {
    2
}
fn default_memory() -> u64 {
    2048
}
fn default_disk() -> u64 {
    20
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            default_cpus: default_cpus(),
            default_memory_mib: default_memory(),
            default_disk_gib: default_disk(),
            default_network: "NAT".to_string(),
            default_uefi: false,
            auto_suspend_on_shutdown: true,
            shared_folder_auto_mount: false,
            first_run_completed: false,
        }
    }
}

impl Preferences {
    /// Get the preferences file path.
    fn path() -> PathBuf {
        let base = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("libre-vmm");
        base.join("preferences.json")
    }

    /// Load preferences from disk, or return defaults.
    pub fn load() -> Self {
        let path = Self::path();
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
                Err(_) => Self::default(),
            }
        } else {
            Self::default()
        }
    }

    /// Save preferences to disk.
    ///
    /// SECURITY (CWE-732): Sets restrictive file permissions (0o600) on the preferences file.
    /// Without this, the default umask may leave the file world-readable, exposing
    /// user preferences and configuration choices to other local users.
    /// Also restricts the parent directory to 0o700.
    pub fn save(&self) -> Result<(), String> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create preferences dir: {}", e))?;
            // SECURITY (CWE-732): Restrict directory permissions
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
            }
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize preferences: {}", e))?;
        std::fs::write(&path, &json).map_err(|e| format!("Failed to write preferences: {}", e))?;
        // SECURITY (CWE-732): Set restrictive file permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }
}
