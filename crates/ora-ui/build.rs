//! Validates the embedded UI assets.
//!
//! The assets are not version controlled, they are built from the `ui` directory
//! of the Ora repository with `cargo xtask ui build` and shipped in the published crate.

use std::path::Path;

include!("src/markers.rs");

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let dist = Path::new(&manifest_dir).join("dist");

    println!("cargo::rerun-if-changed=dist");

    let index = match std::fs::read_to_string(dist.join("index.html")) {
        Ok(index) => index,
        Err(error) => fail(&format!(
            "the UI assets are missing ({}: {error})",
            dist.join("index.html").display()
        )),
    };

    for marker in [BASE_MARKER, API_URL_MARKER] {
        if !index.contains(marker) {
            fail(&format!(
                "`{}` does not contain `{marker}`, it is not an Ora UI build or it is outdated",
                dist.join("index.html").display()
            ));
        }
    }

    let has_assets = std::fs::read_dir(dist.join("assets"))
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false);

    if !has_assets {
        fail(&format!(
            "`{}` is missing or empty",
            dist.join("assets").display()
        ));
    }
}

fn fail(reason: &str) -> ! {
    eprintln!(
        "error: {reason}\n\n\
         The ora-ui crate embeds the built Ora web UI.\n\
         Build it by running `cargo xtask ui build` in the Ora repository."
    );
    std::process::exit(1);
}
