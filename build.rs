fn main() {
    nextrs::build::emit_registry("app", "src/lib.rs", "nextrs_routes.rs")
        .expect("nextrs::build::emit_registry failed");
    nextrs::build::emit_seeds("app", "nextrs_seeds.rs").expect("nextrs::build::emit_seeds failed");
    nextrs::bundle::bundle_pages(&nextrs::bundle::BundleConfig {
        app_dir: "app",
        client_dir: "client",
        client_alias: "@trust-deeds/client",
        public_dist: "public/dist",
        ..Default::default()
    })
    .expect("nextrs::bundle::bundle_pages failed");
    inject_mobile_viewport().expect("failed to add viewport metadata to NextRS shells");
}

fn inject_mobile_viewport() -> std::io::Result<()> {
    // NextRS 0.3.8 emits client-only page fragments. React eventually hoists the
    // layout's metadata, but mobile browsers need the viewport directive before
    // the app bundle runs so their initial layout viewport is device-width.
    let output = std::path::PathBuf::from(
        std::env::var_os("OUT_DIR").expect("OUT_DIR must be set for build scripts"),
    )
    .join("nextrs_assets.rs");
    let source = std::fs::read_to_string(&output)?;
    let root_marker = r#"<div id=\"__nx_"#;
    let viewport = r#"<meta name=\"viewport\" content=\"width=device-width, initial-scale=1, viewport-fit=cover\" />"#;
    let shell_count = source.matches(root_marker).count();

    if shell_count == 0 {
        return Err(std::io::Error::other(
            "NextRS asset module did not contain a client root marker",
        ));
    }

    let patched = source.replace(root_marker, &format!("{viewport}{root_marker}"));
    if patched != source {
        std::fs::write(output, patched)?;
    }
    Ok(())
}
