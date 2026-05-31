//! ISO auto-detection — detects the operating system from an ISO filename
//! and optionally its volume label via `isoinfo`.

use std::path::Path;
use std::process::Command;
use tracing::{debug, info, warn};

/// Result of ISO auto-detection.
#[derive(Debug, Clone)]
pub struct DetectedOs {
    /// The detected OS name (e.g., "Ubuntu 24.04")
    pub name: String,
    /// Suggested template name to match against builtin_templates()
    pub template_hint: String,
    /// Confidence level
    pub confidence: DetectionConfidence,
}

/// Confidence level for an ISO detection result.
#[derive(Debug, Clone, PartialEq)]
pub enum DetectionConfidence {
    /// Filename strongly matches a known pattern
    High,
    /// Partial match or volume label match
    Medium,
    /// Generic guess
    Low,
}

/// Validate that an ISO path is safe to use.
/// Rejects empty paths, paths containing null bytes, and path traversal (`..`).
fn validate_path(path: &str) -> bool {
    if path.is_empty() {
        warn!("ISO path is empty");
        return false;
    }
    if path.contains('\0') {
        warn!("ISO path contains null bytes");
        return false;
    }
    // Check for path traversal components
    let p = Path::new(path);
    for component in p.components() {
        if let std::path::Component::ParentDir = component {
            warn!("ISO path contains '..' traversal component");
            return false;
        }
    }
    true
}

/// Extract the filename stem (without extension) from a path, lowercased.
fn filename_stem_lower(path: &str) -> Option<String> {
    let p = Path::new(path);
    let stem = p.file_stem()?.to_str()?;
    Some(stem.to_lowercase())
}

/// Extract a version number that follows a prefix in the stem.
/// Looks for a pattern like `prefix` followed by digits (and optionally a dot and more digits).
fn extract_version_after(stem: &str, prefix: &str) -> Option<String> {
    let idx = stem.find(prefix)?;
    let after = &stem[idx + prefix.len()..];
    // Collect digits and at most one dot
    let mut version = String::new();
    let mut saw_dot = false;
    for ch in after.chars() {
        if ch.is_ascii_digit() {
            version.push(ch);
        } else if ch == '.' && !saw_dot {
            saw_dot = true;
            version.push(ch);
        } else {
            break;
        }
    }
    // Trim trailing dot
    if version.ends_with('.') {
        version.pop();
    }
    if version.is_empty() {
        None
    } else {
        Some(version)
    }
}

/// Try to detect the OS from the ISO filename alone (no I/O).
fn detect_from_filename(path: &str) -> Option<DetectedOs> {
    let stem = filename_stem_lower(path)?;
    debug!("Filename stem for detection: {}", stem);

    // Ubuntu: ubuntu-24.04...
    if stem.starts_with("ubuntu-") || stem.starts_with("ubuntu_") {
        let ver = extract_version_after(&stem, "ubuntu-")
            .or_else(|| extract_version_after(&stem, "ubuntu_"));
        let name = match &ver {
            Some(v) => format!("Ubuntu {}", v),
            None => "Ubuntu".to_string(),
        };
        return Some(DetectedOs {
            name,
            template_hint: "Ubuntu".to_string(),
            confidence: DetectionConfidence::High,
        });
    }

    // Linux Mint: linuxmint-22...
    if stem.starts_with("linuxmint-") || stem.starts_with("linuxmint_") {
        let ver = extract_version_after(&stem, "linuxmint-")
            .or_else(|| extract_version_after(&stem, "linuxmint_"));
        let name = match &ver {
            Some(v) => format!("Linux Mint {}", v),
            None => "Linux Mint".to_string(),
        };
        return Some(DetectedOs {
            name,
            template_hint: "Linux Mint".to_string(),
            confidence: DetectionConfidence::High,
        });
    }

    // Debian: debian-12...
    if stem.starts_with("debian-") || stem.starts_with("debian_") {
        let ver = extract_version_after(&stem, "debian-")
            .or_else(|| extract_version_after(&stem, "debian_"));
        let name = match &ver {
            Some(v) => format!("Debian {}", v),
            None => "Debian".to_string(),
        };
        return Some(DetectedOs {
            name,
            template_hint: "Debian".to_string(),
            confidence: DetectionConfidence::High,
        });
    }

    // Fedora: fedora-...-40...
    if stem.starts_with("fedora-") || stem.starts_with("fedora_") {
        let ver = extract_first_version_segment(&stem, "fedora");
        let name = match &ver {
            Some(v) => format!("Fedora {}", v),
            None => "Fedora".to_string(),
        };
        return Some(DetectedOs {
            name,
            template_hint: "Fedora".to_string(),
            confidence: DetectionConfidence::High,
        });
    }

    // Arch Linux: archlinux-...
    if stem.starts_with("archlinux-") || stem.starts_with("archlinux_") {
        return Some(DetectedOs {
            name: "Arch Linux".to_string(),
            template_hint: "Arch Linux".to_string(),
            confidence: DetectionConfidence::High,
        });
    }

    // Manjaro: manjaro-...
    if stem.starts_with("manjaro-") || stem.starts_with("manjaro_") {
        return Some(DetectedOs {
            name: "Manjaro".to_string(),
            template_hint: "Manjaro".to_string(),
            confidence: DetectionConfidence::High,
        });
    }

    // EndeavourOS: endeavouros-...
    if stem.starts_with("endeavouros-") || stem.starts_with("endeavouros_") {
        return Some(DetectedOs {
            name: "EndeavourOS".to_string(),
            template_hint: "EndeavourOS".to_string(),
            confidence: DetectionConfidence::High,
        });
    }

    // CentOS: centos-stream-9...
    if stem.starts_with("centos-") || stem.starts_with("centos_") {
        let ver = extract_first_version_segment(&stem, "centos");
        let name = match &ver {
            Some(v) => format!("CentOS {}", v),
            None => "CentOS".to_string(),
        };
        return Some(DetectedOs {
            name,
            template_hint: "CentOS".to_string(),
            confidence: DetectionConfidence::High,
        });
    }

    // Rocky Linux: rocky-9.3-x86_64...
    if stem.starts_with("rocky-") || stem.starts_with("rocky_") {
        let ver = extract_first_version_segment(&stem, "rocky");
        let name = match &ver {
            Some(v) => format!("Rocky Linux {}", v),
            None => "Rocky Linux".to_string(),
        };
        return Some(DetectedOs {
            name,
            template_hint: "Rocky Linux".to_string(),
            confidence: DetectionConfidence::High,
        });
    }

    // AlmaLinux: almalinux-9.3-x86_64...
    if stem.starts_with("almalinux-") || stem.starts_with("almalinux_") {
        let ver = extract_first_version_segment(&stem, "almalinux");
        let name = match &ver {
            Some(v) => format!("AlmaLinux {}", v),
            None => "AlmaLinux".to_string(),
        };
        return Some(DetectedOs {
            name,
            template_hint: "AlmaLinux".to_string(),
            confidence: DetectionConfidence::High,
        });
    }

    // openSUSE: opensuse-...
    if stem.starts_with("opensuse-") || stem.starts_with("opensuse_") || stem.contains("opensuse") {
        return Some(DetectedOs {
            name: "openSUSE".to_string(),
            template_hint: "openSUSE".to_string(),
            confidence: DetectionConfidence::High,
        });
    }

    // NixOS: nixos-...
    if stem.starts_with("nixos-") || stem.starts_with("nixos_") {
        return Some(DetectedOs {
            name: "NixOS".to_string(),
            template_hint: "NixOS".to_string(),
            confidence: DetectionConfidence::High,
        });
    }

    // Pop!_OS: pop_os- or pop-os-
    if stem.starts_with("pop_os-")
        || stem.starts_with("pop-os-")
        || stem.starts_with("pop_os_")
        || stem.starts_with("pop-os_")
    {
        return Some(DetectedOs {
            name: "Pop!_OS".to_string(),
            template_hint: "Pop!_OS".to_string(),
            confidence: DetectionConfidence::High,
        });
    }

    // Windows 10: win10 or windows10
    if stem.contains("win10") || stem.contains("windows10") || stem.contains("windows_10") {
        return Some(DetectedOs {
            name: "Windows 10".to_string(),
            template_hint: "Windows 10".to_string(),
            confidence: DetectionConfidence::High,
        });
    }

    // Windows 11: win11 or windows11
    if stem.contains("win11") || stem.contains("windows11") || stem.contains("windows_11") {
        return Some(DetectedOs {
            name: "Windows 11".to_string(),
            template_hint: "Windows 11".to_string(),
            confidence: DetectionConfidence::High,
        });
    }

    // Windows Server: winserver or windowsserver
    if stem.contains("winserver")
        || stem.contains("windowsserver")
        || stem.contains("windows_server")
    {
        return Some(DetectedOs {
            name: "Windows Server".to_string(),
            template_hint: "Windows Server".to_string(),
            confidence: DetectionConfidence::High,
        });
    }

    // FreeBSD: freebsd-13...
    if stem.starts_with("freebsd-") || stem.starts_with("freebsd_") {
        let ver = extract_version_after(&stem, "freebsd-")
            .or_else(|| extract_version_after(&stem, "freebsd_"));
        let name = match &ver {
            Some(v) => format!("FreeBSD {}", v),
            None => "FreeBSD".to_string(),
        };
        return Some(DetectedOs {
            name,
            template_hint: "FreeBSD".to_string(),
            confidence: DetectionConfidence::High,
        });
    }

    debug!("No filename pattern matched for stem: {}", stem);
    None
}

/// Extract the first version-like numeric segment from a stem, skipping the
/// distro prefix and any non-numeric middle segments.
/// e.g. "fedora-workstation-live-x86_64-40" with prefix "fedora" -> "40"
///      "rocky-9.3-x86_64-dvd" with prefix "rocky" -> "9.3"
///      "centos-stream-9-latest-x86_64-dvd1" with prefix "centos" -> "9"
fn extract_first_version_segment(stem: &str, prefix: &str) -> Option<String> {
    // Skip past the prefix
    let after = &stem[prefix.len()..];
    // Split on '-' and '_', find the first segment that starts with a digit
    // but skip segments that look like architecture (x86_64, amd64, etc.)
    let parts: Vec<&str> = after.split(|c: char| c == '-' || c == '_').collect();
    for part in &parts {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Skip known non-version segments that start with digits
        let lower = trimmed.to_lowercase();
        if lower == "x86"
            || lower == "64"
            || lower == "x86_64"
            || lower == "amd64"
            || lower == "arm64"
            || lower == "i386"
            || lower == "i686"
        {
            continue;
        }
        if trimmed.chars().next().map_or(false, |c| c.is_ascii_digit()) {
            let mut version = String::new();
            let mut saw_dot = false;
            for ch in trimmed.chars() {
                if ch.is_ascii_digit() {
                    version.push(ch);
                } else if ch == '.' && !saw_dot {
                    saw_dot = true;
                    version.push(ch);
                } else {
                    break;
                }
            }
            if version.ends_with('.') {
                version.pop();
            }
            if !version.is_empty() {
                return Some(version);
            }
        }
    }
    None
}

/// Try to detect the OS from the ISO volume label using `isoinfo`.
fn detect_from_volume_label(path: &str) -> Option<DetectedOs> {
    debug!(
        "Attempting volume label detection via isoinfo for: {}",
        path
    );

    let output = Command::new("isoinfo")
        .args(["-d", "-i", "--", path])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output();

    let output = match output {
        Ok(o) => o,
        Err(e) => {
            debug!("isoinfo not available or failed to run: {}", e);
            return None;
        },
    };

    if !output.status.success() {
        debug!("isoinfo exited with non-zero status");
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Find the "Volume id:" line
    let volume_id = stdout.lines().find(|line| line.starts_with("Volume id:"))?;

    let label = volume_id.strip_prefix("Volume id:")?.trim();

    if label.is_empty() {
        debug!("Volume label is empty");
        return None;
    }

    let label_lower = label.to_lowercase();
    info!("ISO volume label: {}", label);

    // Try to match volume label patterns
    if label_lower.contains("ubuntu") {
        let ver = extract_version_from_label(&label_lower, "ubuntu");
        let name = match &ver {
            Some(v) => format!("Ubuntu {}", v),
            None => "Ubuntu".to_string(),
        };
        return Some(DetectedOs {
            name,
            template_hint: "Ubuntu".to_string(),
            confidence: DetectionConfidence::Medium,
        });
    }

    if label_lower.contains("linux mint") || label_lower.contains("linuxmint") {
        let name = if label_lower.contains("linux mint") {
            extract_version_from_label(&label_lower, "linux mint")
                .map(|v| format!("Linux Mint {}", v))
                .unwrap_or_else(|| "Linux Mint".to_string())
        } else {
            extract_version_from_label(&label_lower, "linuxmint")
                .map(|v| format!("Linux Mint {}", v))
                .unwrap_or_else(|| "Linux Mint".to_string())
        };
        return Some(DetectedOs {
            name,
            template_hint: "Linux Mint".to_string(),
            confidence: DetectionConfidence::Medium,
        });
    }

    if label_lower.contains("debian") {
        let ver = extract_version_from_label(&label_lower, "debian");
        let name = match &ver {
            Some(v) => format!("Debian {}", v),
            None => "Debian".to_string(),
        };
        return Some(DetectedOs {
            name,
            template_hint: "Debian".to_string(),
            confidence: DetectionConfidence::Medium,
        });
    }

    if label_lower.contains("fedora") {
        let ver = extract_version_from_label(&label_lower, "fedora");
        let name = match &ver {
            Some(v) => format!("Fedora {}", v),
            None => "Fedora".to_string(),
        };
        return Some(DetectedOs {
            name,
            template_hint: "Fedora".to_string(),
            confidence: DetectionConfidence::Medium,
        });
    }

    if label_lower.contains("arch") && label_lower.contains("linux") {
        return Some(DetectedOs {
            name: "Arch Linux".to_string(),
            template_hint: "Arch Linux".to_string(),
            confidence: DetectionConfidence::Medium,
        });
    }

    if label_lower.contains("centos") {
        let ver = extract_version_from_label(&label_lower, "centos");
        let name = match &ver {
            Some(v) => format!("CentOS {}", v),
            None => "CentOS".to_string(),
        };
        return Some(DetectedOs {
            name,
            template_hint: "CentOS".to_string(),
            confidence: DetectionConfidence::Medium,
        });
    }

    if label_lower.contains("rocky") {
        let ver = extract_version_from_label(&label_lower, "rocky");
        let name = match &ver {
            Some(v) => format!("Rocky Linux {}", v),
            None => "Rocky Linux".to_string(),
        };
        return Some(DetectedOs {
            name,
            template_hint: "Rocky Linux".to_string(),
            confidence: DetectionConfidence::Medium,
        });
    }

    if label_lower.contains("almalinux") {
        let ver = extract_version_from_label(&label_lower, "almalinux");
        let name = match &ver {
            Some(v) => format!("AlmaLinux {}", v),
            None => "AlmaLinux".to_string(),
        };
        return Some(DetectedOs {
            name,
            template_hint: "AlmaLinux".to_string(),
            confidence: DetectionConfidence::Medium,
        });
    }

    if label_lower.contains("opensuse") {
        return Some(DetectedOs {
            name: "openSUSE".to_string(),
            template_hint: "openSUSE".to_string(),
            confidence: DetectionConfidence::Medium,
        });
    }

    if label_lower.contains("freebsd") {
        let ver = extract_version_from_label(&label_lower, "freebsd");
        let name = match &ver {
            Some(v) => format!("FreeBSD {}", v),
            None => "FreeBSD".to_string(),
        };
        return Some(DetectedOs {
            name,
            template_hint: "FreeBSD".to_string(),
            confidence: DetectionConfidence::Medium,
        });
    }

    // Windows labels tend to vary widely; check common patterns
    if label_lower.contains("windows") || label_lower.starts_with("win") {
        if label_lower.contains("11") {
            return Some(DetectedOs {
                name: "Windows 11".to_string(),
                template_hint: "Windows 11".to_string(),
                confidence: DetectionConfidence::Medium,
            });
        }
        if label_lower.contains("10") {
            return Some(DetectedOs {
                name: "Windows 10".to_string(),
                template_hint: "Windows 10".to_string(),
                confidence: DetectionConfidence::Medium,
            });
        }
        if label_lower.contains("server") {
            return Some(DetectedOs {
                name: "Windows Server".to_string(),
                template_hint: "Windows Server".to_string(),
                confidence: DetectionConfidence::Medium,
            });
        }
        return Some(DetectedOs {
            name: "Windows".to_string(),
            template_hint: "Windows 10".to_string(),
            confidence: DetectionConfidence::Low,
        });
    }

    debug!("Volume label '{}' did not match any known patterns", label);
    None
}

/// Extract a version number that appears after a keyword in a volume label.
/// e.g. "Ubuntu 24.04.1 LTS" with keyword "ubuntu" -> Some("24.04")
fn extract_version_from_label(label: &str, keyword: &str) -> Option<String> {
    let idx = label.find(keyword)?;
    let after = &label[idx + keyword.len()..];
    // Skip non-digit characters (spaces, dashes, etc.)
    let trimmed = after.trim_start_matches(|c: char| !c.is_ascii_digit());
    if trimmed.is_empty() {
        return None;
    }
    let mut version = String::new();
    let mut saw_dot = false;
    for ch in trimmed.chars() {
        if ch.is_ascii_digit() {
            version.push(ch);
        } else if ch == '.' && !saw_dot {
            saw_dot = true;
            version.push(ch);
        } else {
            break;
        }
    }
    if version.ends_with('.') {
        version.pop();
    }
    if version.is_empty() {
        None
    } else {
        Some(version)
    }
}

/// Detect the operating system from an ISO file path.
///
/// 1. First tries filename pattern matching (fast, no I/O)
/// 2. Falls back to isoinfo volume label parsing (slower, needs isoinfo installed)
///
/// Returns `None` if the OS could not be determined.
pub fn detect_os_from_iso(path: &str) -> Option<DetectedOs> {
    if !validate_path(path) {
        return None;
    }

    // Step 1: Try filename-based detection (fast, no I/O)
    if let Some(detected) = detect_from_filename(path) {
        info!(
            "Detected OS from filename: {} (confidence: {:?})",
            detected.name, detected.confidence
        );
        return Some(detected);
    }

    // Step 2: Fall back to volume label detection via isoinfo
    if let Some(detected) = detect_from_volume_label(path) {
        info!(
            "Detected OS from volume label: {} (confidence: {:?})",
            detected.name, detected.confidence
        );
        return Some(detected);
    }

    warn!("Could not detect OS from ISO: {}", path);
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ubuntu() {
        let result = detect_from_filename("/isos/ubuntu-24.04-desktop-amd64.iso").unwrap();
        assert_eq!(result.name, "Ubuntu 24.04");
        assert_eq!(result.template_hint, "Ubuntu");
        assert_eq!(result.confidence, DetectionConfidence::High);
    }

    #[test]
    fn test_linux_mint() {
        let result = detect_from_filename("/isos/linuxmint-22-cinnamon-64bit.iso").unwrap();
        assert_eq!(result.name, "Linux Mint 22");
        assert_eq!(result.template_hint, "Linux Mint");
    }

    #[test]
    fn test_debian() {
        let result = detect_from_filename("/isos/debian-12.5.0-amd64-netinst.iso").unwrap();
        assert_eq!(result.name, "Debian 12.5");
        assert_eq!(result.template_hint, "Debian");
    }

    #[test]
    fn test_fedora() {
        let result = detect_from_filename("/isos/Fedora-Workstation-Live-x86_64-40.iso").unwrap();
        assert_eq!(result.name, "Fedora 40");
        assert_eq!(result.template_hint, "Fedora");
    }

    #[test]
    fn test_arch() {
        let result = detect_from_filename("/isos/archlinux-2024.06.01-x86_64.iso").unwrap();
        assert_eq!(result.name, "Arch Linux");
        assert_eq!(result.template_hint, "Arch Linux");
    }

    #[test]
    fn test_manjaro() {
        let result = detect_from_filename("/isos/manjaro-kde-24.0-x86_64.iso").unwrap();
        assert_eq!(result.name, "Manjaro");
        assert_eq!(result.template_hint, "Manjaro");
    }

    #[test]
    fn test_windows_10() {
        let result = detect_from_filename("/isos/Win10_22H2_English_x64.iso").unwrap();
        assert_eq!(result.name, "Windows 10");
        assert_eq!(result.template_hint, "Windows 10");
    }

    #[test]
    fn test_windows_11() {
        let result = detect_from_filename("/isos/Win11_23H2_English_x64v2.iso").unwrap();
        assert_eq!(result.name, "Windows 11");
        assert_eq!(result.template_hint, "Windows 11");
    }

    #[test]
    fn test_freebsd() {
        let result = detect_from_filename("/isos/FreeBSD-14.0-RELEASE-amd64-disc1.iso").unwrap();
        assert_eq!(result.name, "FreeBSD 14.0");
        assert_eq!(result.template_hint, "FreeBSD");
    }

    #[test]
    fn test_pop_os() {
        let result = detect_from_filename("/isos/pop-os_22.04_amd64_intel_38.iso").unwrap();
        assert_eq!(result.name, "Pop!_OS");
        assert_eq!(result.template_hint, "Pop!_OS");

        // Also test the underscore variant
        let result2 = detect_from_filename("/isos/Pop_OS-22.04.iso").unwrap();
        assert_eq!(result2.name, "Pop!_OS");
    }

    #[test]
    fn test_rocky() {
        let result = detect_from_filename("/isos/Rocky-9.3-x86_64-dvd.iso").unwrap();
        assert_eq!(result.name, "Rocky Linux 9.3");
        assert_eq!(result.template_hint, "Rocky Linux");
    }

    #[test]
    fn test_almalinux() {
        let result = detect_from_filename("/isos/AlmaLinux-9.3-x86_64-dvd.iso").unwrap();
        assert_eq!(result.name, "AlmaLinux 9.3");
        assert_eq!(result.template_hint, "AlmaLinux");
    }

    #[test]
    fn test_opensuse() {
        let result = detect_from_filename("/isos/openSUSE-Leap-15.5-DVD-x86_64.iso").unwrap();
        assert_eq!(result.name, "openSUSE");
        assert_eq!(result.template_hint, "openSUSE");
    }

    #[test]
    fn test_nixos() {
        let result = detect_from_filename("/isos/nixos-gnome-24.05-x86_64-linux.iso").unwrap();
        assert_eq!(result.name, "NixOS");
        assert_eq!(result.template_hint, "NixOS");
    }

    #[test]
    fn test_endeavouros() {
        let result = detect_from_filename("/isos/endeavouros-2024.06-x86_64.iso").unwrap();
        assert_eq!(result.name, "EndeavourOS");
        assert_eq!(result.template_hint, "EndeavourOS");
    }

    #[test]
    fn test_centos() {
        let result = detect_from_filename("/isos/CentOS-Stream-9-latest-x86_64-dvd1.iso").unwrap();
        assert_eq!(result.name, "CentOS 9");
        assert_eq!(result.template_hint, "CentOS");
    }

    #[test]
    fn test_windows_server() {
        let result = detect_from_filename("/isos/WinServer2022_x64.iso").unwrap();
        assert_eq!(result.name, "Windows Server");
        assert_eq!(result.template_hint, "Windows Server");
    }

    #[test]
    fn test_unknown_iso() {
        let result = detect_from_filename("/isos/some-random-thing.iso");
        assert!(result.is_none());
    }

    #[test]
    fn test_path_validation_empty() {
        assert!(!validate_path(""));
    }

    #[test]
    fn test_path_validation_null_byte() {
        assert!(!validate_path("/isos/bad\0path.iso"));
    }

    #[test]
    fn test_path_validation_traversal() {
        assert!(!validate_path("/isos/../../../etc/passwd"));
    }

    #[test]
    fn test_path_validation_ok() {
        assert!(validate_path("/home/user/Downloads/ubuntu-24.04.iso"));
    }

    #[test]
    fn test_extract_version_after() {
        assert_eq!(
            extract_version_after("ubuntu-24.04", "ubuntu-"),
            Some("24.04".to_string())
        );
        assert_eq!(
            extract_version_after("debian-12", "debian-"),
            Some("12".to_string())
        );
        assert_eq!(
            extract_version_after("freebsd-14.0", "freebsd-"),
            Some("14.0".to_string())
        );
    }

    #[test]
    fn test_extract_first_version_segment() {
        assert_eq!(
            extract_first_version_segment("fedora-workstation-live-x86_64-40", "fedora"),
            Some("40".to_string())
        );
        assert_eq!(
            extract_first_version_segment("rocky-9.3-x86_64-dvd", "rocky"),
            Some("9.3".to_string())
        );
        assert_eq!(
            extract_first_version_segment("centos-stream-9-latest-x86_64-dvd1", "centos"),
            Some("9".to_string())
        );
    }

    #[test]
    fn test_extract_version_from_label() {
        assert_eq!(
            extract_version_from_label("ubuntu 24.04.1 lts", "ubuntu"),
            Some("24.04".to_string())
        );
        assert_eq!(
            extract_version_from_label("fedora-40-workstation", "fedora"),
            Some("40".to_string())
        );
    }
}
