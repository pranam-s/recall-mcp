//! Release metadata drift guards. These files live at the repository root and
//! are easy to forget during a version bump; the test fails when they disagree
//! with the compiled crate version.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(relative: &str) -> String {
    let path = repo_root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn readme_has_contents_and_no_internal_ci_notes() {
    let readme = read("README.md");
    assert!(
        readme.contains("## Contents"),
        "README must have a Contents section"
    );
    for banned in [
        "GitHub Actions",
        "billing",
        "owner-gated",
        "development machine",
        "dev machine",
    ] {
        assert!(
            !readme.contains(banned),
            "README must not carry internal CI or host notes: {banned:?}"
        );
    }
}

#[test]
fn version_is_consistent_across_release_metadata() {
    let version = env!("CARGO_PKG_VERSION");
    assert_eq!(version, "0.5.0", "update this guard with the release");

    let server: serde_json::Value =
        serde_json::from_str(&read("server.json")).expect("server.json parses");
    assert_eq!(server["version"], version);
    assert_eq!(server["name"], "io.github.pranam-s/recall-mcp");
    // The registry schema caps `description` at 100 characters.
    assert!(
        server["description"]
            .as_str()
            .is_some_and(|d| !d.is_empty() && d.chars().count() <= 100),
        "server.json description must be 1..=100 characters: {}",
        server["description"]
    );
    let packages = server["packages"].as_array().expect("packages array");
    // An OCI package is optional until the image is published. When one is
    // present, its identifier must carry the tag the image is pushed under.
    if let Some(oci) = packages
        .iter()
        .find(|package| package["registryType"] == "oci")
    {
        let expected_identifier = format!("ghcr.io/pranam-s/recall-mcp:{version}");
        assert_eq!(
            oci["identifier"].as_str(),
            Some(expected_identifier.as_str()),
            "the OCI identifier must carry the release tag the image is pushed under"
        );
        assert_eq!(oci["transport"]["type"], "stdio");
        assert_eq!(oci["runtimeHint"], "docker");
        let oci_args = oci["packageArguments"].as_array().expect("OCI args array");
        assert!(
            oci_args.iter().any(|a| a["value"] == "serve")
                && oci_args.iter().any(|a| a["name"] == "--stdio"),
            "the advertised OCI stdio package must run `serve --stdio`: {oci_args:?}"
        );
    }
    let cargo = packages
        .iter()
        .find(|package| package["registryType"] == "cargo")
        .expect("a cargo package");
    assert_eq!(cargo["identifier"], "recall-mcp-cli");
    assert_eq!(cargo["version"], version);
    assert_eq!(cargo["transport"]["type"], "stdio");

    let citation = read("CITATION.cff");
    assert!(
        citation.contains(&format!("version: {version}")),
        "CITATION.cff version must match {version}"
    );
    assert!(
        citation.contains("identifiers:"),
        "CITATION.cff must use the identifiers array form"
    );

    let readme = read("README.md");
    assert!(
        readme.contains("mcp-name: io.github.pranam-s/recall-mcp"),
        "README must expose the registry mcp-name line"
    );
    assert!(
        readme.contains(&format!("Status: **{version}")),
        "README status must name {version}"
    );
    assert!(
        repo_root()
            .join(format!("releases/NOTES-{version}.md"))
            .exists(),
        "release notes for {version} must exist"
    );
}

#[test]
fn mcp_surface_keeps_the_six_documented_tools() {
    let source = read("crates/recall-server/src/mcp.rs");
    for tool in [
        "fn remember",
        "fn recall",
        "fn forget",
        "fn update_memory",
        "fn list_memories",
        "fn capture",
    ] {
        assert!(source.contains(tool), "missing MCP tool {tool}");
    }
    let openapi = read("crates/recall-server/src/openapi.json");
    for tool in [
        "remember",
        "recall",
        "forget",
        "update_memory",
        "list_memories",
        "capture",
    ] {
        assert!(
            openapi.contains(tool),
            "OpenAPI MCP description must name {tool}"
        );
    }
    assert!(openapi.contains("bearerAuth"));
    assert!(openapi.contains("apiKeyAuth"));
}
