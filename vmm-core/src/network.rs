//! Network management — listing and creating virtual networks.
//!
//! SECURITY AUDIT NOTE: This module is read-only — it queries libvirt for existing
//! network state and starts a hardcoded ("default") network. It does NOT:
//! - Generate network XML from user input (no XML injection surface)
//! - Execute shell commands (no command injection surface)
//! - Accept user-supplied bridge names, MAC addresses, or IP addresses
//! Network XML generation with user parameters is in `xml_builder.rs::build_nic_xml`
//! which validates MAC addresses and allowlists NIC models.

use crate::error::{VmmError, VmmResult};
use virt::connect::Connect;
use virt::network::Network;

/// Info about a virtual network.
#[derive(Debug, Clone)]
pub struct NetworkInfo {
    pub name: String,
    pub active: bool,
    pub autostart: bool,
    pub bridge: String,
}

/// List all virtual networks.
pub fn list_networks(conn: &Connect) -> VmmResult<Vec<NetworkInfo>> {
    let networks = conn
        .list_all_networks(0)
        .map_err(|e| VmmError::NetworkError(format!("Failed to list networks: {}", e)))?;

    let mut result = Vec::new();
    for net in networks {
        let name = net.get_name().unwrap_or_default();
        let active = net.is_active().unwrap_or(false);
        let autostart = net.get_autostart().unwrap_or(false);
        let bridge = net.get_bridge_name().unwrap_or_default();

        result.push(NetworkInfo {
            name,
            active,
            autostart,
            bridge,
        });
    }

    Ok(result)
}

/// Ensure the default NAT network exists and is running.
pub fn ensure_default_network(conn: &Connect) -> VmmResult<()> {
    match Network::lookup_by_name(conn, "default") {
        Ok(net) => {
            if !net.is_active().unwrap_or(false) {
                net.create().map_err(|e| {
                    VmmError::NetworkError(format!("Failed to start default network: {}", e))
                })?;
            }
            Ok(())
        },
        Err(_) => Err(VmmError::NetworkError(
            "Default network not found. Run: sudo virsh net-start default".into(),
        )),
    }
}
