//! Virtual Network Editor — CRUD for libvirt virtual networks.
//!
//! Create, modify, and delete virtual networks with NAT, bridged,
//! or host-only modes, DHCP ranges, and DNS configuration.

use crate::error::{VmmError, VmmResult};
use tracing::info;
use virt::connect::Connect;
use virt::network::Network;

/// Network mode for virtual network creation.
#[derive(Debug, Clone, PartialEq)]
pub enum NetworkMode {
    Nat,
    Bridged,
    Isolated,
}

impl std::fmt::Display for NetworkMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Nat => write!(f, "NAT"),
            Self::Bridged => write!(f, "Bridged"),
            Self::Isolated => write!(f, "Isolated (Host-Only)"),
        }
    }
}

/// Configuration for creating a virtual network.
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    pub name: String,
    pub mode: NetworkMode,
    pub bridge_name: String,
    pub subnet: String,     // e.g., "192.168.100"
    pub netmask: String,    // e.g., "255.255.255.0"
    pub dhcp_start: String, // e.g., "192.168.100.100"
    pub dhcp_end: String,   // e.g., "192.168.100.200"
    pub dns_enabled: bool,
    pub autostart: bool,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            mode: NetworkMode::Nat,
            bridge_name: String::new(),
            subnet: "192.168.100".to_string(),
            netmask: "255.255.255.0".to_string(),
            dhcp_start: "192.168.100.100".to_string(),
            dhcp_end: "192.168.100.200".to_string(),
            dns_enabled: true,
            autostart: true,
        }
    }
}

/// Detailed info about a network (extends NetworkInfo).
#[derive(Debug, Clone)]
pub struct NetworkDetail {
    pub name: String,
    pub active: bool,
    pub autostart: bool,
    pub bridge: String,
    pub xml: String,
}

/// Get detailed info for a network, including its XML.
pub fn get_network_detail(conn: &Connect, name: &str) -> VmmResult<NetworkDetail> {
    validate_network_name(name)?;
    let net = Network::lookup_by_name(conn, name)
        .map_err(|e| VmmError::NetworkError(format!("Network '{}' not found: {}", name, e)))?;

    let xml = net
        .get_xml_desc(0)
        .map_err(|e| VmmError::NetworkError(format!("Failed to get XML: {}", e)))?;

    Ok(NetworkDetail {
        name: net.get_name().unwrap_or_default(),
        active: net.is_active().unwrap_or(false),
        autostart: net.get_autostart().unwrap_or(false),
        bridge: net.get_bridge_name().unwrap_or_default(),
        xml,
    })
}

/// Create a new virtual network from configuration.
/// SECURITY: CWE-91 — All user values are XML-escaped before insertion.
pub fn create_network(conn: &Connect, config: &NetworkConfig) -> VmmResult<()> {
    validate_network_name(&config.name)?;
    validate_ip(&config.dhcp_start)?;
    validate_ip(&config.dhcp_end)?;
    validate_ip(&config.netmask)?;
    // SECURITY (SVE-L, CWE-20): Validate subnet prefix — it is used to build the
    // gateway IP and inserted into libvirt XML. Without validation, an attacker
    // could inject XML metacharacters via the subnet field.
    validate_subnet_prefix(&config.subnet)?;

    let xml = build_network_xml(config);

    Network::define_xml(conn, &xml)
        .map_err(|e| VmmError::NetworkError(format!("Failed to define network: {}", e)))?;

    // Auto-start if requested
    if let Ok(net) = Network::lookup_by_name(conn, &config.name) {
        if config.autostart {
            let _ = net.set_autostart(true);
        }
        let _ = net.create();
    }

    info!("Created virtual network '{}'", config.name);
    Ok(())
}

/// Delete a virtual network.
pub fn delete_network(conn: &Connect, name: &str) -> VmmResult<()> {
    validate_network_name(name)?;
    let net = Network::lookup_by_name(conn, name)
        .map_err(|e| VmmError::NetworkError(format!("Network '{}' not found: {}", name, e)))?;

    // Stop if active
    if net.is_active().unwrap_or(false) {
        net.destroy()
            .map_err(|e| VmmError::NetworkError(format!("Failed to stop network: {}", e)))?;
    }

    net.undefine()
        .map_err(|e| VmmError::NetworkError(format!("Failed to undefine network: {}", e)))?;

    info!("Deleted virtual network '{}'", name);
    Ok(())
}

/// Start a network.
pub fn start_network(conn: &Connect, name: &str) -> VmmResult<()> {
    validate_network_name(name)?;
    let net = Network::lookup_by_name(conn, name)
        .map_err(|e| VmmError::NetworkError(format!("Network '{}' not found: {}", name, e)))?;
    net.create()
        .map_err(|e| VmmError::NetworkError(format!("Failed to start network: {}", e)))?;
    Ok(())
}

/// Stop a network.
pub fn stop_network(conn: &Connect, name: &str) -> VmmResult<()> {
    validate_network_name(name)?;
    let net = Network::lookup_by_name(conn, name)
        .map_err(|e| VmmError::NetworkError(format!("Network '{}' not found: {}", name, e)))?;
    net.destroy()
        .map_err(|e| VmmError::NetworkError(format!("Failed to stop network: {}", e)))?;
    Ok(())
}

// ===== XML Generation =====

fn build_network_xml(config: &NetworkConfig) -> String {
    let name = xml_escape(&config.name);
    // SECURITY (SVE-J, CWE-91): XML-escape the fallback bridge name derived from
    // config.name. Without this, a crafted network name containing XML metacharacters
    // could inject arbitrary XML into the bridge name attribute.
    let bridge = if config.bridge_name.is_empty() {
        format!("virbr-{}", xml_escape(&config.name))
    } else {
        xml_escape(&config.bridge_name)
    };

    let forward = match config.mode {
        NetworkMode::Nat => "<forward mode='nat'/>",
        NetworkMode::Bridged => "<forward mode='bridge'/>",
        NetworkMode::Isolated => "", // No forward = isolated
    };

    // SECURITY (SVE-L, CWE-91): XML-escape the gateway address derived from
    // config.subnet. Without this, a crafted subnet string could inject XML
    // into the <ip address='...'> attribute.
    let gateway = xml_escape(&format!("{}.1", config.subnet));
    let dhcp_start = xml_escape(&config.dhcp_start);
    let dhcp_end = xml_escape(&config.dhcp_end);
    let netmask = xml_escape(&config.netmask);

    let dns = if config.dns_enabled {
        "  <dns enable='yes'/>\n"
    } else {
        "  <dns enable='no'/>\n"
    };

    format!(
        r#"<network>
  <name>{name}</name>
  <bridge name='{bridge}' stp='on' delay='0'/>
  {forward}
{dns}  <ip address='{gateway}' netmask='{netmask}'>
    <dhcp>
      <range start='{dhcp_start}' end='{dhcp_end}'/>
    </dhcp>
  </ip>
</network>"#
    )
}

// ===== Validation =====

/// SECURITY: CWE-91 — Validate network name to prevent XML injection.
fn validate_network_name(name: &str) -> VmmResult<()> {
    if name.is_empty() || name.len() > 64 {
        return Err(VmmError::NetworkError(
            "Network name must be 1-64 characters".into(),
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Err(VmmError::NetworkError(
            "Network name may only contain alphanumeric, '-', '_'".into(),
        ));
    }
    if name.starts_with('-') {
        return Err(VmmError::NetworkError(
            "Network name must not start with '-'".into(),
        ));
    }
    Ok(())
}

/// SECURITY (SVE-L, CWE-20): Validate a subnet prefix like "192.168.100".
/// Must be exactly 3 valid octets separated by dots.
fn validate_subnet_prefix(subnet: &str) -> VmmResult<()> {
    if subnet.is_empty() {
        return Err(VmmError::NetworkError(
            "Subnet prefix cannot be empty".into(),
        ));
    }
    let parts: Vec<&str> = subnet.split('.').collect();
    if parts.len() != 3 {
        return Err(VmmError::NetworkError(format!(
            "Invalid subnet prefix '{}': expected 3 octets (e.g. '192.168.100')",
            subnet
        )));
    }
    for part in &parts {
        if part.parse::<u8>().is_err() {
            return Err(VmmError::NetworkError(format!(
                "Invalid subnet prefix '{}': each octet must be 0-255",
                subnet
            )));
        }
    }
    Ok(())
}

/// Validate an IPv4 address format.
fn validate_ip(ip: &str) -> VmmResult<()> {
    if ip.is_empty() {
        return Ok(()); // empty is allowed for some fields
    }
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() != 4 {
        return Err(VmmError::NetworkError(format!(
            "Invalid IP address: {}",
            ip
        )));
    }
    for part in &parts {
        if part.parse::<u8>().is_err() {
            return Err(VmmError::NetworkError(format!(
                "Invalid IP address: {}",
                ip
            )));
        }
    }
    Ok(())
}

/// SECURITY: CWE-91 — Escape XML special characters.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\'', "&apos;")
        .replace('"', "&quot;")
}
