//! Storage pool management.

use crate::config::VmConfigIo;
use crate::error::{VmmError, VmmResult};
use std::path::Path;
use virt::connect::Connect;
use virt::storage_pool::StoragePool;

/// Info about a storage pool.
#[derive(Debug, Clone)]
pub struct PoolInfo {
    pub name: String,
    pub active: bool,
    pub capacity_bytes: u64,
    pub available_bytes: u64,
    pub path: String,
}

/// List storage pools.
pub fn list_pools(conn: &Connect) -> VmmResult<Vec<PoolInfo>> {
    let pools = conn
        .list_all_storage_pools(0)
        .map_err(|e| VmmError::StorageError(format!("Failed to list pools: {}", e)))?;

    let mut result = Vec::new();
    for pool in pools {
        let name = pool.get_name().unwrap_or_default();
        let active = pool.is_active().unwrap_or(false);

        let (capacity, available) = if active {
            let info = pool
                .get_info()
                .unwrap_or(virt::storage_pool::StoragePoolInfo {
                    state: 0,
                    capacity: 0,
                    allocation: 0,
                    available: 0,
                });
            (info.capacity, info.available)
        } else {
            (0, 0)
        };

        let xml = pool.get_xml_desc(0).unwrap_or_default();
        let path = extract_pool_path(&xml).unwrap_or_default();

        result.push(PoolInfo {
            name,
            active,
            capacity_bytes: capacity,
            available_bytes: available,
            path,
        });
    }

    Ok(result)
}

/// Ensure the default storage pool exists for Libre VMM.
pub fn ensure_default_pool(conn: &Connect) -> VmmResult<()> {
    let pool_name = "libre-vmm";
    let pool_path = crate::config::VmConfig::default_vm_dir();

    match StoragePool::lookup_by_name(conn, pool_name) {
        Ok(pool) => {
            if !pool.is_active().unwrap_or(false) {
                pool.create(0)
                    .map_err(|e| VmmError::StorageError(format!("Failed to start pool: {}", e)))?;
            }
            Ok(())
        },
        Err(_) => {
            // SECURITY: Validate pool_path before use (CWE-22, CWE-59, CWE-367).
            // pool_path derives from $HOME which could be attacker-controlled.
            validate_pool_path(&pool_path)?;

            std::fs::create_dir_all(&pool_path)?;

            // SECURITY: After create_dir_all, verify the final path is not a symlink (CWE-367).
            // A TOCTOU race could plant a symlink between validation and create_dir_all.
            // Re-check with lstat to detect symlinks at the target.
            let lmeta = std::fs::symlink_metadata(&pool_path).map_err(|e| {
                VmmError::StorageError(format!("Cannot lstat pool path '{}': {}", pool_path, e))
            })?;
            if lmeta.file_type().is_symlink() {
                return Err(VmmError::StorageError(format!(
                    "Pool path '{}' is a symbolic link (blocked for security, CWE-59)",
                    pool_path
                )));
            }

            // SECURITY: Restrict directory permissions to owner-only (CWE-732).
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = std::fs::Permissions::from_mode(0o700);
                let _ = std::fs::set_permissions(&pool_path, perms);
            }

            // SECURITY: XML-escape pool_path to prevent XML injection via directory names (CWE-91).
            // pool_name is hardcoded ("libre-vmm") so it's safe, but pool_path derives from
            // user's home directory which could theoretically contain XML metacharacters.
            let escaped_path = xml_escape(&pool_path);
            let xml = format!(
                r#"<pool type='dir'>
  <name>{}</name>
  <target>
    <path>{}</path>
  </target>
</pool>"#,
                pool_name, escaped_path
            );

            let pool = StoragePool::define_xml(conn, &xml, 0)
                .map_err(|e| VmmError::StorageError(format!("Failed to define pool: {}", e)))?;
            pool.create(0)
                .map_err(|e| VmmError::StorageError(format!("Failed to start pool: {}", e)))?;
            pool.set_autostart(true)
                .map_err(|e| VmmError::StorageError(format!("Failed to set autostart: {}", e)))?;

            Ok(())
        },
    }
}

/// SECURITY: Validate that a storage pool path is safe (CWE-22, CWE-59).
///
/// Prevents path traversal and symlink attacks by ensuring the path:
/// - Is absolute
/// - Does not contain `..` components
/// - Is not a symlink (if it already exists)
/// - Does not point into sensitive system directories
fn validate_pool_path(path: &str) -> VmmResult<()> {
    let p = Path::new(path);

    // CWE-22: Must be absolute — reject relative paths that could resolve unpredictably
    if !p.is_absolute() {
        return Err(VmmError::StorageError(format!(
            "Storage pool path must be absolute: {}",
            path
        )));
    }

    // CWE-22: Reject ".." components — prevents traversal out of intended directories
    for component in p.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err(VmmError::StorageError(format!(
                "Storage pool path must not contain '..': {}",
                path
            )));
        }
    }

    // CWE-59: Check each existing ancestor for symlinks. create_dir_all follows symlinks
    // at every path component, so an attacker could plant a symlink at any intermediate
    // directory (e.g., ~/.local/share -> /etc) to redirect the pool to a sensitive location.
    let mut check = p.to_path_buf();
    while let Some(parent) = check.parent() {
        if parent == Path::new("/") {
            break;
        }
        if parent.exists() {
            let lmeta = std::fs::symlink_metadata(parent).map_err(|e| {
                VmmError::StorageError(format!("Cannot lstat '{}': {}", parent.display(), e))
            })?;
            if lmeta.file_type().is_symlink() {
                return Err(VmmError::StorageError(format!(
                    "Path component '{}' is a symbolic link (blocked for security, CWE-59)",
                    parent.display()
                )));
            }
        }
        check = parent.to_path_buf();
    }
    // Also check the final path itself if it exists
    if p.exists() {
        let lmeta = std::fs::symlink_metadata(p)
            .map_err(|e| VmmError::StorageError(format!("Cannot lstat '{}': {}", path, e)))?;
        if lmeta.file_type().is_symlink() {
            return Err(VmmError::StorageError(format!(
                "Storage pool path is a symbolic link (blocked for security, CWE-59): {}",
                path
            )));
        }
    }

    // CWE-22: Block sensitive system directories — a manipulated $HOME could point here
    let blocked_prefixes = [
        "/etc", "/root", "/proc", "/sys", "/dev", "/boot", "/run", "/bin", "/sbin", "/usr",
    ];
    for prefix in blocked_prefixes {
        if path.starts_with(prefix) {
            return Err(VmmError::StorageError(format!(
                "Storage pool path must not be inside '{}': {}",
                prefix, path
            )));
        }
    }

    Ok(())
}

/// SECURITY: Escape XML special characters to prevent injection (CWE-91).
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn extract_pool_path(xml: &str) -> Option<String> {
    let tag = "<path>";
    let end_tag = "</path>";
    let start = xml.find(tag)? + tag.len();
    let end = xml[start..].find(end_tag)? + start;
    Some(xml[start..end].to_string())
}
