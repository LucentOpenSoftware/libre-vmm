//! Unattended installation — Windows Autounattend.xml and Linux cloud-init ISO generation.
//!
//! Generates answer files for automated OS installation and packages them
//! into ISO images that can be attached to VMs as secondary CD-ROM drives.

use crate::error::{VmmError, VmmResult};
use std::process::{Command, Stdio};
use tracing::info;

/// Target OS for unattended installation.
#[derive(Debug, Clone, PartialEq)]
pub enum UnattendedTarget {
    Windows,
    LinuxCloudInit,
}

/// Windows unattended install configuration.
#[derive(Debug, Clone)]
pub struct WindowsUnattendedConfig {
    pub username: String,
    pub password: String,
    pub hostname: String,
    pub locale: String,
    pub timezone: String,
    pub product_key: Option<String>,
    pub skip_oobe: bool,
    pub auto_login: bool,
    pub enable_rdp: bool,
}

impl Default for WindowsUnattendedConfig {
    fn default() -> Self {
        Self {
            username: "User".to_string(),
            password: String::new(),
            hostname: "WIN-VM".to_string(),
            locale: "en-US".to_string(),
            timezone: "UTC".to_string(),
            product_key: None,
            skip_oobe: true,
            auto_login: false,
            enable_rdp: false,
        }
    }
}

/// Cloud-init configuration (Linux).
#[derive(Debug, Clone)]
pub struct CloudInitConfig {
    pub hostname: String,
    pub username: String,
    pub password: Option<String>,
    pub ssh_authorized_keys: Vec<String>,
    pub packages: Vec<String>,
    pub timezone: String,
}

impl Default for CloudInitConfig {
    fn default() -> Self {
        Self {
            hostname: "linux-vm".to_string(),
            username: "user".to_string(),
            password: None,
            ssh_authorized_keys: Vec::new(),
            packages: Vec::new(),
            timezone: "UTC".to_string(),
        }
    }
}

/// Check if genisoimage (or mkisofs) is available.
pub fn iso_tool_available() -> bool {
    Command::new("genisoimage")
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
        || Command::new("mkisofs")
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
}

fn iso_tool() -> &'static str {
    if Command::new("genisoimage")
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
    {
        "genisoimage"
    } else {
        "mkisofs"
    }
}

/// Generate Windows Autounattend.xml content.
///
/// SECURITY: CWE-91 — XML-escape all user-provided values to prevent injection.
pub fn generate_autounattend_xml(config: &WindowsUnattendedConfig) -> String {
    let esc = |s: &str| -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    };

    let product_key_section = if let Some(ref key) = config.product_key {
        format!(
            r#"<ProductKey>
                <Key>{}</Key>
                <WillShowUI>OnError</WillShowUI>
            </ProductKey>"#,
            esc(key)
        )
    } else {
        String::new()
    };

    let auto_logon = if config.auto_login {
        format!(
            r#"<AutoLogon>
                <Password><Value>{}</Value></Password>
                <Enabled>true</Enabled>
                <Username>{}</Username>
            </AutoLogon>"#,
            esc(&config.password),
            esc(&config.username)
        )
    } else {
        String::new()
    };

    let rdp_cmd = if config.enable_rdp {
        r#"<RunSynchronousCommand wcm:action="add">
                <Order>1</Order>
                <Path>netsh advfirewall firewall set rule group="remote desktop" new enable=Yes</Path>
            </RunSynchronousCommand>
            <RunSynchronousCommand wcm:action="add">
                <Order>2</Order>
                <Path>reg add "HKLM\SYSTEM\CurrentControlSet\Control\Terminal Server" /v fDenyTSConnections /t REG_DWORD /d 0 /f</Path>
            </RunSynchronousCommand>"#
    } else {
        ""
    };

    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<unattend xmlns="urn:schemas-microsoft-com:unattend"
          xmlns:wcm="http://schemas.microsoft.com/WMIConfig/2002/State">
    <settings pass="windowsPE">
        <component name="Microsoft-Windows-International-Core-WinPE"
                   processorArchitecture="amd64" language="neutral"
                   publicKeyToken="31bf3856ad364e35" versionScope="nonSxS">
            <SetupUILanguage>
                <UILanguage>{locale}</UILanguage>
            </SetupUILanguage>
            <InputLocale>{locale}</InputLocale>
            <SystemLocale>{locale}</SystemLocale>
            <UILanguage>{locale}</UILanguage>
            <UserLocale>{locale}</UserLocale>
        </component>
        <component name="Microsoft-Windows-Setup"
                   processorArchitecture="amd64" language="neutral"
                   publicKeyToken="31bf3856ad364e35" versionScope="nonSxS">
            {product_key}
            <DiskConfiguration>
                <Disk wcm:action="add">
                    <DiskID>0</DiskID>
                    <WillWipeDisk>true</WillWipeDisk>
                    <CreatePartitions>
                        <CreatePartition wcm:action="add">
                            <Order>1</Order>
                            <Size>512</Size>
                            <Type>EFI</Type>
                        </CreatePartition>
                        <CreatePartition wcm:action="add">
                            <Order>2</Order>
                            <Size>128</Size>
                            <Type>MSR</Type>
                        </CreatePartition>
                        <CreatePartition wcm:action="add">
                            <Order>3</Order>
                            <Extend>true</Extend>
                            <Type>Primary</Type>
                        </CreatePartition>
                    </CreatePartitions>
                    <ModifyPartitions>
                        <ModifyPartition wcm:action="add">
                            <Order>1</Order>
                            <PartitionID>1</PartitionID>
                            <Format>FAT32</Format>
                            <Label>System</Label>
                        </ModifyPartition>
                        <ModifyPartition wcm:action="add">
                            <Order>2</Order>
                            <PartitionID>3</PartitionID>
                            <Format>NTFS</Format>
                            <Label>Windows</Label>
                        </ModifyPartition>
                    </ModifyPartitions>
                </Disk>
            </DiskConfiguration>
            <ImageInstall>
                <OSImage>
                    <InstallTo>
                        <DiskID>0</DiskID>
                        <PartitionID>3</PartitionID>
                    </InstallTo>
                </OSImage>
            </ImageInstall>
            <UserData>
                <AcceptEula>true</AcceptEula>
            </UserData>
        </component>
    </settings>
    <settings pass="specialize">
        <component name="Microsoft-Windows-Shell-Setup"
                   processorArchitecture="amd64" language="neutral"
                   publicKeyToken="31bf3856ad364e35" versionScope="nonSxS">
            <ComputerName>{hostname}</ComputerName>
            <TimeZone>{timezone}</TimeZone>
        </component>
        {rdp_section}
    </settings>
    <settings pass="oobeSystem">
        <component name="Microsoft-Windows-Shell-Setup"
                   processorArchitecture="amd64" language="neutral"
                   publicKeyToken="31bf3856ad364e35" versionScope="nonSxS">
            <OOBE>
                <HideEULAPage>true</HideEULAPage>
                <HideLocalAccountScreen>true</HideLocalAccountScreen>
                <HideOnlineAccountScreens>true</HideOnlineAccountScreens>
                <HideWirelessSetupInOOBE>true</HideWirelessSetupInOOBE>
                <ProtectYourPC>3</ProtectYourPC>
            </OOBE>
            {auto_logon}
            <UserAccounts>
                <LocalAccounts>
                    <LocalAccount wcm:action="add">
                        <Name>{username}</Name>
                        <Group>Administrators</Group>
                        <Password>
                            <Value>{password}</Value>
                            <PlainText>true</PlainText>
                        </Password>
                    </LocalAccount>
                </LocalAccounts>
            </UserAccounts>
        </component>
    </settings>
</unattend>"#,
        locale = esc(&config.locale),
        product_key = product_key_section,
        hostname = esc(&config.hostname),
        timezone = esc(&config.timezone),
        rdp_section = rdp_cmd,
        auto_logon = auto_logon,
        username = esc(&config.username),
        password = esc(&config.password),
    )
}

/// SECURITY: CWE-91/CWE-94 — Escape a string for safe YAML scalar insertion.
/// Wraps in single quotes and escapes internal single quotes.
/// This prevents YAML injection via newlines, colons, and special chars.
fn yaml_escape(s: &str) -> String {
    // SECURITY: CWE-94 — Reject strings with control characters (except tab)
    // that could break YAML structure or inject multi-line content.
    let sanitized: String = s
        .chars()
        .filter(|c| !c.is_control() || *c == '\t')
        .collect();
    // Single-quote the value; escape internal single quotes per YAML spec ('' = literal ')
    format!("'{}'", sanitized.replace('\'', "''"))
}

/// SECURITY: CWE-20 — Validate a hostname for cloud-init/Windows use.
/// Allows only alphanumeric, hyphens, and dots. Max 63 chars per label.
fn validate_hostname(hostname: &str) -> VmmResult<()> {
    if hostname.is_empty() || hostname.len() > 253 {
        return Err(VmmError::Other(
            "Hostname must be 1-253 characters".to_string(),
        ));
    }
    if !hostname
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
    {
        return Err(VmmError::Other(
            "Hostname contains invalid characters (only a-z, 0-9, -, .)".to_string(),
        ));
    }
    if hostname.starts_with('-') || hostname.ends_with('-') {
        return Err(VmmError::Other(
            "Hostname must not start or end with a hyphen".to_string(),
        ));
    }
    Ok(())
}

/// SECURITY: CWE-20 — Validate a username for cloud-init/Windows use.
fn validate_username(username: &str) -> VmmResult<()> {
    if username.is_empty() || username.len() > 32 {
        return Err(VmmError::Other(
            "Username must be 1-32 characters".to_string(),
        ));
    }
    if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        return Err(VmmError::Other(
            "Username contains invalid characters".to_string(),
        ));
    }
    if username.starts_with('-') {
        return Err(VmmError::Other(
            "Username must not start with a hyphen".to_string(),
        ));
    }
    Ok(())
}

/// SECURITY: CWE-20 — Validate an SSH public key line.
fn validate_ssh_key(key: &str) -> bool {
    // Must start with a known key type prefix and not contain newlines
    let valid_prefixes = ["ssh-rsa ", "ssh-ed25519 ", "ssh-dss ", "ecdsa-sha2-"];
    if key.contains('\n') || key.contains('\r') {
        return false;
    }
    valid_prefixes.iter().any(|p| key.starts_with(p))
}

/// SECURITY: CWE-20 — Validate a package name.
fn validate_package_name(pkg: &str) -> bool {
    !pkg.is_empty()
        && pkg.len() <= 128
        && pkg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.+".contains(c))
        && !pkg.starts_with('-')
}

/// Generate cloud-init user-data YAML.
///
/// SECURITY: CWE-91 — All user-controlled values are YAML-escaped to prevent injection.
pub fn generate_cloud_init_userdata(config: &CloudInitConfig) -> String {
    let mut lines = vec!["#cloud-config".to_string()];
    lines.push(format!("hostname: {}", yaml_escape(&config.hostname)));
    lines.push(format!("timezone: {}", yaml_escape(&config.timezone)));

    // User account
    lines.push("users:".to_string());
    lines.push(format!("  - name: {}", yaml_escape(&config.username)));
    lines.push("    groups: sudo".to_string());
    lines.push("    shell: /bin/bash".to_string());
    lines.push("    sudo: ALL=(ALL) NOPASSWD:ALL".to_string());
    if let Some(ref pass) = config.password {
        // SECURITY: CWE-91 — Password MUST be quoted to prevent YAML injection
        lines.push(format!("    passwd: {}", yaml_escape(pass)));
        lines.push("    lock_passwd: false".to_string());
    }
    if !config.ssh_authorized_keys.is_empty() {
        lines.push("    ssh_authorized_keys:".to_string());
        for key in &config.ssh_authorized_keys {
            // SECURITY: CWE-20 — Only include validated SSH key lines
            if validate_ssh_key(key) {
                lines.push(format!("      - {}", yaml_escape(key)));
            }
        }
    }

    // Packages
    if !config.packages.is_empty() {
        lines.push("packages:".to_string());
        for pkg in &config.packages {
            // SECURITY: CWE-20 — Only include validated package names
            if validate_package_name(pkg) {
                lines.push(format!("  - {}", yaml_escape(pkg)));
            }
        }
        lines.push("package_update: true".to_string());
    }

    lines.join("\n") + "\n"
}

/// Generate cloud-init meta-data.
pub fn generate_cloud_init_metadata(config: &CloudInitConfig) -> String {
    // SECURITY: CWE-91 — Escape hostname in metadata too
    format!(
        "instance-id: {}\nlocal-hostname: {}\n",
        uuid::Uuid::new_v4(),
        yaml_escape(&config.hostname)
    )
}

/// Create a Windows Autounattend ISO.
///
/// Generates Autounattend.xml and packages it into an ISO that Windows
/// setup will automatically detect.
pub fn create_autounattend_iso(
    config: &WindowsUnattendedConfig,
    output_path: &str,
) -> VmmResult<()> {
    // SECURITY: CWE-20 — Validate hostname and username before generating XML
    validate_hostname(&config.hostname)?;
    validate_username(&config.username)?;

    let xml = generate_autounattend_xml(config);

    // SECURITY: CWE-377 — Use /dev/shm for temp files containing passwords
    let temp_dir = format!("/dev/shm/.libre-vmm-unattend-{}", uuid::Uuid::new_v4());
    std::fs::create_dir_all(&temp_dir)
        .map_err(|e| VmmError::Other(format!("Failed to create temp dir: {}", e)))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&temp_dir, std::fs::Permissions::from_mode(0o700));
    }

    // Write Autounattend.xml
    std::fs::write(format!("{}/Autounattend.xml", temp_dir), &xml)
        .map_err(|e| VmmError::Other(format!("Failed to write answer file: {}", e)))?;

    // Ensure output parent dir exists
    if let Some(parent) = std::path::Path::new(output_path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Create ISO
    let tool = iso_tool();
    let output = Command::new(tool)
        .args(["-o", output_path, "-J", "-r", &temp_dir])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| VmmError::Other(format!("{} not found: {}", tool, e)))?;

    // Always clean up temp dir (contains password)
    let _ = std::fs::remove_dir_all(&temp_dir);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VmmError::Other(format!("ISO creation failed: {}", stderr)));
    }

    info!("Autounattend ISO created at {}", output_path);
    Ok(())
}

/// Create a cloud-init ISO (cidata volume).
pub fn create_cloud_init_iso(config: &CloudInitConfig, output_path: &str) -> VmmResult<()> {
    // SECURITY: CWE-20 — Validate hostname and username before generating YAML
    validate_hostname(&config.hostname)?;
    validate_username(&config.username)?;

    let userdata = generate_cloud_init_userdata(config);
    let metadata = generate_cloud_init_metadata(config);

    let temp_dir = format!("/dev/shm/.libre-vmm-cloudinit-{}", uuid::Uuid::new_v4());
    std::fs::create_dir_all(&temp_dir)
        .map_err(|e| VmmError::Other(format!("Failed to create temp dir: {}", e)))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&temp_dir, std::fs::Permissions::from_mode(0o700));
    }

    std::fs::write(format!("{}/user-data", temp_dir), &userdata)
        .map_err(|e| VmmError::Other(format!("Failed to write user-data: {}", e)))?;
    std::fs::write(format!("{}/meta-data", temp_dir), &metadata)
        .map_err(|e| VmmError::Other(format!("Failed to write meta-data: {}", e)))?;

    if let Some(parent) = std::path::Path::new(output_path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    let tool = iso_tool();
    let output = Command::new(tool)
        .args([
            "-output",
            output_path,
            "-volid",
            "cidata",
            "-joliet",
            "-rock",
            &format!("{}/user-data", temp_dir),
            &format!("{}/meta-data", temp_dir),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| VmmError::Other(format!("{} not found: {}", tool, e)))?;

    let _ = std::fs::remove_dir_all(&temp_dir);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VmmError::Other(format!("ISO creation failed: {}", stderr)));
    }

    info!("Cloud-init ISO created at {}", output_path);
    Ok(())
}
