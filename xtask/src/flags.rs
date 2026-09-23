xflags::xflags! {
    cmd xtask {
        cmd codegen {
            cmd proto {}
        }
        cmd ui {
            /// Build the web UI and place the assets in the `ora-ui` crate.
            cmd build {}
        }
    }
}
