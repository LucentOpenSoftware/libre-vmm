//! Restricted VM policies: prevent modification of locked-down VMs.
//!
//! A restricted VM has an attached policy file (`<uuid>.policy.json`) that
//! enumerates allowed operations. The policy is enforced in connection.rs at
//! every mutating operation: settings update, USB attach, power-off, delete,
//! snapshot, clone, etc.
//!
//! SECURITY MODEL: This is intent-enforcement, not capability-based security.
//! It's defense for a cooperative environment (student labs, contractor VMs,
//! shared workstations), not a sandbox against a determined attacker. A user
//! with file-system access can edit/delete the policy file. For real isolation,
//! combine with disk encryption (LUKS) and OS-level file ACLs.

use crate::config::VmConfigIo;
use crate::error::{VmmError, VmmResult};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Operations that may be restricted by a policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    ModifyConfig,
    AttachUsb,
    AddSharedFolder,
    TakeSnapshot,
    RevertSnapshot,
    Clone,
    Export,
    ForceStop,
    Delete,
    ChangeNetwork,
}

impl Operation {
    /// Human-readable name of the operation, used in error messages.
    fn label(self) -> &'static str {
        match self {
            Operation::ModifyConfig => "modify VM configuration",
            Operation::AttachUsb => "attach USB device",
            Operation::AddSharedFolder => "add shared folder",
            Operation::TakeSnapshot => "take snapshot",
            Operation::RevertSnapshot => "revert snapshot",
            Operation::Clone => "clone VM",
            Operation::Export => "export VM",
            Operation::ForceStop => "force-stop VM",
            Operation::Delete => "delete VM",
            Operation::ChangeNetwork => "change network configuration",
        }
    }
}

/// A policy file attached to a VM. All fields default to permissive
/// (false / empty / None) so an absent or partial policy never accidentally
/// locks a VM that was not meant to be restricted.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RestrictionPolicy {
    /// Human-readable policy name (e.g., "Student Lab").
    #[serde(default)]
    pub name: String,

    /// Policy author / issuer.
    #[serde(default)]
    pub issuer: String,

    /// When this policy expires. If `Some` and current time > expiration,
    /// the VM is treated as unusable for any mutating operation.
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,

    /// If true, VM config cannot be modified at all.
    #[serde(default)]
    pub read_only_config: bool,

    /// If true, no USB device can be attached.
    #[serde(default)]
    pub block_usb: bool,

    /// If true, no shared folder can be added/changed.
    #[serde(default)]
    pub block_shared_folders: bool,

    /// If true, snapshots cannot be created or reverted.
    #[serde(default)]
    pub block_snapshots: bool,

    /// If true, VM cannot be cloned or exported.
    #[serde(default)]
    pub block_clone_export: bool,

    /// If true, force-stop is prevented (graceful shutdown only).
    #[serde(default)]
    pub block_force_stop: bool,

    /// If true, VM cannot be deleted.
    #[serde(default)]
    pub block_delete: bool,

    /// If true, network mode cannot be changed.
    #[serde(default)]
    pub block_network_change: bool,

    /// Optional message shown to the user in the GUI explaining restrictions.
    #[serde(default)]
    pub user_message: String,
}

impl RestrictionPolicy {
    /// Directory where restriction policy files are stored.
    fn policy_dir() -> std::path::PathBuf {
        let dir = format!("{}/restrictions", crate::config::VmConfig::config_dir());
        std::path::PathBuf::from(dir)
    }

    /// Path where the policy for `vm_id` is stored.
    pub fn policy_path(vm_id: &Uuid) -> std::path::PathBuf {
        Self::policy_dir().join(format!("{}.policy.json", vm_id))
    }

    /// Load a policy for a VM. Returns `Ok(None)` if no policy file exists.
    pub fn load(vm_id: &Uuid) -> VmmResult<Option<Self>> {
        let path = Self::policy_path(vm_id);
        if !path.exists() {
            return Ok(None);
        }
        let json = std::fs::read_to_string(&path)?;
        let policy: Self = serde_json::from_str(&json)?;
        Ok(Some(policy))
    }

    /// Save the policy file with restrictive permissions (0o600 on Unix).
    ///
    /// SECURITY: Writes atomically (write to tmp + rename) so a crash or
    /// concurrent reader never observes a partially-written policy that
    /// could be misinterpreted as permissive. The temp file lives in the
    /// same directory as the target so `rename(2)` is atomic on the same
    /// filesystem.
    pub fn save(&self, vm_id: &Uuid) -> VmmResult<()> {
        let dir = Self::policy_dir();
        std::fs::create_dir_all(&dir)?;

        // SECURITY (CWE-732): Restrict directory to owner-only.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        }

        let final_path = Self::policy_path(vm_id);
        // Place tmp file alongside the target to ensure rename() is atomic
        // (atomicity requires same filesystem).
        let tmp_path = dir.join(format!(".{}.policy.json.tmp.{}", vm_id, std::process::id()));

        let json = serde_json::to_string_pretty(self)?;

        // Open with create_new + 0o600 to avoid TOCTOU and accidentally
        // overwriting some other process's temp file.
        {
            use std::io::Write;
            #[cfg(unix)]
            use std::os::unix::fs::OpenOptionsExt;

            let mut f = {
                #[cfg(unix)]
                {
                    std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .mode(0o600)
                        .open(&tmp_path)?
                }
                #[cfg(not(unix))]
                {
                    std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&tmp_path)?
                }
            };
            f.write_all(json.as_bytes())?;
            f.sync_all()?;
        }

        // Atomic rename onto final path.
        if let Err(e) = std::fs::rename(&tmp_path, &final_path) {
            // Best effort cleanup.
            let _ = std::fs::remove_file(&tmp_path);
            return Err(e.into());
        }

        // Re-assert permissions on the final file (rename preserves the
        // tmp file's mode, but be explicit).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&final_path, std::fs::Permissions::from_mode(0o600));
        }

        Ok(())
    }

    /// Delete the policy file (removes restrictions). It is not an error
    /// for the file to be absent.
    pub fn delete(vm_id: &Uuid) -> VmmResult<()> {
        let path = Self::policy_path(vm_id);
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }

    /// Returns true if the policy carries an expiration and that
    /// expiration is in the past.
    pub fn is_expired(&self) -> bool {
        match self.expires_at {
            Some(deadline) => Utc::now() > deadline,
            None => false,
        }
    }

    /// Returns a reason string if the named operation is blocked, or
    /// `None` if the operation is allowed under this policy.
    pub fn check(&self, op: Operation) -> Option<String> {
        if self.is_expired() {
            return Some(format!(
                "policy '{}' expired; VM is locked",
                if self.name.is_empty() {
                    "restriction"
                } else {
                    &self.name
                }
            ));
        }

        let blocked = match op {
            Operation::ModifyConfig => self.read_only_config,
            Operation::AttachUsb => self.block_usb,
            Operation::AddSharedFolder => self.block_shared_folders,
            Operation::TakeSnapshot | Operation::RevertSnapshot => self.block_snapshots,
            Operation::Clone | Operation::Export => self.block_clone_export,
            Operation::ForceStop => self.block_force_stop,
            Operation::Delete => self.block_delete,
            Operation::ChangeNetwork => self.block_network_change,
        };

        if blocked {
            let extra = if self.user_message.is_empty() {
                String::new()
            } else {
                format!(" ({})", self.user_message)
            };
            Some(format!(
                "operation '{}' blocked by restriction policy{}",
                op.label(),
                extra
            ))
        } else {
            None
        }
    }
}

/// Convenience helper: convert a `check()` result into a `VmmResult`.
/// Used by `connection.rs` enforce_policy().
pub(crate) fn check_or_err(policy: &RestrictionPolicy, op: Operation) -> VmmResult<()> {
    if let Some(reason) = policy.check(op) {
        Err(VmmError::InvalidConfig(format!(
            "Restricted VM: {}",
            reason
        )))
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    /// Build a policy with custom storage dir for round-trip tests.
    /// We can't easily override `config_dir()` from tests without DI, so we
    /// instead test the on-disk save/load round-trip by invoking `save` and
    /// `load` against a real (per-test) temp UUID. The location is the user's
    /// real config dir; the policy file is deleted at the end of the test.
    fn fresh_uuid() -> Uuid {
        Uuid::new_v4()
    }

    #[test]
    fn round_trip_save_load() {
        let id = fresh_uuid();
        let original = RestrictionPolicy {
            name: "Student Lab".into(),
            issuer: "Prof. Smith".into(),
            expires_at: Some(Utc::now() + Duration::days(30)),
            read_only_config: true,
            block_usb: true,
            block_shared_folders: false,
            block_snapshots: true,
            block_clone_export: true,
            block_force_stop: false,
            block_delete: true,
            block_network_change: true,
            user_message: "Locked for CS-101 lab session".into(),
        };

        original.save(&id).expect("save");
        let loaded = RestrictionPolicy::load(&id)
            .expect("load")
            .expect("policy present");

        assert_eq!(loaded.name, original.name);
        assert_eq!(loaded.issuer, original.issuer);
        assert_eq!(loaded.read_only_config, original.read_only_config);
        assert_eq!(loaded.block_usb, original.block_usb);
        assert_eq!(loaded.block_shared_folders, original.block_shared_folders);
        assert_eq!(loaded.block_snapshots, original.block_snapshots);
        assert_eq!(loaded.block_clone_export, original.block_clone_export);
        assert_eq!(loaded.block_force_stop, original.block_force_stop);
        assert_eq!(loaded.block_delete, original.block_delete);
        assert_eq!(loaded.block_network_change, original.block_network_change);
        assert_eq!(loaded.user_message, original.user_message);
        assert!(loaded.expires_at.is_some());

        // Cleanup.
        RestrictionPolicy::delete(&id).expect("delete");
        assert!(RestrictionPolicy::load(&id).unwrap().is_none());
    }

    #[test]
    fn load_missing_returns_none() {
        let id = fresh_uuid();
        let result = RestrictionPolicy::load(&id).expect("load missing must not error");
        assert!(result.is_none());
    }

    #[test]
    fn delete_missing_is_ok() {
        let id = fresh_uuid();
        RestrictionPolicy::delete(&id).expect("delete missing must not error");
    }

    #[test]
    fn is_expired_none_is_false() {
        let p = RestrictionPolicy::default();
        assert!(!p.is_expired());
    }

    #[test]
    fn is_expired_future_is_false() {
        let mut p = RestrictionPolicy::default();
        p.expires_at = Some(Utc::now() + Duration::hours(1));
        assert!(!p.is_expired());
    }

    #[test]
    fn is_expired_past_is_true() {
        let mut p = RestrictionPolicy::default();
        p.expires_at = Some(Utc::now() - Duration::hours(1));
        assert!(p.is_expired());
    }

    #[test]
    fn check_default_policy_allows_everything() {
        let p = RestrictionPolicy::default();
        for op in [
            Operation::ModifyConfig,
            Operation::AttachUsb,
            Operation::AddSharedFolder,
            Operation::TakeSnapshot,
            Operation::RevertSnapshot,
            Operation::Clone,
            Operation::Export,
            Operation::ForceStop,
            Operation::Delete,
            Operation::ChangeNetwork,
        ] {
            assert!(p.check(op).is_none(), "default policy must allow {:?}", op);
        }
    }

    #[test]
    fn check_modify_config_blocked() {
        let mut p = RestrictionPolicy::default();
        p.read_only_config = true;
        assert!(p.check(Operation::ModifyConfig).is_some());
        // Other operations remain allowed.
        assert!(p.check(Operation::AttachUsb).is_none());
        assert!(p.check(Operation::Delete).is_none());
    }

    #[test]
    fn check_attach_usb_blocked() {
        let mut p = RestrictionPolicy::default();
        p.block_usb = true;
        assert!(p.check(Operation::AttachUsb).is_some());
        assert!(p.check(Operation::ModifyConfig).is_none());
    }

    #[test]
    fn check_shared_folder_blocked() {
        let mut p = RestrictionPolicy::default();
        p.block_shared_folders = true;
        assert!(p.check(Operation::AddSharedFolder).is_some());
    }

    #[test]
    fn check_snapshots_blocked_covers_both_ops() {
        let mut p = RestrictionPolicy::default();
        p.block_snapshots = true;
        assert!(p.check(Operation::TakeSnapshot).is_some());
        assert!(p.check(Operation::RevertSnapshot).is_some());
    }

    #[test]
    fn check_clone_export_blocked_covers_both_ops() {
        let mut p = RestrictionPolicy::default();
        p.block_clone_export = true;
        assert!(p.check(Operation::Clone).is_some());
        assert!(p.check(Operation::Export).is_some());
    }

    #[test]
    fn check_force_stop_blocked() {
        let mut p = RestrictionPolicy::default();
        p.block_force_stop = true;
        assert!(p.check(Operation::ForceStop).is_some());
    }

    #[test]
    fn check_delete_blocked() {
        let mut p = RestrictionPolicy::default();
        p.block_delete = true;
        assert!(p.check(Operation::Delete).is_some());
    }

    #[test]
    fn check_network_change_blocked() {
        let mut p = RestrictionPolicy::default();
        p.block_network_change = true;
        assert!(p.check(Operation::ChangeNetwork).is_some());
    }

    #[test]
    fn check_expired_blocks_everything() {
        let mut p = RestrictionPolicy::default();
        p.name = "Expired Lab".into();
        p.expires_at = Some(Utc::now() - Duration::days(1));
        // Even an otherwise-permissive policy blocks everything once expired.
        for op in [
            Operation::ModifyConfig,
            Operation::AttachUsb,
            Operation::Delete,
        ] {
            let msg = p.check(op).expect("expired policy must block");
            assert!(msg.to_lowercase().contains("expired"), "got: {}", msg);
        }
    }

    #[test]
    fn check_user_message_surfaces_in_reason() {
        let mut p = RestrictionPolicy::default();
        p.block_delete = true;
        p.user_message = "Contact admin to unlock".into();
        let reason = p.check(Operation::Delete).expect("blocked");
        assert!(
            reason.contains("Contact admin to unlock"),
            "got: {}",
            reason
        );
    }

    #[test]
    fn atomic_save_leaves_no_tmp_file() {
        let id = fresh_uuid();
        let p = RestrictionPolicy::default();
        p.save(&id).expect("save");

        // No leftover tmp file with our pid suffix.
        let dir = RestrictionPolicy::policy_dir();
        let prefix = format!(".{}.policy.json.tmp.", id);
        let entries = std::fs::read_dir(&dir).expect("read dir");
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            assert!(
                !name_str.starts_with(&prefix),
                "leftover tmp file: {}",
                name_str
            );
        }

        // Final policy file exists.
        assert!(RestrictionPolicy::policy_path(&id).exists());

        // Cleanup.
        RestrictionPolicy::delete(&id).expect("delete");
    }

    #[cfg(unix)]
    #[test]
    fn save_sets_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let id = fresh_uuid();
        let p = RestrictionPolicy::default();
        p.save(&id).expect("save");

        let meta = std::fs::metadata(RestrictionPolicy::policy_path(&id)).expect("meta");
        let mode = meta.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "expected 0o600, got {:o}", mode);

        RestrictionPolicy::delete(&id).expect("delete");
    }

    #[test]
    fn check_or_err_returns_err_when_blocked() {
        let mut p = RestrictionPolicy::default();
        p.block_delete = true;
        assert!(check_or_err(&p, Operation::Delete).is_err());
        assert!(check_or_err(&p, Operation::ModifyConfig).is_ok());
    }
}
