//! End-to-end CLI tests: remember -> recall -> export -> import round trip
//! through the real binary, exactly as a script would drive it.

use std::process::Command;

use serde_json::Value;

struct Output {
    stdout: String,
    status: std::process::ExitStatus,
}

fn run(db: &str, args: &[&str]) -> Output {
    let out = Command::new(env!("CARGO_BIN_EXE_recall-cli"))
        .arg("--db")
        .arg(db)
        .args(args)
        .output()
        .expect("run recall-cli");
    Output {
        stdout: String::from_utf8(out.stdout).expect("utf8 stdout"),
        status: out.status,
    }
}

#[test]
fn cli_remember_recall_export_import_roundtrip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("e2e.db");
    let db_str = db.to_str().unwrap();

    // remember (auto-creates namespace)
    let out = run(
        db_str,
        &[
            "remember",
            "--namespace",
            "e2e",
            "--text",
            "cargo test drives the cli",
            "--tags",
            "test,cli",
        ],
    );
    assert!(out.status.success(), "remember failed: {}", out.stdout);
    let memory: Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(memory["text"], "cargo test drives the cli");
    assert_eq!(memory["source"], "cli");

    // recall finds it with a breakdown
    let out = run(
        db_str,
        &[
            "recall",
            "--namespace",
            "e2e",
            "--query",
            "cli test",
            "--k",
            "3",
        ],
    );
    assert!(out.status.success(), "recall failed: {}", out.stdout);
    let hits: Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(hits.as_array().unwrap().len(), 1);
    assert_eq!(hits[0]["breakdown"]["total"].as_f64().unwrap() > 0.0, true);

    // export to file
    let export_path = dir.path().join("export.json");
    let out = run(db_str, &["export", "--out", export_path.to_str().unwrap()]);
    assert!(out.status.success(), "export failed: {}", out.stdout);
    assert!(export_path.exists());

    // import into a fresh db, skipping nothing
    let db2 = dir.path().join("e2e-2.db");
    let out = run(
        db2.to_str().unwrap(),
        &["import", "--file", export_path.to_str().unwrap()],
    );
    assert!(out.status.success(), "import failed: {}", out.stdout);
    let report: Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(report["memories_inserted"], 1);
    assert_eq!(report["namespaces_created"], 1);

    // recall in the imported db finds the memory
    let out = run(
        db2.to_str().unwrap(),
        &["recall", "--namespace", "e2e", "--query", "cargo cli"],
    );
    assert!(out.status.success());
    let hits: Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(hits.as_array().unwrap().len(), 1);
}

#[test]
fn cli_rejects_unknown_namespace_on_strict_paths_and_bad_queries() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("neg.db");

    // recall on unknown namespace errors (non-zero exit)
    let out = run(
        db.to_str().unwrap(),
        &["recall", "--namespace", "ghost", "--query", "x"],
    );
    assert!(!out.status.success());

    // blank text is rejected
    let out = run(
        db.to_str().unwrap(),
        &["remember", "--namespace", "n", "--text", "   "],
    );
    assert!(!out.status.success());
}
