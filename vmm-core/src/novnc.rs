//! noVNC browser console — websockify proxy for browser-based VNC access.
//!
//! Wraps `websockify` to serve noVNC HTML/JS files and proxy WebSocket
//! connections to the QEMU VNC port, enabling browser-based VM console access.

use crate::error::{VmmError, VmmResult};
use std::process::{Child, Command, Stdio};
use tracing::info;

/// Status of the noVNC server.
#[derive(Debug, Clone, PartialEq)]
pub enum NoVncStatus {
    Stopped,
    Starting,
    Running,
    Error(String),
}

/// A running noVNC/websockify server instance.
pub struct NoVncServer {
    pub port: u16,
    pub vm_name: String,
    pub vnc_port: u16,
    pub status: NoVncStatus,
    child: Option<Child>,
}

impl NoVncServer {
    /// Get the browser URL for this noVNC instance.
    pub fn url(&self) -> String {
        format!("http://localhost:{}/vnc.html?autoconnect=true", self.port)
    }
}

impl Drop for NoVncServer {
    fn drop(&mut self) {
        if let Some(ref mut child) = self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Check if websockify is available on the system.
pub fn websockify_available() -> bool {
    Command::new("websockify")
        .arg("--help")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

/// Find the noVNC web directory (HTML/JS files).
pub fn find_novnc_path() -> Option<String> {
    let candidates = [
        "/usr/share/novnc",
        "/usr/share/noVNC",
        "/usr/share/webapps/novnc",
        "/usr/local/share/novnc",
    ];
    for path in &candidates {
        if std::path::Path::new(path).join("vnc.html").exists() {
            return Some(path.to_string());
        }
    }
    None
}

/// Start a noVNC/websockify server proxying to a QEMU VNC port.
///
/// SECURITY: CWE-284 — Binds to 127.0.0.1 only to prevent remote access.
pub fn start_novnc(vm_name: &str, vnc_port: u16, listen_port: u16) -> VmmResult<NoVncServer> {
    if !websockify_available() {
        return Err(VmmError::Other(
            "websockify not found — install python3-websockify".to_string(),
        ));
    }

    let web_path = find_novnc_path().unwrap_or_else(|| "/usr/share/novnc".to_string());

    // SECURITY: Validate port range — reject privileged ports (CWE-20)
    if listen_port < 1024 {
        return Err(VmmError::Other(
            "Listen port must be >= 1024 (non-privileged)".to_string(),
        ));
    }
    // SECURITY: Reject vnc_port == 0 (invalid port)
    if vnc_port == 0 {
        return Err(VmmError::Other("VNC port must not be 0".to_string()));
    }

    info!(
        "Starting noVNC for '{}' on port {} (VNC port {})",
        vm_name, listen_port, vnc_port
    );

    // SECURITY: CWE-284 — Bind to localhost only
    let child = Command::new("websockify")
        .args([
            "--web",
            &web_path,
            &format!("127.0.0.1:{}", listen_port),
            &format!("127.0.0.1:{}", vnc_port),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| VmmError::Other(format!("Failed to start websockify: {}", e)))?;

    Ok(NoVncServer {
        port: listen_port,
        vm_name: vm_name.to_string(),
        vnc_port,
        status: NoVncStatus::Running,
        child: Some(child),
    })
}

/// Stop a running noVNC server.
pub fn stop_novnc(server: &mut NoVncServer) -> VmmResult<()> {
    if let Some(ref mut child) = server.child {
        let _ = child.kill();
        let _ = child.wait();
        info!("noVNC server stopped for '{}'", server.vm_name);
    }
    server.child = None;
    server.status = NoVncStatus::Stopped;
    Ok(())
}

/// Open a URL in the default browser.
pub fn open_in_browser(url: &str) -> VmmResult<()> {
    // SECURITY: CWE-78 — Use `--` to separate URL from flags to prevent injection.
    Command::new("xdg-open")
        .arg("--")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| VmmError::Other(format!("Failed to open browser: {}", e)))?;
    Ok(())
}
