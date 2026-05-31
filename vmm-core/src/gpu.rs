//! GPU capability detection for 3D acceleration and device passthrough.
//!
//! Detects host GPU capabilities: virgl (virtio-gpu 3D), VFIO-capable
//! PCI devices, and OpenGL renderer info. Helps users understand what
//! acceleration is available on their system.

use tracing::info;

/// Host GPU capabilities summary.
#[derive(Debug, Clone)]
pub struct GpuCapabilities {
    /// Whether KVM is available (prerequisite for virgl)
    pub kvm_available: bool,
    /// Whether virgl (virtio-gpu 3D rendering) appears supported
    pub virgl_supported: bool,
    /// Whether any GPU-class PCI devices are bound to vfio-pci
    pub vfio_devices: Vec<VfioGpuDevice>,
    /// Host OpenGL renderer string (if available)
    pub gl_renderer: Option<String>,
    /// Summary message for the user
    pub summary: String,
}

/// A PCI device available for VFIO passthrough.
#[derive(Debug, Clone)]
pub struct VfioGpuDevice {
    /// PCI address (e.g., "0000:01:00.0")
    pub pci_address: String,
    /// Device description from lspci
    pub description: String,
    /// Whether currently bound to vfio-pci driver
    pub vfio_bound: bool,
}

/// Detect GPU capabilities on the host system.
pub fn detect_gpu_capabilities() -> GpuCapabilities {
    info!("Detecting GPU capabilities...");

    let kvm_available = std::path::Path::new("/dev/kvm").exists();

    // Check for virgl support by looking for the virtio-gpu module
    // and checking if QEMU was built with virgl support
    let virgl_supported = check_virgl_support();

    // Scan for VFIO-capable GPU devices
    let vfio_devices = scan_vfio_gpu_devices();

    // Try to get GL renderer info
    let gl_renderer = get_gl_renderer();

    // Build summary
    let mut parts = Vec::new();
    if kvm_available {
        parts.push("KVM acceleration available".to_string());
    }
    if virgl_supported {
        parts.push("VirGL 3D rendering supported".to_string());
    }
    if !vfio_devices.is_empty() {
        let bound = vfio_devices.iter().filter(|d| d.vfio_bound).count();
        parts.push(format!(
            "{} GPU(s) detected ({} bound to VFIO)",
            vfio_devices.len(),
            bound
        ));
    }
    if let Some(ref gl) = gl_renderer {
        parts.push(format!("Host renderer: {}", gl));
    }

    let summary = if parts.is_empty() {
        "No GPU acceleration features detected".to_string()
    } else {
        parts.join(" | ")
    };

    GpuCapabilities {
        kvm_available,
        virgl_supported,
        vfio_devices,
        gl_renderer,
        summary,
    }
}

/// Check if VirGL (virtio-gpu 3D) is likely supported.
fn check_virgl_support() -> bool {
    // Method 1: Check if the kernel module virtio_gpu is loaded/available
    if std::path::Path::new("/sys/module/virtio_gpu").exists() {
        return true;
    }

    // Method 2: Check QEMU capabilities via qemu-system-x86_64 -device help
    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance to child process.
    if let Ok(output) = std::process::Command::new("qemu-system-x86_64")
        .args(["-device", "help"])
        .stdin(std::process::Stdio::null())
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.contains("virtio-gpu-gl-pci") || stdout.contains("virtio-vga-gl") {
            return true;
        }
    }

    // Method 3: Check if /dev/dri/renderD128 exists (DRI render node)
    if std::path::Path::new("/dev/dri/renderD128").exists() {
        return true;
    }

    false
}

/// Scan for PCI GPU devices that could be used with VFIO passthrough.
fn scan_vfio_gpu_devices() -> Vec<VfioGpuDevice> {
    let mut devices = Vec::new();

    // Use lspci to find VGA/3D controllers
    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance to child process.
    let output = match std::process::Command::new("lspci")
        .args(["-nn", "-D"])
        .stdin(std::process::Stdio::null())
        .output()
    {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return devices,
    };

    for line in output.lines() {
        // Look for VGA compatible controller or 3D controller
        if line.contains("VGA compatible controller")
            || line.contains("3D controller")
            || line.contains("Display controller")
        {
            // Extract PCI address (first field)
            let pci_address = line.split_whitespace().next().unwrap_or("").to_string();

            // SECURITY: Validate PCI address format before using it to construct sysfs paths (CWE-22).
            // lspci output could theoretically contain crafted PCI addresses like
            // "../../etc/shadow" that would traverse the sysfs directory structure.
            // Valid PCI format: DDDD:BB:DD.F (domain:bus:device.function), e.g., "0000:01:00.0"
            if !validate_pci_address(&pci_address) {
                tracing::warn!("Skipping invalid PCI address from lspci: {}", pci_address);
                continue;
            }

            // Check if bound to vfio-pci
            let vfio_bound = if !pci_address.is_empty() {
                let driver_link = format!("/sys/bus/pci/devices/{}/driver", pci_address);
                if let Ok(driver) = std::fs::read_link(&driver_link) {
                    driver
                        .file_name()
                        .map(|n| n.to_string_lossy().contains("vfio"))
                        .unwrap_or(false)
                } else {
                    false
                }
            } else {
                false
            };

            // Get description (everything after the PCI class)
            // SECURITY (CWE-91 / XML Injection): Sanitize the description string.
            // This field comes from lspci output and could contain arbitrary text.
            // If ever interpolated into libvirt XML, characters like <, >, &, ', "
            // would enable XML injection. Sanitize defensively at the source.
            let raw_description = if let Some(idx) = line.find(": ") {
                &line[idx + 2..]
            } else {
                line
            };
            let description = sanitize_xml_text(raw_description);

            devices.push(VfioGpuDevice {
                pci_address,
                description,
                vfio_bound,
            });
        }
    }

    devices
}

/// Validate a PCI address matches the strict format: DDDD:BB:DD.F
/// (4 hex : 2 hex : 2 hex . 1 hex digit). Example: "0000:01:00.0"
///
/// SECURITY (CWE-22: Path Traversal, CWE-20: Improper Input Validation):
/// This address is used to construct sysfs paths like /sys/bus/pci/devices/<addr>/driver.
/// A loose validator that allows variable-length segments could permit crafted strings
/// (e.g., "0000:01:00.0/../../shadow") to traverse the filesystem. We enforce exact
/// segment lengths to guarantee the address maps to a single, canonical sysfs entry.
fn validate_pci_address(addr: &str) -> bool {
    // Exact length: DDDD:BB:DD.F = 4+1+2+1+2+1+1 = 12 characters
    if addr.len() != 12 {
        return false;
    }
    // SECURITY: Only allow hex digits, colons, and dots — no slashes, dots-dots, etc.
    if !addr
        .chars()
        .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.')
    {
        return false;
    }
    // Must have exactly 3 colon-separated parts
    // SECURITY (CWE-129): Use .get() instead of direct indexing for defense-in-depth,
    // even though the length check above guards against out-of-bounds.
    let parts: Vec<&str> = addr.split(':').collect();
    if parts.len() != 3 {
        return false;
    }
    let (domain, bus, devfunc) = match (parts.get(0), parts.get(1), parts.get(2)) {
        (Some(d), Some(b), Some(df)) => (*d, *b, *df),
        _ => return false,
    };
    // Enforce exact segment lengths: domain=4, bus=2, dev.func
    if domain.len() != 4 || bus.len() != 2 {
        return false;
    }
    // Last segment must be exactly DD.F (2 hex, dot, 1 hex)
    let last_parts: Vec<&str> = devfunc.split('.').collect();
    if last_parts.len() != 2 {
        return false;
    }
    let (dev, func) = match (last_parts.get(0), last_parts.get(1)) {
        (Some(d), Some(f)) => (*d, *f),
        _ => return false,
    };
    if dev.len() != 2 || func.len() != 1 {
        return false;
    }
    // All characters in each segment must be hex (already checked above, but belt-and-suspenders)
    for p in &[domain, bus] {
        if !p.chars().all(|c| c.is_ascii_hexdigit()) {
            return false;
        }
    }
    for p in &[dev, func] {
        if !p.chars().all(|c| c.is_ascii_hexdigit()) {
            return false;
        }
    }
    true
}

/// Sanitize a string for safe inclusion in XML text/attributes.
///
/// SECURITY (CWE-91): Escapes XML special characters to prevent injection
/// when device descriptions or other external strings are embedded in
/// libvirt XML definitions. Also strips control characters (CWE-116).
fn sanitize_xml_text(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '\'' => output.push_str("&apos;"),
            '"' => output.push_str("&quot;"),
            // Strip control characters except tab, newline, carriage return
            c if c.is_control() && c != '\t' && c != '\n' && c != '\r' => {},
            c => output.push(c),
        }
    }
    output
}

/// Try to get the host's OpenGL renderer string.
fn get_gl_renderer() -> Option<String> {
    // Try glxinfo -B for a quick summary
    // SECURITY: CWE-403 — Close stdin to prevent FD inheritance to child process.
    if let Ok(output) = std::process::Command::new("glxinfo")
        .args(["-B"])
        .stdin(std::process::Stdio::null())
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.starts_with("OpenGL renderer string:") {
                    // SECURITY (CWE-91): Sanitize renderer string from external command output
                    let renderer = line.trim_start_matches("OpenGL renderer string:").trim();
                    return Some(sanitize_xml_text(renderer));
                }
            }
        }
    }

    None
}
