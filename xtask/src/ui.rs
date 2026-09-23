use std::{path::Path, process::Command};

/// Builds the web UI into the `ora-ui` crate, so that it can be embedded.
pub(crate) fn build() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .unwrap();
    let ui_dir = workspace_root.join("ui");
    let out_dir = workspace_root.join("crates/ora-ui/dist");

    run(&ui_dir, &["install", "--frozen-lockfile"]);
    run(&ui_dir, &["exec", "vue-tsc", "-b"]);
    run(
        &ui_dir,
        &[
            "exec",
            "vite",
            "build",
            "--outDir",
            out_dir.to_str().unwrap(),
            "--emptyOutDir",
        ],
    );
}

fn run(dir: &Path, args: &[&str]) {
    let status = Command::new("pnpm")
        .args(args)
        .current_dir(dir)
        .status()
        .expect("failed to run pnpm, is it installed?");

    assert!(status.success(), "`pnpm {}` failed", args.join(" "));
}
