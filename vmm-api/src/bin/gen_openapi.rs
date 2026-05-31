//! Emits the committed `docs/openapi.json` spec.
//!
//! Run from the workspace root:
//! ```
//! cargo run -p vmm-api --bin gen_openapi
//! ```
//!
//! The output path is resolved relative to the current working directory so
//! that it lands in `<workspace>/docs/openapi.json` when invoked from the
//! workspace root.

use std::path::PathBuf;
use utoipa::OpenApi;
use vmm_api::openapi::ApiDoc;

fn main() {
    let json = ApiDoc::openapi()
        .to_pretty_json()
        .expect("OpenApi serialization should always succeed for a derived ApiDoc");

    // Prefer `<cwd>/docs/openapi.json`. If `docs/` does not exist next to the
    // current directory, fall back to `./openapi.json` so the binary still
    // succeeds when run from a checkout that lacks the `docs/` folder.
    let cwd = std::env::current_dir().expect("cwd readable");
    let primary = cwd.join("docs").join("openapi.json");
    let target: PathBuf = if cwd.join("docs").is_dir() {
        primary
    } else {
        cwd.join("openapi.json")
    };

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).expect("create output directory");
    }

    std::fs::write(&target, json).expect("write OpenAPI JSON");
    println!("Wrote {}", target.display());
}
