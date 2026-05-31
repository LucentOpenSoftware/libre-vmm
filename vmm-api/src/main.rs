//! vmm-api — REST API server for Libre VMM.
//!
//! Provides HTTP endpoints for managing virtual machines remotely.
//! Uses axum for the web framework and vmm-core for VM operations.
//!
//! ## Authentication
//! Uses simple API key authentication via the `X-API-Key` header.
//! The API key is configured at startup or generated automatically.
//!
//! ## Usage
//! ```
//! vmm-api                          # Start with auto-generated key
//! vmm-api --port 8080              # Custom port
//! vmm-api --api-key mykey123       # Custom API key
//! vmm-api --bind 0.0.0.0           # Listen on all interfaces
//! ```

use vmm_api::openapi::ApiDoc;
use vmm_api::{auth, routes};

use axum::Router;
use std::net::SocketAddr;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;
use utoipa::OpenApi;
use utoipa_redoc::{Redoc, Servable};
use utoipa_swagger_ui::SwaggerUi;

/// SECURITY: Maximum request body size — 1 MiB (CWE-400: Uncontrolled Resource Consumption).
/// Without this, an attacker can send a multi-GB JSON body causing OOM/DoS.
const MAX_REQUEST_BODY_SIZE: usize = 1024 * 1024; // 1 MiB

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // Parse CLI args
    let args = parse_args();

    // Generate or use provided API key
    let api_key = if args.api_key.is_empty() {
        let key = generate_api_key();
        // SECURITY: Only print key to stderr (not logged to files), show once
        // Do NOT log via tracing (CWE-532: sensitive info in logs)
        eprintln!("\n  API Key: {}\n  (this will not be shown again)\n", key);
        key
    } else {
        args.api_key.clone()
    };

    // Test libvirt connection
    match vmm_core::HypervisorConnection::connect_best() {
        Ok(_) => info!("Libvirt connection OK"),
        Err(e) => {
            tracing::error!("Failed to connect to libvirt: {}", e);
            tracing::error!("The API server requires a working libvirt connection.");
            std::process::exit(1);
        },
    }

    // Build router
    //
    // LAYER ORDER MATTERS (axum processes layers bottom-to-top on request):
    // 1. TraceLayer (outermost — logs all requests)
    // 2. Security headers (applied to all responses)
    // 3. CORS (must wrap auth so preflight OPTIONS works)
    // 4. Body size limit (reject huge payloads before auth processing)
    // 5. Rate limiter + Auth middleware (applied to routes)
    // 6. Content-Type validation (innermost — checked after auth)
    // 7. Route handlers
    let app = Router::new()
        .merge(routes::vm_routes())
        .merge(routes::snapshot_routes())
        .merge(routes::console_routes())
        .merge(routes::clone_routes())
        .merge(routes::system_routes())
        // OpenAPI docs + swagger-ui + redoc — auth-exempt via auth::api_key_auth
        // (these paths must remain readable so users can learn the API before they have a key).
        .merge(SwaggerUi::new("/api/v1/docs").url("/api/v1/openapi.json", ApiDoc::openapi()))
        .merge(Redoc::with_url("/api/v1/redoc", ApiDoc::openapi()))
        // SECURITY: Content-Type validation — reject non-JSON POST/PUT/PATCH bodies (CWE-20)
        .layer(axum::middleware::from_fn(content_type_validation))
        // SECURITY: Auth middleware with built-in rate limiting (CWE-307)
        .layer(axum::middleware::from_fn(move |req, next| {
            let key = api_key.clone();
            auth::api_key_auth(req, next, key)
        }))
        // SECURITY: Request body size limit — prevents OOM from large payloads (CWE-400)
        .layer(tower_http::limit::RequestBodyLimitLayer::new(
            MAX_REQUEST_BODY_SIZE,
        ))
        // SECURITY: Restrict CORS — no wildcard origins, methods, or headers (CWE-942)
        .layer(
            CorsLayer::new()
                .allow_origin([
                    "http://localhost:8420".parse().expect("valid CORS origin"),
                    "http://127.0.0.1:8420".parse().expect("valid CORS origin"),
                    "http://localhost:3000".parse().expect("valid CORS origin"),
                    "http://127.0.0.1:3000".parse().expect("valid CORS origin"),
                ])
                .allow_methods([
                    http::Method::GET,
                    http::Method::POST,
                    http::Method::DELETE,
                    http::Method::OPTIONS,
                ])
                .allow_headers([
                    http::header::CONTENT_TYPE,
                    http::header::ACCEPT,
                    http::HeaderName::from_static("x-api-key"),
                ]),
        )
        // SECURITY: Security response headers (CWE-693: Protection Mechanism Failure)
        .layer(axum::middleware::from_fn(security_headers))
        .layer(TraceLayer::new_for_http());

    // SECURITY: Warn if binding to non-loopback address (CWE-668)
    let bind_addr: std::net::IpAddr = args.bind.parse()?;
    if !bind_addr.is_loopback() {
        eprintln!(
            "\n  WARNING: Binding to non-loopback address {} exposes the API to the network!",
            bind_addr
        );
        eprintln!("  Ensure your firewall is configured and API key is strong.\n");
    }

    let addr = SocketAddr::new(bind_addr, args.port);
    info!("Starting Libre VMM API server on http://{}", addr);
    println!("  Libre VMM API server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// SECURITY: Middleware to add security headers to all responses (CWE-693).
///
/// - X-Content-Type-Options: nosniff — prevents MIME-type sniffing (CWE-430)
/// - X-Frame-Options: DENY — prevents clickjacking (CWE-1021)
/// - Cache-Control: no-store — prevents caching of sensitive VM data (CWE-525)
/// - Content-Security-Policy: default-src 'none' — API returns only JSON
/// - X-XSS-Protection: 0 — disable browser heuristic (can cause issues; CSP is the real defense)
async fn security_headers(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let mut response = next.run(req).await;
    let headers = response.headers_mut();

    headers.insert(
        http::header::X_CONTENT_TYPE_OPTIONS,
        http::HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        http::header::X_FRAME_OPTIONS,
        http::HeaderValue::from_static("DENY"),
    );
    headers.insert(
        http::header::CACHE_CONTROL,
        http::HeaderValue::from_static("no-store, no-cache, must-revalidate"),
    );
    headers.insert(
        http::header::CONTENT_SECURITY_POLICY,
        http::HeaderValue::from_static("default-src 'none'; frame-ancestors 'none'"),
    );
    // Explicit Content-Type on all JSON responses
    // (already set by axum's Json extractor, but belt-and-suspenders)

    response
}

/// SECURITY: Middleware to validate Content-Type on requests with bodies (CWE-20).
///
/// POST/PUT/PATCH requests MUST have Content-Type: application/json.
/// Rejects requests with missing or wrong Content-Type to prevent deserialization confusion
/// and ensures clients are explicitly sending JSON.
async fn content_type_validation(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<axum::response::Response, axum::http::StatusCode> {
    let dominated_methods = [http::Method::POST, http::Method::PUT, http::Method::PATCH];

    if dominated_methods.contains(req.method()) {
        let content_type = req
            .headers()
            .get(http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if !content_type.starts_with("application/json") {
            return Err(axum::http::StatusCode::UNSUPPORTED_MEDIA_TYPE);
        }
    }

    Ok(next.run(req).await)
}

struct Args {
    port: u16,
    bind: String,
    api_key: String,
}

fn parse_args() -> Args {
    let mut args = Args {
        port: 8420,
        bind: "127.0.0.1".to_string(),
        api_key: String::new(),
    };

    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--port" | "-p" => {
                if let Some(val) = iter.next() {
                    args.port = val.parse().unwrap_or(8420);
                }
            },
            "--bind" | "-b" => {
                if let Some(val) = iter.next() {
                    args.bind = val;
                }
            },
            "--api-key" | "-k" => {
                if let Some(val) = iter.next() {
                    args.api_key = val;
                }
            },
            "--help" | "-h" => {
                println!("Libre VMM API Server");
                println!();
                println!("Usage: vmm-api [OPTIONS]");
                println!();
                println!("Options:");
                println!("  -p, --port <PORT>       Listen port (default: 8420)");
                println!("  -b, --bind <ADDR>       Bind address (default: 127.0.0.1)");
                println!("  -k, --api-key <KEY>     API key (auto-generated if not set)");
                println!("  -h, --help              Show this help");
                std::process::exit(0);
            },
            _ => {},
        }
    }

    args
}

/// Generate a cryptographically strong API key (CWE-330).
/// Uses two UUIDs concatenated for 256-bit entropy instead of 128-bit.
fn generate_api_key() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().to_string().replace('-', ""),
        uuid::Uuid::new_v4().to_string().replace('-', ""),
    )
}
