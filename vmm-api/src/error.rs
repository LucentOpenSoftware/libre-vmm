//! API error types and HTTP status code mapping.
//!
//! ## Security
//! - Internal errors return generic messages to clients (CWE-209)
//! - Full error details are logged server-side only
//! - JSON rejection errors are sanitized to prevent info disclosure

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

/// API error response body.
#[derive(Serialize)]
pub struct ApiError {
    pub error: String,
    pub code: u16,
}

/// Wrapper for vmm-core errors that implements IntoResponse.
pub struct AppError(pub vmm_core::VmmError);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self.0 {
            vmm_core::VmmError::VmNotFound { .. } => {
                // Safe to expose: only reveals that a VM name doesn't exist
                (StatusCode::NOT_FOUND, "VM not found".to_string())
            },
            vmm_core::VmmError::VmAlreadyRunning { .. } => {
                (StatusCode::CONFLICT, "VM is already running".to_string())
            },
            vmm_core::VmmError::VmNotRunning { .. } => {
                (StatusCode::CONFLICT, "VM is not running".to_string())
            },
            vmm_core::VmmError::InvalidConfig(msg) => {
                // SECURITY: InvalidConfig messages are constructed by our code (not libvirt),
                // so they are safe to return. But sanitize just in case.
                let safe_msg = sanitize_error_message(msg);
                (StatusCode::BAD_REQUEST, safe_msg)
            },
            _ => {
                // SECURITY: Don't leak internal error details to API clients (CWE-209).
                // Log the full error server-side, return generic message to client.
                // This catches libvirt errors, IO errors, etc. that may contain:
                // - Internal file paths (CWE-209)
                // - Libvirt connection URIs
                // - Stack traces or system details
                tracing::error!("Internal error: {}", self.0);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            },
        };

        let body = ApiError {
            error: message,
            code: status.as_u16(),
        };

        (status, Json(body)).into_response()
    }
}

impl From<vmm_core::VmmError> for AppError {
    fn from(err: vmm_core::VmmError) -> Self {
        AppError(err)
    }
}

/// SECURITY: Sanitize error messages before returning to clients (CWE-209).
///
/// Strips potential internal paths, connection strings, and other sensitive
/// details that might have been included in error messages.
fn sanitize_error_message(msg: &str) -> String {
    // Don't return messages that look like they contain file paths
    if msg.contains("/home/")
        || msg.contains("/etc/")
        || msg.contains("/var/")
        || msg.contains("/tmp/")
    {
        tracing::warn!("Sanitized error message containing internal path: {}", msg);
        return "Invalid configuration".to_string();
    }
    // Don't return messages with connection URIs
    if msg.contains("qemu://") || msg.contains("qemu+") {
        tracing::warn!("Sanitized error message containing connection URI: {}", msg);
        return "Invalid configuration".to_string();
    }
    // Truncate overly long error messages (defense against error message injection)
    if msg.len() > 256 {
        return format!("{}...", &msg[..256]);
    }
    msg.to_string()
}
