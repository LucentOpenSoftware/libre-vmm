//! OpenAPI 3.0 document for the Libre VMM REST API.
//!
//! The [`ApiDoc`] struct is consumed in two places:
//!
//! 1. At runtime, by `main.rs`, to mount `/api/v1/openapi.json`,
//!    `/api/v1/docs` (swagger-ui), and `/api/v1/redoc`.
//! 2. At build/release time, by the `gen_openapi` binary, to emit a
//!    committed `docs/openapi.json` for distribution alongside the source.
//!
//! ## Authentication
//! Every endpoint except `/api/v1/health` and the documentation endpoints
//! themselves requires a `X-API-Key` header. The key is printed once at
//! server startup (or supplied via `--api-key`).

use utoipa::{Modify, OpenApi};

#[derive(OpenApi)]
#[openapi(
    paths(
        // System
        crate::routes::health,
        crate::routes::system_info,
        // VM lifecycle
        crate::routes::list_vms,
        crate::routes::get_vm,
        crate::routes::create_vm,
        crate::routes::delete_vm,
        crate::routes::start_vm,
        crate::routes::shutdown_vm,
        crate::routes::stop_vm,
        crate::routes::pause_vm,
        crate::routes::resume_vm,
        crate::routes::reboot_vm,
        // Snapshots
        crate::routes::list_snapshots,
        crate::routes::create_snapshot,
        crate::routes::revert_snapshot,
        crate::routes::delete_snapshot,
        // Console
        crate::routes::start_console,
        // Cloning
        crate::routes::clone_vm,
    ),
    components(schemas(
        crate::routes::VmResponse,
        crate::routes::VmListResponse,
        crate::routes::SnapshotResponse,
        crate::routes::HealthResponse,
        crate::routes::SystemInfoResponse,
        crate::routes::MessageResponse,
        crate::routes::ApiErrorResponse,
        crate::routes::CreateVmRequest,
        crate::routes::CreateSnapshotRequest,
        crate::routes::ConsoleRequest,
        crate::routes::ConsoleResponse,
        crate::routes::CloneRequest,
    )),
    modifiers(&SecurityAddon),
    tags(
        (name = "System", description = "Hypervisor and host info, plus the unauthenticated health probe."),
        (name = "VMs", description = "VM lifecycle: create, delete, start, stop, pause, resume, reboot."),
        (name = "Snapshots", description = "Per-VM snapshot create / list / revert / delete."),
        (name = "Console", description = "Browser-based noVNC console access for running VMs."),
        (name = "Cloning", description = "Full and linked VM cloning."),
    ),
    info(
        title = "Libre VMM REST API",
        version = env!("CARGO_PKG_VERSION"),
        description = "A libre alternative to VMware Workstation, with a real REST API that VMware Workstation does not provide.\n\n\
                       ## Authentication\n\n\
                       All endpoints except `GET /api/v1/health` and the docs themselves require an `X-API-Key` header. \
                       The key is printed once on stderr when the server starts (or set via `--api-key`).\n\n\
                       ```\n\
                       curl -H 'X-API-Key: <key>' http://127.0.0.1:8420/api/v1/vms\n\
                       ```\n\n\
                       ## Rate limiting\n\n\
                       After 10 failed auth attempts within 60 s the server locks out **all** requests for 5 minutes. \
                       This is a global limiter (per-IP is trivially bypassed via proxies).\n\n\
                       ## Errors\n\n\
                       All 4xx/5xx responses share the [`ApiErrorResponse`] envelope. \
                       Internal error details (libvirt messages, file paths) are scrubbed server-side (CWE-209).",
        license(name = "GPL-3.0-or-later"),
    ),
    servers(
        (url = "http://127.0.0.1:8420", description = "Default localhost binding"),
    ),
)]
pub struct ApiDoc;

/// Injects the `api_key` security scheme (X-API-Key header).
///
/// Without this modifier, `security(("api_key" = []))` on each handler would
/// reference a scheme that doesn't exist in `components.securitySchemes`,
/// producing an invalid OpenAPI document.
pub struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        use utoipa::openapi::security::{ApiKey, ApiKeyValue, SecurityScheme};

        let components = openapi
            .components
            .as_mut()
            .expect("OpenApi components must exist (utoipa always creates them when schemas are registered)");

        components.add_security_scheme(
            "api_key",
            SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::new("X-API-Key"))),
        );
    }
}
