//! REST API route handlers for Libre VMM.
//!
//! ## Endpoints
//!
//! ### System
//! - `GET  /api/v1/health`           — Health check (no auth required)
//! - `GET  /api/v1/system/info`      — Hypervisor info
//!
//! ### VMs
//! - `GET    /api/v1/vms`            — List all VMs
//! - `GET    /api/v1/vms/:name`      — Get VM details
//! - `POST   /api/v1/vms`            — Create a new VM
//! - `DELETE /api/v1/vms/:name`      — Delete a VM
//! - `POST   /api/v1/vms/:name/start`     — Start VM
//! - `POST   /api/v1/vms/:name/shutdown`  — Graceful shutdown
//! - `POST   /api/v1/vms/:name/stop`      — Force stop
//! - `POST   /api/v1/vms/:name/pause`     — Pause VM
//! - `POST   /api/v1/vms/:name/resume`    — Resume VM
//! - `POST   /api/v1/vms/:name/reboot`    — Reboot VM
//!
//! ### Snapshots
//! - `GET    /api/v1/vms/:name/snapshots`               — List snapshots
//! - `POST   /api/v1/vms/:name/snapshots`               — Create snapshot
//! - `POST   /api/v1/vms/:name/snapshots/:snap/revert`  — Revert to snapshot
//! - `DELETE /api/v1/vms/:name/snapshots/:snap`          — Delete snapshot
//!
//! ### Console
//! - `POST   /api/v1/vms/:name/console`  — Start noVNC browser console
//!
//! ### Clone
//! - `POST   /api/v1/vms/:name/clone`    — Clone a VM (full or linked)

use axum::{
    extract::Path,
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use vmm_core::config::VmConfigIo;

use crate::error::AppError;

// ============================================================
// Response types
// ============================================================

#[derive(Serialize, ToSchema)]
pub struct VmListResponse {
    /// All VMs known to the hypervisor (regardless of state).
    pub vms: Vec<VmResponse>,
    /// Number of VMs in `vms`. Mirrors `vms.len()` for client convenience.
    pub count: usize,
}

#[derive(Serialize, ToSchema)]
pub struct VmResponse {
    /// VM name (unique within the hypervisor).
    pub name: String,
    /// Libvirt UUID for the VM.
    pub uuid: String,
    /// Current state: `running`, `paused`, `shut off`, `crashed`, etc.
    pub state: String,
    /// Number of virtual CPUs allocated.
    pub vcpus: u32,
    /// RAM allocation in MiB.
    pub memory_mib: u64,
    /// Cumulative CPU time consumed by the VM in nanoseconds.
    pub cpu_time_ns: u64,
}

impl From<vmm_core::VmInfo> for VmResponse {
    fn from(info: vmm_core::VmInfo) -> Self {
        Self {
            name: info.name,
            uuid: info.uuid,
            state: info.state.to_string(),
            vcpus: info.vcpus,
            memory_mib: info.memory_mib,
            cpu_time_ns: info.cpu_time_ns,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct SnapshotResponse {
    /// Snapshot name (unique per VM).
    pub name: String,
    /// Free-form description supplied at creation time.
    pub description: String,
    /// ISO-8601-ish timestamp string of when the snapshot was taken.
    pub creation_time: String,
    /// VM state captured by the snapshot (`running`, `shutoff`, etc.).
    pub state: String,
}

impl From<vmm_core::SnapshotInfo> for SnapshotResponse {
    fn from(info: vmm_core::SnapshotInfo) -> Self {
        Self {
            name: info.name,
            description: info.description,
            creation_time: info.creation_time.to_string(),
            state: info.state,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct HealthResponse {
    /// Always `"ok"` if the server is reachable.
    pub status: String,
    /// Server version (matches the `vmm-api` crate version).
    pub version: String,
    /// Libvirt connection status. Always `false` over this endpoint —
    /// it is intentionally redacted from the unauthenticated health probe.
    /// Use `GET /api/v1/system/info` (authenticated) for the real value.
    pub libvirt_connected: bool,
}

/// SECURITY: connection_uri deliberately excluded — it can contain socket paths
/// and host information that aids reconnaissance (CWE-200).
#[derive(Serialize, ToSchema)]
pub struct SystemInfoResponse {
    /// Hypervisor product/version string (e.g. `QEMU 8.2.2`).
    pub hypervisor: String,
    /// Whether KVM acceleration is available on the host.
    pub kvm_available: bool,
    /// Connection family: `qemu/kvm`, `xen`, or `unknown`.
    pub connection_type: String,
}

#[derive(Serialize, ToSchema)]
pub struct MessageResponse {
    /// Human-readable status message describing the action result.
    pub message: String,
}

/// Standard error envelope returned for all 4xx and 5xx responses.
#[derive(Serialize, ToSchema)]
pub struct ApiErrorResponse {
    /// Human-readable error description. Internal details are stripped server-side.
    pub error: String,
    /// HTTP status code, duplicated in the body for client convenience.
    pub code: u16,
}

// ============================================================
// Request types
// ============================================================

/// SECURITY: `deny_unknown_fields` rejects payloads with unexpected keys (CWE-20).
/// This prevents mass-assignment style attacks and limits deserialization surface.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateVmRequest {
    /// VM name (1-64 chars, alphanumerics + `-_.`). Must be unique.
    #[schema(example = "arch-dev")]
    pub name: String,
    /// Template ID or label substring. Defaults to `arch-linux`.
    #[serde(default = "default_template")]
    #[schema(example = "arch-linux")]
    pub template: String,
    /// Virtual CPU count (1-256). Defaults to 2.
    #[serde(default = "default_vcpus")]
    #[schema(example = 2, minimum = 1, maximum = 256)]
    pub vcpus: u32,
    /// RAM in MiB (64 - 1048576). Defaults to 4096.
    #[serde(default = "default_memory")]
    #[schema(example = 4096, minimum = 64, maximum = 1048576)]
    pub memory_mib: u64,
    /// Primary disk size in GiB (1 - 16384). Defaults to 25.
    #[serde(default = "default_disk")]
    #[schema(example = 25, minimum = 1, maximum = 16384)]
    pub disk_size_gib: u64,
    /// Optional path to a boot ISO on the host. Must not contain `..`.
    #[serde(default)]
    pub iso_path: Option<String>,
    /// Optional free-form description (max 1024 chars).
    #[serde(default)]
    pub description: String,
    /// Boot via UEFI (LibreUEFI/OVMF). Defaults to `true`.
    #[serde(default = "default_true")]
    pub uefi: bool,
}

fn default_template() -> String {
    "arch-linux".to_string()
}
fn default_vcpus() -> u32 {
    2
}
fn default_memory() -> u64 {
    4096
}
fn default_disk() -> u64 {
    25
}
fn default_true() -> bool {
    true
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateSnapshotRequest {
    /// Snapshot name (1-128 chars, alphanumerics + ` -_.()`). Must not start with `-`.
    #[schema(example = "pre-upgrade")]
    pub name: String,
    /// Optional description (max 1024 chars). Must not contain `<`, `>`, or `&`.
    #[serde(default)]
    pub description: String,
}

// ============================================================
// Helpers
// ============================================================

fn get_conn() -> Result<vmm_core::HypervisorConnection, AppError> {
    vmm_core::HypervisorConnection::connect_best().map_err(AppError::from)
}

/// SECURITY: Validate VM name from URL path parameters before passing to vmm-core (CWE-20).
/// All routes that accept a VM name from the URL MUST call this.
/// Prevents argument injection into virsh, XML injection, and path traversal.
fn validate_api_vm_name(name: &str) -> Result<(), AppError> {
    if let Some(err) = vmm_core::config::validate_vm_name(name) {
        return Err(AppError(vmm_core::VmmError::InvalidConfig(format!(
            "Invalid VM name: {}",
            err
        ))));
    }
    Ok(())
}

/// SECURITY: Validate snapshot name from URL path parameter (CWE-20).
fn validate_api_snapshot_name(name: &str) -> Result<(), AppError> {
    if name.is_empty() || name.len() > 128 {
        return Err(AppError(vmm_core::VmmError::InvalidConfig(
            "Snapshot name must be 1-128 characters".to_string(),
        )));
    }
    if name.starts_with('-') {
        return Err(AppError(vmm_core::VmmError::InvalidConfig(
            "Snapshot name must not start with '-'".to_string(),
        )));
    }
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || " -_.()".contains(c))
    {
        return Err(AppError(vmm_core::VmmError::InvalidConfig(
            "Snapshot name contains invalid characters".to_string(),
        )));
    }
    Ok(())
}

// ============================================================
// System routes
// ============================================================

pub fn system_routes() -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/system/info", get(system_info))
}

#[utoipa::path(
    get,
    path = "/api/v1/health",
    tag = "System",
    summary = "Liveness probe (unauthenticated)",
    description = "Returns server status without contacting libvirt. \
                   Does NOT require an API key — safe for load balancers and uptime checks. \
                   `libvirt_connected` is always returned as `false`; use `/api/v1/system/info` for the real value.",
    responses(
        (status = 200, description = "Server is reachable", body = HealthResponse),
    ),
)]
pub async fn health() -> Json<HealthResponse> {
    // SECURITY: Do not expose libvirt connection status to unauthenticated clients (CWE-306).
    // The health endpoint bypasses API key auth, so it must not leak internal state.
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        libvirt_connected: false, // Redacted — requires authentication via /api/v1/system/info
    })
}

#[utoipa::path(
    get,
    path = "/api/v1/system/info",
    tag = "System",
    summary = "Hypervisor and host info",
    description = "Returns the hypervisor product/version, KVM availability, and connection family. \
                   The full libvirt connection URI is deliberately omitted (CWE-200).",
    responses(
        (status = 200, description = "Hypervisor info", body = SystemInfoResponse),
        (status = 401, description = "Missing or invalid X-API-Key", body = ApiErrorResponse),
        (status = 500, description = "Internal server error (e.g., libvirt unreachable)", body = ApiErrorResponse),
    ),
    security(("api_key" = [])),
)]
pub async fn system_info() -> Result<Json<SystemInfoResponse>, AppError> {
    let conn = get_conn()?;
    let hypervisor = conn
        .hypervisor_info()
        .unwrap_or_else(|_| "Unknown".to_string());
    let kvm = conn.kvm_available();
    // SECURITY: Only expose the connection type (qemu, xen, etc.), not the full URI
    // which may contain socket paths or hostnames (CWE-200: Information Exposure).
    let uri = conn.uri().to_string();
    let conn_type = if uri.contains("qemu") {
        "qemu/kvm"
    } else if uri.contains("xen") {
        "xen"
    } else {
        "unknown"
    };

    Ok(Json(SystemInfoResponse {
        hypervisor,
        kvm_available: kvm,
        connection_type: conn_type.to_string(),
    }))
}

// ============================================================
// VM routes
// ============================================================

pub fn vm_routes() -> Router {
    Router::new()
        .route("/api/v1/vms", get(list_vms).post(create_vm))
        .route("/api/v1/vms/{name}", get(get_vm).delete(delete_vm))
        .route("/api/v1/vms/{name}/start", post(start_vm))
        .route("/api/v1/vms/{name}/shutdown", post(shutdown_vm))
        .route("/api/v1/vms/{name}/stop", post(stop_vm))
        .route("/api/v1/vms/{name}/pause", post(pause_vm))
        .route("/api/v1/vms/{name}/resume", post(resume_vm))
        .route("/api/v1/vms/{name}/reboot", post(reboot_vm))
}

#[utoipa::path(
    get,
    path = "/api/v1/vms",
    tag = "VMs",
    summary = "List all VMs",
    description = "Returns every VM defined in the hypervisor, including running, paused, and shut-off VMs. \
                   No pagination — the list is typically small (single-user workstation use case).",
    responses(
        (status = 200, description = "List of VMs", body = VmListResponse),
        (status = 401, description = "Missing or invalid X-API-Key", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    ),
    security(("api_key" = [])),
)]
pub async fn list_vms() -> Result<Json<VmListResponse>, AppError> {
    let conn = get_conn()?;
    let vms = conn.list_vms()?;
    let count = vms.len();
    let vms: Vec<VmResponse> = vms.into_iter().map(VmResponse::from).collect();
    Ok(Json(VmListResponse { vms, count }))
}

#[utoipa::path(
    get,
    path = "/api/v1/vms/{name}",
    tag = "VMs",
    summary = "Get a single VM by name",
    description = "Returns details of one VM. The `name` path parameter is validated server-side \
                   (1-64 chars, alphanumerics + `-_.`) before being passed to libvirt.",
    params(
        ("name" = String, Path, description = "VM name", example = "arch-dev"),
    ),
    responses(
        (status = 200, description = "VM details", body = VmResponse),
        (status = 400, description = "Invalid VM name", body = ApiErrorResponse),
        (status = 401, description = "Missing or invalid X-API-Key", body = ApiErrorResponse),
        (status = 404, description = "VM not found", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    ),
    security(("api_key" = [])),
)]
pub async fn get_vm(Path(name): Path<String>) -> Result<Json<VmResponse>, AppError> {
    validate_api_vm_name(&name)?;
    let conn = get_conn()?;
    let vms = conn.list_vms()?;
    let vm = vms
        .into_iter()
        .find(|v| v.name == name)
        .ok_or_else(|| AppError(vmm_core::VmmError::VmNotFound { name: name.clone() }))?;
    Ok(Json(VmResponse::from(vm)))
}

#[utoipa::path(
    post,
    path = "/api/v1/vms",
    tag = "VMs",
    summary = "Create a new VM",
    description = "Defines a new VM from a built-in template. The VM is created in the `shut off` state — \
                   call `POST /api/v1/vms/{name}/start` to boot it. \
                   All numeric fields are clamped to safe ranges; oversized strings and path-traversal in \
                   `iso_path` are rejected.",
    request_body = CreateVmRequest,
    responses(
        (status = 201, description = "VM created", body = MessageResponse),
        (status = 400, description = "Invalid request (bad name, out-of-range numbers, etc.)", body = ApiErrorResponse),
        (status = 401, description = "Missing or invalid X-API-Key", body = ApiErrorResponse),
        (status = 415, description = "Content-Type must be application/json"),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    ),
    security(("api_key" = [])),
)]
pub async fn create_vm(
    Json(req): Json<CreateVmRequest>,
) -> Result<(StatusCode, Json<MessageResponse>), AppError> {
    let conn = get_conn()?;

    // Find matching template
    // SECURITY (CWE-129): Use .get(0) instead of [0] to avoid panic if templates list
    // is ever empty. Fall back to returning an error rather than crashing the API server.
    let templates = vmm_core::template::builtin_templates();
    let template = match templates
        .iter()
        .find(|t| {
            t.id == req.template
                || t.label
                    .to_lowercase()
                    .contains(&req.template.to_lowercase())
        })
        .or_else(|| templates.first())
    {
        Some(t) => t,
        None => {
            return Err(AppError(vmm_core::VmmError::InvalidConfig(
                "No VM templates available".to_string(),
            )))
        },
    };

    // SECURITY: Validate resource bounds to prevent host DoS (CWE-20)
    if let Some(err) = vmm_core::config::validate_vm_name(&req.name) {
        return Err(AppError(vmm_core::VmmError::InvalidConfig(format!(
            "Invalid VM name: {}",
            err
        ))));
    }
    // SECURITY: Cap string field lengths to prevent memory abuse (CWE-400)
    if req.description.len() > 1024 {
        return Err(AppError(vmm_core::VmmError::InvalidConfig(
            "Description too long (max 1024 chars)".to_string(),
        )));
    }
    if req.template.len() > 128 {
        return Err(AppError(vmm_core::VmmError::InvalidConfig(
            "Template name too long (max 128 chars)".to_string(),
        )));
    }
    if let Some(ref iso) = req.iso_path {
        if iso.len() > 4096 {
            return Err(AppError(vmm_core::VmmError::InvalidConfig(
                "ISO path too long (max 4096 chars)".to_string(),
            )));
        }
        // SECURITY: Reject path traversal in iso_path (CWE-22)
        if iso.contains("..") {
            return Err(AppError(vmm_core::VmmError::InvalidConfig(
                "ISO path must not contain '..'".to_string(),
            )));
        }
    }
    if req.vcpus == 0 || req.vcpus > 256 {
        return Err(AppError(vmm_core::VmmError::InvalidConfig(
            "vcpus must be between 1 and 256".to_string(),
        )));
    }
    if req.memory_mib < 64 || req.memory_mib > 1024 * 1024 {
        return Err(AppError(vmm_core::VmmError::InvalidConfig(
            "memory_mib must be between 64 and 1048576 (1 TiB)".to_string(),
        )));
    }
    if req.disk_size_gib == 0 || req.disk_size_gib > 16384 {
        return Err(AppError(vmm_core::VmmError::InvalidConfig(
            "disk_size_gib must be between 1 and 16384 (16 TiB)".to_string(),
        )));
    }

    let mut config = vmm_core::VmConfig::from_template(&req.name, template, None);
    config.vcpus = req.vcpus;
    config.memory_mib = req.memory_mib;
    config.disk_size_gib = req.disk_size_gib;
    config.uefi = req.uefi;
    config.description = req.description;
    config.iso_path = req.iso_path;

    conn.create_vm(&config)?;

    Ok((
        StatusCode::CREATED,
        Json(MessageResponse {
            message: format!("VM '{}' created successfully", req.name),
        }),
    ))
}

#[utoipa::path(
    delete,
    path = "/api/v1/vms/{name}",
    tag = "VMs",
    summary = "Delete a VM (and its disks)",
    description = "Removes the VM definition AND the backing disk images. \
                   This is irreversible. If the VM is running it will be force-stopped first.",
    params(
        ("name" = String, Path, description = "VM name", example = "arch-dev"),
    ),
    responses(
        (status = 200, description = "VM deleted", body = MessageResponse),
        (status = 400, description = "Invalid VM name", body = ApiErrorResponse),
        (status = 401, description = "Missing or invalid X-API-Key", body = ApiErrorResponse),
        (status = 404, description = "VM not found", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    ),
    security(("api_key" = [])),
)]
pub async fn delete_vm(Path(name): Path<String>) -> Result<Json<MessageResponse>, AppError> {
    validate_api_vm_name(&name)?;
    let conn = get_conn()?;
    conn.delete_vm(&name, true)?;
    Ok(Json(MessageResponse {
        message: format!("VM '{}' deleted", name),
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/vms/{name}/start",
    tag = "VMs",
    summary = "Start a VM",
    description = "Boots a previously-created VM. Returns 409 if the VM is already running.",
    params(
        ("name" = String, Path, description = "VM name", example = "arch-dev"),
    ),
    responses(
        (status = 200, description = "VM started", body = MessageResponse),
        (status = 400, description = "Invalid VM name", body = ApiErrorResponse),
        (status = 401, description = "Missing or invalid X-API-Key", body = ApiErrorResponse),
        (status = 404, description = "VM not found", body = ApiErrorResponse),
        (status = 409, description = "VM is already running", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    ),
    security(("api_key" = [])),
)]
pub async fn start_vm(Path(name): Path<String>) -> Result<Json<MessageResponse>, AppError> {
    validate_api_vm_name(&name)?;
    let conn = get_conn()?;
    conn.start_vm(&name)?;
    Ok(Json(MessageResponse {
        message: format!("VM '{}' started", name),
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/vms/{name}/shutdown",
    tag = "VMs",
    summary = "Graceful shutdown",
    description = "Sends an ACPI shutdown signal to the guest OS. The guest may ignore it. \
                   For an unconditional power-off use `/stop` instead.",
    params(
        ("name" = String, Path, description = "VM name", example = "arch-dev"),
    ),
    responses(
        (status = 200, description = "Shutdown signal sent", body = MessageResponse),
        (status = 400, description = "Invalid VM name", body = ApiErrorResponse),
        (status = 401, description = "Missing or invalid X-API-Key", body = ApiErrorResponse),
        (status = 404, description = "VM not found", body = ApiErrorResponse),
        (status = 409, description = "VM is not running", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    ),
    security(("api_key" = [])),
)]
pub async fn shutdown_vm(Path(name): Path<String>) -> Result<Json<MessageResponse>, AppError> {
    validate_api_vm_name(&name)?;
    let conn = get_conn()?;
    conn.shutdown_vm(&name)?;
    Ok(Json(MessageResponse {
        message: format!("Shutdown signal sent to '{}'", name),
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/vms/{name}/stop",
    tag = "VMs",
    summary = "Force-stop a VM",
    description = "Immediately powers off the VM (equivalent to pulling the plug). \
                   Risks data loss in the guest. Prefer `/shutdown` for clean shutdowns.",
    params(
        ("name" = String, Path, description = "VM name", example = "arch-dev"),
    ),
    responses(
        (status = 200, description = "VM force-stopped", body = MessageResponse),
        (status = 400, description = "Invalid VM name", body = ApiErrorResponse),
        (status = 401, description = "Missing or invalid X-API-Key", body = ApiErrorResponse),
        (status = 404, description = "VM not found", body = ApiErrorResponse),
        (status = 409, description = "VM is not running", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    ),
    security(("api_key" = [])),
)]
pub async fn stop_vm(Path(name): Path<String>) -> Result<Json<MessageResponse>, AppError> {
    validate_api_vm_name(&name)?;
    let conn = get_conn()?;
    conn.force_stop_vm(&name)?;
    Ok(Json(MessageResponse {
        message: format!("VM '{}' force-stopped", name),
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/vms/{name}/pause",
    tag = "VMs",
    summary = "Pause a running VM",
    description = "Suspends VM execution while keeping its memory resident. Resume with `/resume`.",
    params(
        ("name" = String, Path, description = "VM name", example = "arch-dev"),
    ),
    responses(
        (status = 200, description = "VM paused", body = MessageResponse),
        (status = 400, description = "Invalid VM name", body = ApiErrorResponse),
        (status = 401, description = "Missing or invalid X-API-Key", body = ApiErrorResponse),
        (status = 404, description = "VM not found", body = ApiErrorResponse),
        (status = 409, description = "VM is not running", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    ),
    security(("api_key" = [])),
)]
pub async fn pause_vm(Path(name): Path<String>) -> Result<Json<MessageResponse>, AppError> {
    validate_api_vm_name(&name)?;
    let conn = get_conn()?;
    conn.pause_vm(&name)?;
    Ok(Json(MessageResponse {
        message: format!("VM '{}' paused", name),
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/vms/{name}/resume",
    tag = "VMs",
    summary = "Resume a paused VM",
    description = "Resumes execution of a VM that was paused via `/pause`.",
    params(
        ("name" = String, Path, description = "VM name", example = "arch-dev"),
    ),
    responses(
        (status = 200, description = "VM resumed", body = MessageResponse),
        (status = 400, description = "Invalid VM name", body = ApiErrorResponse),
        (status = 401, description = "Missing or invalid X-API-Key", body = ApiErrorResponse),
        (status = 404, description = "VM not found", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    ),
    security(("api_key" = [])),
)]
pub async fn resume_vm(Path(name): Path<String>) -> Result<Json<MessageResponse>, AppError> {
    validate_api_vm_name(&name)?;
    let conn = get_conn()?;
    conn.resume_vm(&name)?;
    Ok(Json(MessageResponse {
        message: format!("VM '{}' resumed", name),
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/vms/{name}/reboot",
    tag = "VMs",
    summary = "Reboot a VM",
    description = "Sends an ACPI reboot signal to the guest OS. The guest may ignore it.",
    params(
        ("name" = String, Path, description = "VM name", example = "arch-dev"),
    ),
    responses(
        (status = 200, description = "Reboot signal sent", body = MessageResponse),
        (status = 400, description = "Invalid VM name", body = ApiErrorResponse),
        (status = 401, description = "Missing or invalid X-API-Key", body = ApiErrorResponse),
        (status = 404, description = "VM not found", body = ApiErrorResponse),
        (status = 409, description = "VM is not running", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    ),
    security(("api_key" = [])),
)]
pub async fn reboot_vm(Path(name): Path<String>) -> Result<Json<MessageResponse>, AppError> {
    validate_api_vm_name(&name)?;
    let conn = get_conn()?;
    conn.reboot_vm(&name)?;
    Ok(Json(MessageResponse {
        message: format!("Reboot signal sent to '{}'", name),
    }))
}

// ============================================================
// Snapshot routes
// ============================================================

pub fn snapshot_routes() -> Router {
    Router::new()
        .route(
            "/api/v1/vms/{name}/snapshots",
            get(list_snapshots).post(create_snapshot),
        )
        .route(
            "/api/v1/vms/{name}/snapshots/{snap}/revert",
            post(revert_snapshot),
        )
        .route(
            "/api/v1/vms/{name}/snapshots/{snap}",
            delete(delete_snapshot),
        )
}

#[utoipa::path(
    get,
    path = "/api/v1/vms/{name}/snapshots",
    tag = "Snapshots",
    summary = "List snapshots for a VM",
    description = "Returns every snapshot defined for the given VM, oldest first. \
                   An empty list is returned (not 404) when the VM has no snapshots.",
    params(
        ("name" = String, Path, description = "VM name", example = "arch-dev"),
    ),
    responses(
        (status = 200, description = "Snapshot list (possibly empty)", body = [SnapshotResponse]),
        (status = 400, description = "Invalid VM name", body = ApiErrorResponse),
        (status = 401, description = "Missing or invalid X-API-Key", body = ApiErrorResponse),
        (status = 404, description = "VM not found", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    ),
    security(("api_key" = [])),
)]
pub async fn list_snapshots(
    Path(name): Path<String>,
) -> Result<Json<Vec<SnapshotResponse>>, AppError> {
    validate_api_vm_name(&name)?;
    let conn = get_conn()?;
    let snaps = vmm_core::snapshot::list_snapshots(conn.raw_conn(), &name)
        .map_err(|e| AppError(vmm_core::VmmError::SnapshotError(e.to_string())))?;
    let snaps: Vec<SnapshotResponse> = snaps.into_iter().map(SnapshotResponse::from).collect();
    Ok(Json(snaps))
}

#[utoipa::path(
    post,
    path = "/api/v1/vms/{name}/snapshots",
    tag = "Snapshots",
    summary = "Create a snapshot",
    description = "Captures the current VM state (and memory, if running) as a named snapshot. \
                   The snapshot name must be 1-128 chars (alphanumerics + ` -_.()`) and must not start \
                   with `-`. The description must not contain `<`, `>`, or `&` (XML injection defense, CWE-91).",
    params(
        ("name" = String, Path, description = "VM name", example = "arch-dev"),
    ),
    request_body = CreateSnapshotRequest,
    responses(
        (status = 201, description = "Snapshot created", body = MessageResponse),
        (status = 400, description = "Invalid name or description", body = ApiErrorResponse),
        (status = 401, description = "Missing or invalid X-API-Key", body = ApiErrorResponse),
        (status = 404, description = "VM not found", body = ApiErrorResponse),
        (status = 415, description = "Content-Type must be application/json"),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    ),
    security(("api_key" = [])),
)]
pub async fn create_snapshot(
    Path(name): Path<String>,
    Json(req): Json<CreateSnapshotRequest>,
) -> Result<(StatusCode, Json<MessageResponse>), AppError> {
    validate_api_vm_name(&name)?;
    // SECURITY: Validate snapshot name to prevent XML injection (CWE-20)
    if req.name.is_empty() || req.name.len() > 128 {
        return Err(AppError(vmm_core::VmmError::InvalidConfig(
            "Snapshot name must be 1-128 characters".to_string(),
        )));
    }
    if !req
        .name
        .chars()
        .all(|c| c.is_alphanumeric() || " -_.()".contains(c))
    {
        return Err(AppError(vmm_core::VmmError::InvalidConfig(
            "Snapshot name contains invalid characters".to_string(),
        )));
    }
    if req.description.len() > 1024 {
        return Err(AppError(vmm_core::VmmError::InvalidConfig(
            "Snapshot description too long (max 1024 chars)".to_string(),
        )));
    }
    // SECURITY: Reject XML-special characters in description to prevent XML injection (CWE-91).
    // The snapshot module applies xml_escape(), but defense-in-depth rejects at the API layer.
    if req.description.contains('<')
        || req.description.contains('>')
        || req.description.contains('&')
    {
        return Err(AppError(vmm_core::VmmError::InvalidConfig(
            "Snapshot description must not contain <, >, or & characters".to_string(),
        )));
    }
    let conn = get_conn()?;
    vmm_core::snapshot::create_snapshot(conn.raw_conn(), &name, &req.name, &req.description)
        .map_err(|e| AppError(vmm_core::VmmError::SnapshotError(e.to_string())))?;
    Ok((
        StatusCode::CREATED,
        Json(MessageResponse {
            message: format!("Snapshot '{}' created for VM '{}'", req.name, name),
        }),
    ))
}

#[utoipa::path(
    post,
    path = "/api/v1/vms/{name}/snapshots/{snap}/revert",
    tag = "Snapshots",
    summary = "Revert to a snapshot",
    description = "Rolls the VM back to the state captured by the snapshot. \
                   Any state written since the snapshot is discarded.",
    params(
        ("name" = String, Path, description = "VM name", example = "arch-dev"),
        ("snap" = String, Path, description = "Snapshot name", example = "pre-upgrade"),
    ),
    responses(
        (status = 200, description = "Revert succeeded", body = MessageResponse),
        (status = 400, description = "Invalid VM or snapshot name", body = ApiErrorResponse),
        (status = 401, description = "Missing or invalid X-API-Key", body = ApiErrorResponse),
        (status = 404, description = "VM or snapshot not found", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    ),
    security(("api_key" = [])),
)]
pub async fn revert_snapshot(
    Path((name, snap)): Path<(String, String)>,
) -> Result<Json<MessageResponse>, AppError> {
    validate_api_vm_name(&name)?;
    validate_api_snapshot_name(&snap)?;
    let conn = get_conn()?;
    vmm_core::snapshot::revert_snapshot(conn.raw_conn(), &name, &snap)
        .map_err(|e| AppError(vmm_core::VmmError::SnapshotError(e.to_string())))?;
    Ok(Json(MessageResponse {
        message: format!("Reverted VM '{}' to snapshot '{}'", name, snap),
    }))
}

#[utoipa::path(
    delete,
    path = "/api/v1/vms/{name}/snapshots/{snap}",
    tag = "Snapshots",
    summary = "Delete a snapshot",
    description = "Removes a snapshot from the VM. The VM itself is not affected. Irreversible.",
    params(
        ("name" = String, Path, description = "VM name", example = "arch-dev"),
        ("snap" = String, Path, description = "Snapshot name", example = "pre-upgrade"),
    ),
    responses(
        (status = 200, description = "Snapshot deleted", body = MessageResponse),
        (status = 400, description = "Invalid VM or snapshot name", body = ApiErrorResponse),
        (status = 401, description = "Missing or invalid X-API-Key", body = ApiErrorResponse),
        (status = 404, description = "VM or snapshot not found", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    ),
    security(("api_key" = [])),
)]
pub async fn delete_snapshot(
    Path((name, snap)): Path<(String, String)>,
) -> Result<Json<MessageResponse>, AppError> {
    validate_api_vm_name(&name)?;
    validate_api_snapshot_name(&snap)?;
    let conn = get_conn()?;
    vmm_core::snapshot::delete_snapshot(conn.raw_conn(), &name, &snap)
        .map_err(|e| AppError(vmm_core::VmmError::SnapshotError(e.to_string())))?;
    Ok(Json(MessageResponse {
        message: format!("Snapshot '{}' deleted from VM '{}'", snap, name),
    }))
}

// ============================================================
// Console routes (noVNC)
// ============================================================

pub fn console_routes() -> Router {
    Router::new().route("/api/v1/vms/{name}/console", post(start_console))
}

#[derive(Serialize, ToSchema)]
pub struct ConsoleResponse {
    /// Browser URL that serves the noVNC client (open in any browser).
    pub url: String,
    /// TCP port the noVNC proxy is listening on.
    pub port: u16,
    /// Human-readable status message.
    pub message: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ConsoleRequest {
    /// VNC port of the running VM. Defaults to 5900.
    #[serde(default = "default_vnc_port")]
    #[schema(example = 5900, minimum = 1)]
    pub vnc_port: u16,
    /// Non-privileged listen port for the noVNC proxy (>= 1024). Defaults to 6080.
    #[serde(default = "default_novnc_port")]
    #[schema(example = 6080, minimum = 1024)]
    pub listen_port: u16,
}

fn default_vnc_port() -> u16 {
    5900
}
fn default_novnc_port() -> u16 {
    6080
}

#[utoipa::path(
    post,
    path = "/api/v1/vms/{name}/console",
    tag = "Console",
    summary = "Start a noVNC browser console for a running VM",
    description = "Spawns a noVNC proxy that bridges the VM's VNC server to a browser-friendly \
                   WebSocket. The returned URL can be opened in any browser. The VM must be running. \
                   The proxy continues to run until the API server exits.",
    params(
        ("name" = String, Path, description = "VM name", example = "arch-dev"),
    ),
    request_body = ConsoleRequest,
    responses(
        (status = 201, description = "Console started", body = ConsoleResponse),
        (status = 400, description = "VM not running or invalid ports", body = ApiErrorResponse),
        (status = 401, description = "Missing or invalid X-API-Key", body = ApiErrorResponse),
        (status = 404, description = "VM not found", body = ApiErrorResponse),
        (status = 415, description = "Content-Type must be application/json"),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    ),
    security(("api_key" = [])),
)]
pub async fn start_console(
    Path(name): Path<String>,
    Json(req): Json<ConsoleRequest>,
) -> Result<(StatusCode, Json<ConsoleResponse>), AppError> {
    validate_api_vm_name(&name)?;

    // Verify VM is running
    let conn = get_conn()?;
    let vms = conn.list_vms()?;
    let vm = vms
        .iter()
        .find(|v| v.name == name)
        .ok_or_else(|| AppError(vmm_core::VmmError::VmNotFound { name: name.clone() }))?;

    if vm.state != vmm_core::VmState::Running {
        return Err(AppError(vmm_core::VmmError::InvalidConfig(
            "VM must be running to access console".to_string(),
        )));
    }

    // SECURITY: Validate port ranges (CWE-20)
    if req.listen_port < 1024 || req.vnc_port == 0 {
        return Err(AppError(vmm_core::VmmError::InvalidConfig(
            "Ports must be non-privileged (>= 1024) and non-zero".to_string(),
        )));
    }

    let server = vmm_core::novnc::start_novnc(&name, req.vnc_port, req.listen_port)
        .map_err(|e| AppError(e))?;

    let url = server.url();
    // Intentionally leak the server handle — it will keep running until the API process exits.
    // A production implementation would track servers per-VM for cleanup.
    std::mem::forget(server);

    Ok((
        StatusCode::CREATED,
        Json(ConsoleResponse {
            url: url.clone(),
            port: req.listen_port,
            message: format!("noVNC console started for '{}' at {}", name, url),
        }),
    ))
}

// ============================================================
// Clone routes
// ============================================================

pub fn clone_routes() -> Router {
    Router::new().route("/api/v1/vms/{name}/clone", post(clone_vm))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CloneRequest {
    /// Name for the cloned VM (same validation rules as VM creation).
    #[schema(example = "arch-dev-clone")]
    pub new_name: String,
    /// Clone type: `full` (independent copy) or `linked` (copy-on-write, shares base disk). Defaults to `full`.
    #[serde(default = "default_clone_type")]
    #[schema(example = "full")]
    pub clone_type: String,
}

fn default_clone_type() -> String {
    "full".to_string()
}

#[utoipa::path(
    post,
    path = "/api/v1/vms/{name}/clone",
    tag = "Cloning",
    summary = "Clone a VM (full or linked)",
    description = "Creates a copy of an existing VM. \
                   `full` clones produce an independent disk image. `linked` clones use copy-on-write \
                   on top of the source disk, which is fast but ties the clone to the source's lifecycle.",
    params(
        ("name" = String, Path, description = "Source VM name", example = "arch-dev"),
    ),
    request_body = CloneRequest,
    responses(
        (status = 201, description = "Clone created", body = MessageResponse),
        (status = 400, description = "Invalid source or clone name", body = ApiErrorResponse),
        (status = 401, description = "Missing or invalid X-API-Key", body = ApiErrorResponse),
        (status = 404, description = "Source VM not found", body = ApiErrorResponse),
        (status = 415, description = "Content-Type must be application/json"),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    ),
    security(("api_key" = [])),
)]
pub async fn clone_vm(
    Path(name): Path<String>,
    Json(req): Json<CloneRequest>,
) -> Result<(StatusCode, Json<MessageResponse>), AppError> {
    validate_api_vm_name(&name)?;
    if let Some(err) = vmm_core::config::validate_vm_name(&req.new_name) {
        return Err(AppError(vmm_core::VmmError::InvalidConfig(format!(
            "Invalid clone name: {}",
            err
        ))));
    }

    let clone_type = match req.clone_type.to_lowercase().as_str() {
        "linked" => vmm_core::CloneType::Linked,
        _ => vmm_core::CloneType::Full,
    };

    let conn = get_conn()?;

    // Load source VM config
    let configs = vmm_core::config::VmConfig::list_all().map_err(|e| AppError(e))?;
    let source_config = configs
        .iter()
        .find(|c| c.name == name)
        .ok_or_else(|| AppError(vmm_core::VmmError::VmNotFound { name: name.clone() }))?;

    vmm_core::clone::clone_vm(&conn, source_config, &req.new_name, &clone_type)?;

    Ok((
        StatusCode::CREATED,
        Json(MessageResponse {
            message: format!("VM '{}' cloned as '{}'", name, req.new_name),
        }),
    ))
}
