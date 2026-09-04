/// manifest.json and Cargo.toml versions are bumped together; the freshness
/// check trusts the manifest. One fails-to-match and prebuilt matching breaks
/// silently, so pin them.
#[test]
fn manifest_and_cargo_versions_match() {
    let manifest_raw =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../manifest.json"))
            .expect("read manifest.json");
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest_raw).expect("parse manifest.json");
    let manifest_version = manifest["version"]
        .as_str()
        .expect("manifest version is a string");

    let cargo_toml = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("read Cargo.toml");
    let cargo_version = cargo_toml
        .lines()
        .find_map(|line| line.strip_prefix("version = "))
        .map(|v| v.trim_matches('"'))
        .expect("Cargo.toml has a version line");

    assert_eq!(
        manifest_version, cargo_version,
        "manifest.json and daemon/Cargo.toml must be bumped together"
    );
}
