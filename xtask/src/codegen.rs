use std::{fs::create_dir_all, path::Path};

use walkdir::WalkDir;

pub(crate) fn generate_proto() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .unwrap();
    let proto_root = workspace_root.join("proto");

    let proto_paths: Vec<_> = WalkDir::new(&proto_root)
        .sort_by_file_name()
        .into_iter()
        .filter_map(|e| {
            let e = e.ok()?;

            if e.file_type().is_dir() {
                return None;
            }

            if e.path().extension()? != "proto" {
                return None;
            }

            Some(e.into_path())
        })
        .collect();

    let includes = &[proto_root];

    {
        let out_dir = workspace_root.join("crates/ora-server/src/proto/generated");
        create_dir_all(&out_dir).unwrap();

        let mut config = tonic_prost_build::Config::new();
        config.enable_type_names();

        tonic_prost_build::configure()
            .build_transport(false)
            .build_client(false)
            .build_server(true)
            .emit_rerun_if_changed(false)
            .generate_default_stubs(true)
            .bytes(".")
            .out_dir(out_dir)
            .compile_with_config(config, &proto_paths, includes)
            .unwrap();
    }

    {
        let out_dir = workspace_root.join("crates/ora/src/proto/generated");

        create_dir_all(&out_dir).unwrap();

        let mut config = tonic_prost_build::Config::new();
        config.enable_type_names();

        tonic_prost_build::configure()
            .build_transport(false)
            .build_client(true)
            .build_server(false)
            .emit_rerun_if_changed(false)
            .bytes(".")
            .out_dir(out_dir)
            .compile_with_config(config, &proto_paths, includes)
            .unwrap();
    }
}
