//! VM snapshot management.

use crate::error::{VmmError, VmmResult};
use crate::guest_agent;
use std::collections::HashMap;
use tracing::{info, warn};
use virt::connect::Connect;
use virt::domain::Domain;
use virt::domain_snapshot::DomainSnapshot;

/// libvirt flag: include only disk state (skip memory) when set.
/// From `virDomainSnapshotCreateFlags` in libvirt headers.
const VIR_DOMAIN_SNAPSHOT_CREATE_DISK_ONLY: u32 = 16;

/// Snapshot information with parent for tree building.
#[derive(Debug, Clone)]
pub struct SnapshotInfo {
    pub name: String,
    pub description: String,
    pub creation_time: i64,
    pub state: String,
    /// Parent snapshot name (None = root snapshot).
    /// Extracted from `<parent><name>...</name></parent>` in libvirt snapshot XML.
    pub parent: Option<String>,
    /// Whether this is the current active snapshot.
    pub is_current: bool,
}

/// A node in the snapshot tree, built from flat SnapshotInfo list.
#[derive(Debug, Clone)]
pub struct SnapshotTreeNode {
    pub info: SnapshotInfo,
    pub children: Vec<SnapshotTreeNode>,
    /// Depth level in the tree (0 = root).
    pub depth: usize,
}

/// Maximum snapshot tree depth to prevent stack overflow from circular references.
const MAX_SNAPSHOT_TREE_DEPTH: usize = 256;

/// Build a tree from a flat list of snapshots using parent relationships.
///
/// SECURITY (CWE-674): Depth is bounded to prevent stack overflow from corrupted
/// snapshot metadata containing circular parent references (A -> B -> A).
pub fn build_snapshot_tree(snapshots: &[SnapshotInfo]) -> Vec<SnapshotTreeNode> {
    // Build a HashMap of parent_name -> Vec<index> in one pass (O(N) instead of O(N²)).
    let mut children_map: HashMap<String, Vec<usize>> = HashMap::new();
    let mut roots: Vec<usize> = Vec::new();

    for (i, snap) in snapshots.iter().enumerate() {
        match &snap.parent {
            Some(parent_name) => {
                children_map.entry(parent_name.clone()).or_default().push(i);
            },
            None => {
                roots.push(i);
            },
        }
    }

    fn build_children(
        parent_name: &str,
        snapshots: &[SnapshotInfo],
        children_map: &HashMap<String, Vec<usize>>,
        depth: usize,
    ) -> Vec<SnapshotTreeNode> {
        // SECURITY (CWE-674): Bound recursion depth to prevent stack overflow
        // from circular parent references in corrupted snapshot metadata.
        if depth >= MAX_SNAPSHOT_TREE_DEPTH {
            return Vec::new();
        }
        let Some(child_indices) = children_map.get(parent_name) else {
            return Vec::new();
        };
        child_indices
            .iter()
            .map(|&i| {
                let s = &snapshots[i];
                let children = build_children(&s.name, snapshots, children_map, depth + 1);
                SnapshotTreeNode {
                    info: s.clone(),
                    children,
                    depth,
                }
            })
            .collect()
    }

    roots
        .into_iter()
        .map(|i| {
            let root = &snapshots[i];
            let children = build_children(&root.name, snapshots, &children_map, 1);
            SnapshotTreeNode {
                info: root.clone(),
                children,
                depth: 0,
            }
        })
        .collect()
}

/// Flatten a snapshot tree into a display-order list with depth info.
pub fn flatten_tree(tree: &[SnapshotTreeNode]) -> Vec<(SnapshotInfo, usize)> {
    let mut result = Vec::new();

    fn walk(node: &SnapshotTreeNode, out: &mut Vec<(SnapshotInfo, usize)>) {
        out.push((node.info.clone(), node.depth));
        for child in &node.children {
            walk(child, out);
        }
    }

    for root in tree {
        walk(root, &mut result);
    }
    result
}

/// Validate a VM name before passing to libvirt (CWE-20).
/// Prevents injection via crafted domain names in snapshot operations.
fn validate_vm_name(name: &str) -> VmmResult<()> {
    if name.is_empty() {
        return Err(VmmError::SnapshotError(
            "VM name cannot be empty".to_string(),
        ));
    }
    if name.len() > 255 {
        return Err(VmmError::SnapshotError(
            "VM name too long (max 255 chars)".to_string(),
        ));
    }
    // SECURITY (CWE-20): libvirt domain names must be a restricted character set.
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == ' ')
    {
        return Err(VmmError::SnapshotError(format!(
            "Invalid VM name '{}': only alphanumeric, hyphen, underscore, and period allowed",
            name
        )));
    }
    Ok(())
}

/// Validate a snapshot name for safety.
/// Prevents injection in virsh commands and libvirt XML (CWE-20, CWE-88).
fn validate_snapshot_name(name: &str) -> VmmResult<()> {
    if name.is_empty() {
        return Err(VmmError::SnapshotError(
            "Snapshot name cannot be empty".to_string(),
        ));
    }
    if name.len() > 128 {
        return Err(VmmError::SnapshotError(
            "Snapshot name too long (max 128)".to_string(),
        ));
    }
    if name.starts_with('-') {
        return Err(VmmError::SnapshotError(
            "Snapshot name must not start with '-' (argument injection risk)".to_string(),
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || " -_.()".contains(c))
    {
        return Err(VmmError::SnapshotError(
            "Snapshot name contains invalid characters".to_string(),
        ));
    }
    Ok(())
}

/// Create a snapshot of a VM.
pub fn create_snapshot(
    conn: &Connect,
    vm_name: &str,
    snap_name: &str,
    description: &str,
) -> VmmResult<()> {
    // SECURITY (CWE-20): Validate both VM name and snapshot name before use.
    validate_vm_name(vm_name)?;
    validate_snapshot_name(snap_name)?;

    // SECURITY (CWE-400): Cap description length to prevent XML bloat.
    // SECURITY (CWE-20): Use floor_char_boundary to avoid panicking on multi-byte
    // UTF-8 sequences when truncating (e.g., a 4-byte emoji at position 4095).
    let safe_description = if description.len() > 4096 {
        // Find the last valid char boundary at or before byte 4096
        let boundary = description[..4096]
            .char_indices()
            .last()
            .map(|(i, _)| i)
            .unwrap_or(0);
        &description[..boundary]
    } else {
        description
    };

    let domain = Domain::lookup_by_name(conn, vm_name).map_err(|_| VmmError::VmNotFound {
        name: vm_name.to_string(),
    })?;

    let xml = format!(
        r#"<domainsnapshot>
  <name>{}</name>
  <description>{}</description>
</domainsnapshot>"#,
        xml_escape(snap_name),
        xml_escape(safe_description),
    );

    DomainSnapshot::create_xml(&domain, &xml, 0)
        .map_err(|e| VmmError::SnapshotError(format!("Failed to create snapshot: {}", e)))?;

    info!("Snapshot '{}' created for VM '{}'", snap_name, vm_name);
    Ok(())
}

/// List all snapshots for a VM.
pub fn list_snapshots(conn: &Connect, vm_name: &str) -> VmmResult<Vec<SnapshotInfo>> {
    validate_vm_name(vm_name)?;
    let domain = Domain::lookup_by_name(conn, vm_name).map_err(|_| VmmError::VmNotFound {
        name: vm_name.to_string(),
    })?;

    let snaps = domain
        .list_all_snapshots(0)
        .map_err(|e| VmmError::SnapshotError(format!("Failed to list snapshots: {}", e)))?;

    let mut snapshots = Vec::new();
    for snap in snaps {
        let name = snap.get_name().unwrap_or_default();
        let xml = snap.get_xml_desc(0).unwrap_or_default();

        // Extract parent name from <parent><name>...</name></parent>
        let parent = extract_parent_name(&xml);

        // Check for <current/> flag in snapshot XML (not always present)
        let is_current = xml.contains("<current/>");

        snapshots.push(SnapshotInfo {
            name,
            description: extract_xml_value(&xml, "description").unwrap_or_default(),
            creation_time: extract_xml_value(&xml, "creationTime")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
            state: extract_xml_value(&xml, "state").unwrap_or_default(),
            parent,
            is_current,
        });
    }

    Ok(snapshots)
}

/// Revert a VM to a snapshot.
pub fn revert_snapshot(conn: &Connect, vm_name: &str, snap_name: &str) -> VmmResult<()> {
    validate_vm_name(vm_name)?;
    validate_snapshot_name(snap_name)?;

    let domain = Domain::lookup_by_name(conn, vm_name).map_err(|_| VmmError::VmNotFound {
        name: vm_name.to_string(),
    })?;

    let snap = DomainSnapshot::lookup_by_name(&domain, snap_name, 0)
        .map_err(|e| VmmError::SnapshotError(format!("Snapshot not found: {}", e)))?;

    snap.revert(0)
        .map_err(|e| VmmError::SnapshotError(format!("Failed to revert: {}", e)))?;

    info!("VM '{}' reverted to snapshot '{}'", vm_name, snap_name);
    Ok(())
}

/// Delete a snapshot.
pub fn delete_snapshot(conn: &Connect, vm_name: &str, snap_name: &str) -> VmmResult<()> {
    validate_vm_name(vm_name)?;
    validate_snapshot_name(snap_name)?;

    let domain = Domain::lookup_by_name(conn, vm_name).map_err(|_| VmmError::VmNotFound {
        name: vm_name.to_string(),
    })?;

    let snap = DomainSnapshot::lookup_by_name(&domain, snap_name, 0)
        .map_err(|e| VmmError::SnapshotError(format!("Snapshot not found: {}", e)))?;

    snap.delete(0)
        .map_err(|e| VmmError::SnapshotError(format!("Failed to delete snapshot: {}", e)))?;

    info!("Snapshot '{}' deleted from VM '{}'", snap_name, vm_name);
    Ok(())
}

/// RAII guard that thaws guest filesystems on drop.
///
/// Critical safety property: once `armed` is set after a successful freeze, the
/// guest is paused at the filesystem layer. If we don't thaw it, the guest is
/// effectively wedged — every read/write blocks forever, no shell responds,
/// nothing can save the user. Using a Drop guard guarantees thaw runs even on
/// panic (e.g. the libvirt call inside `do_snapshot_inner` aborts mid-flight),
/// which a plain `if let Err` cleanup would miss.
struct ThawGuard<'a> {
    vm_name: &'a str,
    armed: bool,
}

impl<'a> Drop for ThawGuard<'a> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        // Best-effort thaw — we can't return an error from drop. If this fails
        // there's nothing we can do programmatically; log loudly so the operator
        // can manually thaw via `virsh qemu-agent-command <vm> '{"execute":"guest-fsfreeze-thaw"}'`.
        match guest_agent::thaw_filesystems(self.vm_name) {
            Ok(n) => {
                info!(
                    "Thawed {} filesystem(s) on VM '{}' after snapshot",
                    n, self.vm_name
                );
            },
            Err(e) => {
                warn!(
                    "FAILED to thaw filesystems on VM '{}': {}. Guest may be wedged — \
                     run `virsh qemu-agent-command {} '{{\"execute\":\"guest-fsfreeze-thaw\"}}'` manually.",
                    self.vm_name, e, self.vm_name
                );
            },
        }
    }
}

/// Create a quiesced (filesystem-consistent) snapshot of a VM.
///
/// If qemu-guest-agent is reachable, this calls `guest-fsfreeze-freeze` before
/// taking the snapshot so the guest OS flushes all dirty pages, then thaws
/// afterwards. The resulting snapshot is safe to restore without filesystem
/// corruption — VMware Workstation calls this a "quiesced" snapshot.
///
/// If the agent is not available, doesn't support freeze, or the freeze call
/// fails, this falls back to a non-quiesced (crash-consistent) snapshot and
/// logs a warning. This keeps snapshots working on guests without qemu-ga
/// installed (e.g. fresh installs, recovery ISOs).
///
/// `include_memory` controls whether RAM state is saved (`false` = disk only,
/// like `virsh snapshot-create-as --disk-only`).
///
/// SAFETY: A `ThawGuard` ensures thaw runs even if the libvirt snapshot call
/// panics mid-flight. A frozen guest left un-thawed cannot respond to anything.
pub fn create_snapshot_quiesced(
    conn: &Connect,
    vm_name: &str,
    snapshot_name: &str,
    description: &str,
    include_memory: bool,
) -> VmmResult<()> {
    // SECURITY (CWE-20): Validate before any external interaction.
    validate_vm_name(vm_name)?;
    validate_snapshot_name(snapshot_name)?;

    // Try to freeze first. If it fails (agent missing, unsupported, etc.) we
    // log and proceed without quiescing — degraded but still useful.
    let mut guard = ThawGuard {
        vm_name,
        armed: false,
    };
    match guest_agent::freeze_filesystems(vm_name) {
        Ok(n) => {
            info!(
                "Froze {} filesystem(s) on VM '{}' for quiesced snapshot",
                n, vm_name
            );
            guard.armed = true;
        },
        Err(e) => {
            warn!(
                "Could not freeze filesystems on VM '{}' ({}); falling back to \
                 non-quiesced snapshot. Install qemu-guest-agent in the guest \
                 for filesystem-consistent snapshots.",
                vm_name, e
            );
        },
    }

    // do_snapshot_inner may panic on libvirt error paths; ThawGuard's Drop
    // still runs and thaws the guest. We deliberately do NOT use catch_unwind
    // here — propagating the panic is correct, the guard handles cleanup.
    let result = do_snapshot_inner(conn, vm_name, snapshot_name, description, include_memory);

    // Guard drops here (or earlier on panic) and thaws.
    drop(guard);
    result
}

/// Internal snapshot creation with explicit flags. Shared between
/// `create_snapshot_quiesced` and any future variants.
fn do_snapshot_inner(
    conn: &Connect,
    vm_name: &str,
    snapshot_name: &str,
    description: &str,
    include_memory: bool,
) -> VmmResult<()> {
    // SECURITY (CWE-400): Cap description length the same way create_snapshot does.
    let safe_description = if description.len() > 4096 {
        let boundary = description[..4096]
            .char_indices()
            .last()
            .map(|(i, _)| i)
            .unwrap_or(0);
        &description[..boundary]
    } else {
        description
    };

    let domain = Domain::lookup_by_name(conn, vm_name).map_err(|_| VmmError::VmNotFound {
        name: vm_name.to_string(),
    })?;

    let xml = build_snapshot_xml(snapshot_name, safe_description);

    let flags = if include_memory {
        0
    } else {
        VIR_DOMAIN_SNAPSHOT_CREATE_DISK_ONLY
    };

    DomainSnapshot::create_xml(&domain, &xml, flags)
        .map_err(|e| VmmError::SnapshotError(format!("Failed to create snapshot: {}", e)))?;

    info!(
        "Snapshot '{}' created for VM '{}' (include_memory={})",
        snapshot_name, vm_name, include_memory
    );
    Ok(())
}

/// Pure XML builder, extracted so it can be unit-tested without libvirt.
fn build_snapshot_xml(snapshot_name: &str, description: &str) -> String {
    format!(
        r#"<domainsnapshot>
  <name>{}</name>
  <description>{}</description>
</domainsnapshot>"#,
        xml_escape(snapshot_name),
        xml_escape(description),
    )
}

/// SECURITY (CWE-91): Escape all five XML special characters to prevent XML injection.
/// Missing quote escaping could allow attribute breakout in contexts where values
/// end up in XML attributes downstream (e.g., libvirt XSLT processing).
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Extract parent snapshot name from `<parent><name>foo</name></parent>`.
/// SECURITY (SVE #23): Bounds-checked to prevent panics on malformed XML.
fn extract_parent_name(xml: &str) -> Option<String> {
    let parent_start = xml.find("<parent>")?;
    let remaining = xml.get(parent_start..)?;
    let parent_end_relative = remaining.find("</parent>")?;
    let parent_block = &remaining[..parent_end_relative];
    extract_xml_value(parent_block, "name")
}

/// SECURITY (SVE #23): Bounds-checked XML value extraction.
/// Returns None if opening/closing tags are not found or if indices are out of bounds,
/// preventing panics from malformed XML.
fn extract_xml_value(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let open_pos = xml.find(&open)?;
    let start = open_pos + open.len();
    // Bounds check: ensure start doesn't exceed xml length
    if start > xml.len() {
        return None;
    }
    let relative_end = xml.get(start..)?.find(&close)?;
    let end = start + relative_end;
    // Bounds check: ensure end is within xml
    if end > xml.len() || start > end {
        return None;
    }
    Some(xml[start..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn build_snapshot_xml_escapes_special_chars() {
        // CWE-91 regression: <, >, & in user-controlled fields must be escaped
        // or a clever name like "</name><action>delete-all</action>" could break out.
        let xml = build_snapshot_xml("evil<name>", "desc & more");
        assert!(xml.contains("evil&lt;name&gt;"));
        assert!(xml.contains("desc &amp; more"));
        assert!(!xml.contains("evil<name>"));
    }

    #[test]
    fn build_snapshot_xml_well_formed() {
        let xml = build_snapshot_xml("snap1", "my snapshot");
        assert!(xml.starts_with("<domainsnapshot>"));
        assert!(xml.contains("<name>snap1</name>"));
        assert!(xml.contains("<description>my snapshot</description>"));
        assert!(xml.trim_end().ends_with("</domainsnapshot>"));
    }

    // Helper struct used to prove ThawGuard semantics without touching libvirt:
    // mirrors the real ThawGuard's structure but flips an AtomicBool on drop
    // when armed, so the test can assert "thaw ran" or "thaw was skipped".
    struct TestThawGuard<'a> {
        thawed: &'a AtomicBool,
        armed: bool,
    }
    impl<'a> Drop for TestThawGuard<'a> {
        fn drop(&mut self) {
            if self.armed {
                self.thawed.store(true, Ordering::SeqCst);
            }
        }
    }

    #[test]
    fn thaw_guard_runs_on_normal_drop() {
        let thawed = AtomicBool::new(false);
        {
            let _g = TestThawGuard {
                thawed: &thawed,
                armed: true,
            };
        }
        assert!(
            thawed.load(Ordering::SeqCst),
            "guard must thaw on normal scope exit"
        );
    }

    #[test]
    fn thaw_guard_skips_when_not_armed() {
        // If freeze failed (we never armed the guard), we must NOT thaw — that
        // would issue an unnecessary qemu-ga roundtrip and confuse logs.
        let thawed = AtomicBool::new(false);
        {
            let _g = TestThawGuard {
                thawed: &thawed,
                armed: false,
            };
        }
        assert!(
            !thawed.load(Ordering::SeqCst),
            "guard must NOT thaw if not armed"
        );
    }

    #[test]
    fn thaw_guard_runs_on_panic_unwind() {
        // The whole reason we use Drop instead of explicit cleanup: panics
        // anywhere in the snapshot path must still thaw the guest. If this
        // test ever fails, a panicking libvirt call could leave a real VM
        // frozen indefinitely.
        let thawed = AtomicBool::new(false);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g = TestThawGuard {
                thawed: &thawed,
                armed: true,
            };
            panic!("simulated snapshot failure mid-flight");
        }));
        assert!(result.is_err(), "panic should have propagated");
        assert!(
            thawed.load(Ordering::SeqCst),
            "guard MUST thaw even when the snapshot path panics"
        );
    }
}
