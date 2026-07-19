fn main() {
    let spec = trust_deeds::generated_openapi();
    let json = spec.to_pretty_json().expect("serialize OpenAPI document");
    let output = concat!(env!("CARGO_MANIFEST_DIR"), "/client/openapi.json");
    std::fs::write(output, json).expect("write client/openapi.json");
    eprintln!("wrote {output}");
}
