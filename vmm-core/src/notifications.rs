//! Desktop notifications for VM state changes and operation results.
//!
//! Uses `notify-rust` crate for desktop notifications on Linux.
//! Fires notifications for: VM start/stop, snapshot complete, clone done,
//! errors, and long-running task completion.

use tracing::warn;

/// Notification urgency level.
#[derive(Debug, Clone, PartialEq)]
pub enum NotifyUrgency {
    /// Informational — green, auto-dismisses.
    Info,
    /// Warning — yellow, persists longer.
    Warning,
    /// Error — red, persists until dismissed.
    Error,
}

/// Notification category for grouping/filtering.
#[derive(Debug, Clone, PartialEq)]
pub enum NotifyCategory {
    /// VM power state change (start, stop, pause, resume).
    VmPower,
    /// Snapshot operation (create, revert, delete).
    Snapshot,
    /// Long-running task completed (clone, export, import).
    TaskComplete,
    /// Error occurred.
    Error,
    /// General info.
    General,
}

/// A pending notification.
#[derive(Debug, Clone)]
pub struct Notification {
    pub summary: String,
    pub body: String,
    pub urgency: NotifyUrgency,
    pub category: NotifyCategory,
}

/// Whether desktop notifications are enabled.
/// Controlled by user preference in app settings.
#[derive(Debug, Clone)]
pub struct NotificationSettings {
    pub enabled: bool,
    pub vm_power_events: bool,
    pub snapshot_events: bool,
    pub task_events: bool,
    pub error_events: bool,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            vm_power_events: true,
            snapshot_events: true,
            task_events: true,
            error_events: true,
        }
    }
}

impl NotificationSettings {
    /// Check if a notification category is enabled.
    pub fn is_category_enabled(&self, category: &NotifyCategory) -> bool {
        if !self.enabled {
            return false;
        }
        match category {
            NotifyCategory::VmPower => self.vm_power_events,
            NotifyCategory::Snapshot => self.snapshot_events,
            NotifyCategory::TaskComplete => self.task_events,
            NotifyCategory::Error => self.error_events,
            NotifyCategory::General => true,
        }
    }
}

/// Send a desktop notification.
/// Falls back gracefully if notify-send is not available.
pub fn send_notification(notif: &Notification, settings: &NotificationSettings) {
    if !settings.is_category_enabled(&notif.category) {
        return;
    }

    // Use notify-send CLI (available on most Linux desktops)
    // This avoids adding a native crate dependency and works universally.
    let urgency = match notif.urgency {
        NotifyUrgency::Info => "low",
        NotifyUrgency::Warning => "normal",
        NotifyUrgency::Error => "critical",
    };

    let expire_ms = match notif.urgency {
        NotifyUrgency::Info => "5000",
        NotifyUrgency::Warning => "10000",
        NotifyUrgency::Error => "0", // persist until dismissed
    };

    // SECURITY: Sanitize summary and body to prevent Pango markup injection (CWE-116)
    // and control character injection (CWE-74).
    // notify-send renders HTML/Pango markup in some desktop environments (GNOME, etc.).
    // A malicious VM name like "<b>IMPORTANT</b><a href='http://evil'>click</a>" could
    // render clickable links or styled text. A bare '&' can cause Pango parse errors
    // or be used in entity injection ("&lt;" → "<"). Strip all markup-significant chars.
    let safe_summary: String = sanitize_notification_text(&notif.summary, 200);
    let safe_body: String = sanitize_notification_text(&notif.body, 500);

    // SECURITY: Use .status() instead of .spawn() to avoid zombie process accumulation
    // (CWE-404 / CWE-400). spawn() without wait() leaks zombie processes — under heavy
    // event load this could exhaust the process table (DoS).
    // Also close stdin (CWE-404) to prevent child from inheriting/blocking on parent stdin.
    let result = std::process::Command::new("notify-send")
        .arg("--app-name=Libre VMM")
        .arg(format!("--urgency={}", urgency))
        .arg(format!("--expire-time={}", expire_ms))
        .arg("--icon=computer")
        .arg("--") // SECURITY: Prevent summary from being interpreted as a flag (CWE-88)
        .arg(&safe_summary)
        .arg(&safe_body)
        .stdin(std::process::Stdio::null()) // CWE-404: don't inherit parent stdin
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status(); // Waits for child — no zombie leak

    if let Err(e) = result {
        warn!("Failed to send desktop notification: {}", e);
    }
}

/// Convenience: notify about a VM power state change.
pub fn notify_vm_power(vm_name: &str, action: &str, settings: &NotificationSettings) {
    let notif = Notification {
        summary: format!("VM {}", action),
        body: format!("'{}' has been {}", vm_name, action.to_lowercase()),
        urgency: NotifyUrgency::Info,
        category: NotifyCategory::VmPower,
    };
    send_notification(&notif, settings);
}

/// Convenience: notify about a snapshot operation.
pub fn notify_snapshot(
    vm_name: &str,
    action: &str,
    snap_name: &str,
    settings: &NotificationSettings,
) {
    let notif = Notification {
        summary: format!("Snapshot {}", action),
        body: format!("'{}' on VM '{}'", snap_name, vm_name),
        urgency: NotifyUrgency::Info,
        category: NotifyCategory::Snapshot,
    };
    send_notification(&notif, settings);
}

/// Convenience: notify about a completed task.
pub fn notify_task_complete(description: &str, success: bool, settings: &NotificationSettings) {
    let notif = Notification {
        summary: if success {
            "Task Complete".to_string()
        } else {
            "Task Failed".to_string()
        },
        body: description.to_string(),
        urgency: if success {
            NotifyUrgency::Info
        } else {
            NotifyUrgency::Error
        },
        category: if success {
            NotifyCategory::TaskComplete
        } else {
            NotifyCategory::Error
        },
    };
    send_notification(&notif, settings);
}

/// Convenience: notify about an error.
pub fn notify_error(summary: &str, detail: &str, settings: &NotificationSettings) {
    let notif = Notification {
        summary: summary.to_string(),
        body: detail.to_string(),
        urgency: NotifyUrgency::Error,
        category: NotifyCategory::Error,
    };
    send_notification(&notif, settings);
}

/// SECURITY: Sanitize text for safe use in desktop notifications (CWE-116, CWE-74).
///
/// Strips characters that are significant in Pango/HTML markup to prevent:
/// - Markup injection via `<`, `>` (HTML tags, clickable links)
/// - Entity injection via `&` (e.g., `&lt;` → `<`, or malformed entities crashing Pango)
/// - Control character injection (newlines could break layout, null bytes cause truncation)
///
/// Caps length to prevent notification daemon DoS from unbounded messages (CWE-400).
fn sanitize_notification_text(text: &str, max_len: usize) -> String {
    text.chars()
        .filter(|c| !c.is_control() && *c != '<' && *c != '>' && *c != '&')
        .take(max_len)
        .collect()
}

/// Check if the notification system is available.
pub fn notifications_available() -> bool {
    std::process::Command::new("notify-send")
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
