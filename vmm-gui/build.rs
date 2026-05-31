fn main() {
    // Link against libspice-client-glib for the embedded SPICE console.
    // Uses pkg-config to find the library paths and flags.
    let lib = pkg_config::Config::new().probe("spice-client-glib-2.0");

    match lib {
        Ok(_) => {
            println!("cargo:rustc-cfg=has_spice");
        },
        Err(e) => {
            // SPICE dev headers not installed — build without SPICE support.
            // The GUI will fall back to VNC-only mode.
            println!(
                "cargo:warning=libspice-client-glib-2.0 not found: {}. SPICE console disabled.",
                e
            );
        },
    }
}
