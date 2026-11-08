//! Retrieval-quality evals.
//!
//! - `golden.json`: 100 labelled queries over `corpus.json` (lexical,
//!   tag-scoped, and temporal cases). Gate: hit-rate@5 >= 0.85.
//! - `semantic.json`: 50 paraphrase queries. The default hash embedder reports
//!   a diagnostic; with the `onnx` feature the semantic gate is enforced.
//!
//! Both report MRR@10 alongside hit-rate@5. No external benchmark is claimed.

use recall_core::{HashEmbedder, NewMemory, RecallParams, SqliteStore, Store, unix_now};
use serde::Deserialize;
use std::path::PathBuf;

#[cfg(feature = "onnx")]
use recall_core::Embedder as _;

fn eval_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../evals")
}

#[derive(Deserialize)]
struct Corpus {
    namespaces: Vec<String>,
    memories: Vec<CorpusMemory>,
}

#[derive(Deserialize)]
struct CorpusMemory {
    id: String,
    namespace: String,
    text: String,
    #[serde(default)]
    tags: Vec<String>,
    /// Age in days relative to the eval clock (temporal cases).
    #[serde(default)]
    age_days: i64,
}

#[derive(Deserialize)]
struct Golden {
    hit_rate_target: f64,
    mrr_target: f64,
    queries: Vec<GoldenQuery>,
}

#[derive(Deserialize)]
struct GoldenQuery {
    namespace: String,
    query: String,
    expect_id: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    case: Option<String>,
}

struct Measurements {
    hit_rate: f64,
    mrr: f64,
    count: usize,
    misses: Vec<String>,
}

fn read_json<T: for<'de> Deserialize<'de>>(name: &str) -> T {
    let path = eval_dir().join(name);
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {name}: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {name}: {e}"))
}

fn seed(store: &SqliteStore, embedder: &dyn recall_core::Embedder, now: i64) -> Corpus {
    let corpus: Corpus = read_json("corpus.json");
    for namespace in &corpus.namespaces {
        store.create_namespace(namespace).expect("create namespace");
    }
    for memory in &corpus.memories {
        store
            .insert_memory(&NewMemory {
                namespace: memory.namespace.clone(),
                text: memory.text.clone(),
                tags: memory.tags.clone(),
                source: Some("eval".to_string()),
                pinned: false,
                created_at: Some(now - memory.age_days * 86_400),
                id: Some(memory.id.clone()),
                embedding: Some(embedder.embed(&memory.text)),
            })
            .expect("insert corpus memory");
    }
    corpus
}

fn measure(
    store: &SqliteStore,
    corpus: &Corpus,
    embedder: &dyn recall_core::Embedder,
    queries: &[GoldenQuery],
    now: i64,
) -> Measurements {
    let corpus_ids: std::collections::HashSet<&str> =
        corpus.memories.iter().map(|m| m.id.as_str()).collect();
    let mut hits = 0usize;
    let mut reciprocal_rank_sum = 0.0;
    let mut misses = Vec::new();
    for q in queries {
        assert!(
            corpus_ids.contains(q.expect_id.as_str()),
            "query {:?} references unknown id {}",
            q.query,
            q.expect_id
        );
        let params = RecallParams {
            tags: q.tags.clone(),
            ..RecallParams::default()
        };
        let query_embedding = embedder.embed(&q.query);
        let results = store
            .recall(&q.namespace, &q.query, Some(&query_embedding), &params, now)
            .expect("recall");
        if results.iter().take(5).any(|h| h.memory.id == q.expect_id) {
            hits += 1;
        } else {
            misses.push(format!(
                "{} -> {} (wanted {}; top: {:?})",
                q.namespace,
                q.query,
                q.expect_id,
                results
                    .iter()
                    .take(5)
                    .map(|h| h.memory.id.as_str())
                    .collect::<Vec<_>>()
            ));
        }
        if let Some(rank) = results
            .iter()
            .take(10)
            .position(|h| h.memory.id == q.expect_id)
        {
            reciprocal_rank_sum += 1.0 / (rank as f64 + 1.0);
        }
    }
    let count = queries.len();
    Measurements {
        hit_rate: hits as f64 / count as f64,
        mrr: reciprocal_rank_sum / count as f64,
        count,
        misses,
    }
}

fn print_report(label: &str, embedder: &str, m: &Measurements, target: f64) {
    println!(
        "{label}: {}/{} hit-rate@5 = {:.3} (gate >= {target:.2}), MRR@10 = {:.3}",
        (m.hit_rate * m.count as f64).round() as usize,
        m.count,
        m.hit_rate,
        m.mrr
    );
    println!("  embedder: {embedder}");
    for miss in &m.misses {
        println!("  miss: {miss}");
    }
}

#[test]
fn golden_set_hit_rate_at_5_meets_target() {
    let golden: Golden = read_json("golden.json");
    assert_eq!(
        golden.queries.len(),
        100,
        "golden set must contain exactly 100 queries"
    );
    assert!(
        (golden.hit_rate_target - 0.85).abs() < f64::EPSILON,
        "gate must stay at the PRD-mandated 0.85"
    );
    // The expanded set must actually cover the three advertised categories.
    for case in ["tag-scoped", "temporal"] {
        assert!(
            golden
                .queries
                .iter()
                .any(|q| q.case.as_deref() == Some(case)),
            "golden set is missing {case} cases"
        );
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let now = unix_now();
    let store = SqliteStore::open(dir.path().join("eval.db")).expect("open store");
    let embedder = HashEmbedder::new_256();
    let corpus = seed(&store, &embedder, now);
    let params = RecallParams::default();
    assert_eq!(
        store.stats().expect("stats").total_memories,
        corpus.memories.len() as u64
    );
    assert_eq!(corpus.memories.len(), 100);
    // Mirror the corpus on disk so a future query edit cannot silently drift.
    assert_eq!(params.k, 5);

    let measured = measure(&store, &corpus, &embedder, &golden.queries, now);
    print_report("golden", "hash-256", &measured, golden.hit_rate_target);
    assert!(
        measured.hit_rate >= golden.hit_rate_target,
        "hit-rate@5 {:.3} below gate {:.2}",
        measured.hit_rate,
        golden.hit_rate_target
    );
    assert!(
        measured.mrr >= golden.mrr_target,
        "MRR@10 {:.3} below target {:.2}",
        measured.mrr,
        golden.mrr_target
    );
}

/// Hash embedder is lexical: the paraphrase set is expected to be weak. This
/// test only validates the fixture and reports the diagnostic number; it does
/// not gate retrieval quality (ONNX owns that gate below).
#[test]
fn semantic_set_reports_hash_diagnostic() {
    let semantic: Golden = read_json("semantic.json");
    assert_eq!(
        semantic.queries.len(),
        50,
        "semantic set must contain exactly 50 queries"
    );
    let dir = tempfile::tempdir().expect("tempdir");
    let now = unix_now();
    let store = SqliteStore::open(dir.path().join("semantic-hash.db")).expect("open store");
    let embedder = HashEmbedder::new_256();
    let corpus = seed(&store, &embedder, now);
    let measured = measure(&store, &corpus, &embedder, &semantic.queries, now);
    print_report(
        "semantic (diagnostic)",
        "hash-256",
        &measured,
        semantic.hit_rate_target,
    );
}

/// The gated semantic run: ONNX must clear the same 0.85 bar on paraphrases.
#[cfg(feature = "onnx")]
#[test]
fn semantic_set_with_onnx_meets_target() {
    let semantic: Golden = read_json("semantic.json");
    let dir = tempfile::tempdir().expect("tempdir");
    let now = unix_now();
    let store = SqliteStore::open(dir.path().join("semantic-onnx.db")).expect("open store");
    let embedder = recall_core::OnnxEmbedder::new().expect("load ONNX model");
    let corpus = seed(&store, &embedder, now);
    let measured = measure(&store, &corpus, &embedder, &semantic.queries, now);
    print_report(
        "semantic",
        embedder.name(),
        &measured,
        semantic.hit_rate_target,
    );
    assert!(
        measured.hit_rate >= semantic.hit_rate_target,
        "ONNX semantic hit-rate@5 {:.3} below gate {:.2}",
        measured.hit_rate,
        semantic.hit_rate_target
    );
    // MRR@10 is reported for the record; the semantic gate is hit-rate@5.
}
