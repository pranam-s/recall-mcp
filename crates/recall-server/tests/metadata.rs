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
fn version_is_consistent_across_release_metadata() {
    let version = env!("CARGO_PKG_VERSION");
    assert_eq!(version, "0.5.0", "update this guard with the release");

    let server: serde_json::Value =
        serde_json::from_str(&read("server.json")).expect("server.json parses");
    assert_eq!(server["version"], version);
    assert_eq!(server["name"], "io.github.pranam-s/recall-mcp");

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
