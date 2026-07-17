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
}
