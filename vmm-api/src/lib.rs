//! vmm-api library — exposes routes, schemas, and the OpenAPI document.
//!
//! The binary (`main.rs`) consumes these modules to assemble the HTTP server.
//! External tools (e.g. `gen_openapi`) consume `openapi::ApiDoc` to emit the
//! committed `docs/openapi.json` spec.

pub mod auth;
pub mod error;
pub mod openapi;
pub mod routes;
