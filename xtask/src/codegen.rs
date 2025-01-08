use std::path::Path;

pub(crate) fn generate_proto() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .unwrap();
    let proto_root = workspace_root.join("proto");
    let out_dir = workspace_root.join("crates/ora-proto/src/generated");

    let proto_paths = [
        proto_root.join("ora/common/v1/job.proto"),
        proto_root.join("ora/common/v1/schedule.proto"),
        proto_root.join("ora/common/v1/time_range.proto"),
        //
        proto_root.join("ora/server/v1/executor.proto"),
        proto_root.join("ora/server/v1/admin.proto"),
        //
        proto_root.join("ora/snapshot/v1/service.proto"),
    ];

    let mut config = prost_build::Config::new();
    config.enable_type_names();

    tonic_build::configure()
        .build_transport(true)
        .build_client(true)
        .build_server(true)
        .emit_rerun_if_changed(false)
        .generate_default_stubs(true)
        .bytes(["."])
        .out_dir(out_dir)
        .compile_protos_with_config(config, &proto_paths, &[&proto_root])
        .unwrap();
}
