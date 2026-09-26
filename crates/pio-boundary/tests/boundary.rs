//! Every crate on the public wire, held to the boundary (see the crate's
//! documentation for the four checks). A new public-wire crate, such as
//! the screen's, is one more row in `CRATES`.
use pio_boundary::{
    PublicWire, check, compiled_sources, dep_info_files, forbidden_reach, metadata,
    outside_allow_list, symlinks_under, workspace_root,
};

const CRATES: [PublicWire; 2] = [
    PublicWire {
        name: "pio-client",
        dir: "crates/pio-client",
        allowed: &[
            "anyhow",
            "base64",
            "libc",
            "serde",
            "serde_json",
            "sha2",
            "uuid",
        ],
        internal: &[],
    },
    PublicWire {
        name: "pio-client-cli",
        dir: "crates/pio-client-cli",
        allowed: &["anyhow", "base64", "libc", "rusqlite", "serde_json", "uuid"],
        internal: &["pio-client"],
    },
];

fn holds(name: &str) {
    let root = workspace_root();
    let metadata = metadata(&root).expect("cargo metadata");
    // The graph is the real one: pio-cli is in it and does reach the
    // service, so a checker that saw nothing would fail here first.
    assert!(!forbidden_reach(&metadata, "pio-cli").is_empty());
    let wire = CRATES.iter().find(|c| c.name == name).unwrap();
    let broken = check(&root, &metadata, wire);
    assert!(
        broken.is_empty(),
        "the public-wire boundary does not hold:\n{}",
        broken.join("\n")
    );
    // And the compiler's list was the real one: the crate's own root is in
    // it, so an empty or unrelated list cannot pass for a clean one.
    let sources = compiled_sources(&root, &metadata, wire).expect("the crate builds");
    let lib = root
        .join(wire.dir)
        .join("src/lib.rs")
        .canonicalize()
        .unwrap();
    assert!(sources.contains(&lib), "{sources:?}");
}

#[test]
fn pio_client_holds_the_boundary() {
    holds("pio-client");
}

#[test]
fn pio_client_cli_holds_the_boundary() {
    holds("pio-client-cli");
}

/// The checkers themselves, where each must fire and where it must not.
#[test]
fn each_check_fires_on_what_it_forbids() {
    let graph = |kind: &str| {
        serde_json::json!({
            "packages": [
                {"id": "c", "name": "pio-client", "dependencies": [
                    {"name": "middle", "kind": null, "path": "/x/middle"},
                    {"name": "pio-client", "kind": null, "path": "/x/pio-client"},
                    {"name": "serde_json", "kind": null}]},
                {"id": "m", "name": "middle"},
                {"id": "k", "name": "pio-core"}],
            "resolve": {"nodes": [
                {"id": "c", "deps": [{"pkg": "m", "dep_kinds": [{"kind": kind}]}]},
                {"id": "m", "deps": [{"pkg": "k", "dep_kinds": [{"kind": null}]}]},
                {"id": "k", "deps": []}]}})
    };
    assert_eq!(forbidden_reach(&graph("build"), "pio-client"), ["pio-core"]);
    assert!(forbidden_reach(&graph("dev"), "pio-client").is_empty());
    let wire = PublicWire {
        name: "pio-client",
        dir: "crates/pio-client",
        allowed: &["serde_json"],
        internal: &["pio-client"],
    };
    assert_eq!(
        outside_allow_list(&graph("build"), &wire),
        ["middle (by path)"]
    );

    let dep_info = "/t/deps/libx-1.rmeta: /w/a\\ b/src/lib.rs /w/a\\ b/src/m.rs\n\n\
                    /w/a\\ b/src/lib.rs:\n/w/a\\ b/src/m.rs:\n\n# env-dep:CARGO_PKG_NAME=x\n";
    assert_eq!(
        dep_info_files(dep_info),
        [
            std::path::PathBuf::from("/w/a b/src/lib.rs"),
            std::path::PathBuf::from("/w/a b/src/m.rs")
        ]
    );

    let dir = std::env::temp_dir().join(format!("pio-boundary-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    assert!(symlinks_under(&dir).is_empty());
    std::os::unix::fs::symlink("/etc/hosts", dir.join("src/leak.rs")).unwrap();
    assert_eq!(symlinks_under(&dir), [dir.join("src/leak.rs")]);
    std::fs::remove_dir_all(&dir).unwrap();
}
