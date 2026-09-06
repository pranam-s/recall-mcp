//! Exact recall-latency percentiles on the 10k-memory fixture (PRD gate:
//! p95 < 25 ms). Ignored by default because it seeds a large store; run with
//!
//! ```text
//! cargo test -p recall-core --release --test p95_latency -- --ignored --nocapture
//! ```
//!
//! Release mode is required: the metric is the production latency budget and
//! debug-build numbers are not meaningful. The timing covers the full
//! `Store::recall` call (FTS pass + brute-force cosine scan + scoring +
//! last_accessed refresh); query embedding happens before the timed region
//! and is reported separately for context.

use std::time::Instant;

use recall_core::{
    Embedder as _, HashEmbedder, NewMemory, RecallParams, SqliteStore, Store, unix_now,
};

const CORPUS_SIZE: usize = 10_000;
const QUERY_COUNT: usize = 1_000;
/// PRD success metric (docs/PRD.md): p95 recall < 25 ms on 10k memories.
const P95_BUDGET_MS: f64 = 25.0;

fn percentile(sorted: &[f64], p: f64) -> f64 {
    let idx = ((p / 100.0) * (sorted.len() as f64 - 1.0)).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

#[test]
#[ignore = "seeds a 10k-memory store; run with --ignored (release mode)"]
fn p95_recall_latency_on_10k_memories_stays_under_budget() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = SqliteStore::open(dir.path().join("p95.db")).expect("open store");
    let embedder = HashEmbedder::new_256();
    store.create_namespace("bench").expect("namespace");

    let seed_start = Instant::now();
    for i in 0..CORPUS_SIZE {
        let topics = [
            "deploy",
            "database",
            "incident review",
            "design decision",
            "meeting notes",
            "onboarding",
            "performance",
            "security audit",
            "release checklist",
            "customer feedback",
        ];
        let text = format!(
            "{} note {}: follow up with the platform team about {}",
            topics[i % topics.len()],
            i,
            topics[(i * 7) % topics.len()]
        );
        let embedding = embedder.embed(&text);
        store
            .insert_memory(&NewMemory {
                namespace: "bench".into(),
                text,
                tags: vec![format!("topic-{}", i % 20)],
                source: Some("bench".into()),
                pinned: i % 500 == 0,
                created_at: Some(unix_now() - (i as i64 % 3600)),
                id: None,
                embedding: Some(embedding),
            })
            .expect("insert");
    }
    eprintln!(
        "seeded {CORPUS_SIZE} memories in {:?}",
        seed_start.elapsed()
    );

    let params = RecallParams::default();
    let mut latencies = Vec::with_capacity(QUERY_COUNT);
    let mut total_hits = 0usize;
    let mut embed_total = std::time::Duration::ZERO;
    for i in 0..QUERY_COUNT {
        let query = format!("incident review note {} root cause", i % 250);
        let t_embed = Instant::now();
        let q = embedder.embed(&query);
        embed_total += t_embed.elapsed();

        let t0 = Instant::now();
        let hits = store
            .recall("bench", &query, Some(&q), &params, unix_now())
            .expect("recall");
        let elapsed = t0.elapsed();
        assert!(hits.len() <= params.k);
        total_hits += hits.len();
        latencies.push(elapsed.as_secs_f64() * 1_000.0);
    }

    latencies.sort_by(|a, b| a.partial_cmp(b).expect("finite latencies"));
    let p50 = percentile(&latencies, 50.0);
    let p95 = percentile(&latencies, 95.0);
    let p99 = percentile(&latencies, 99.0);
    let max = latencies[latencies.len() - 1];
    let mean = latencies.iter().sum::<f64>() / latencies.len() as f64;
    println!(
        "queries: {QUERY_COUNT}, hits/query avg: {:.1}",
        total_hits as f64 / QUERY_COUNT as f64
    );
    println!(
        "query embed (untimed): {:.3} ms avg",
        embed_total.as_secs_f64() * 1_000.0 / QUERY_COUNT as f64
    );
    println!(
        "recall latency ms: mean {mean:.3} | p50 {p50:.3} | p95 {p95:.3} | p99 {p99:.3} | max {max:.3}"
    );
    assert!(
        p95 < P95_BUDGET_MS,
        "p95 recall latency {p95:.3} ms breached the {P95_BUDGET_MS} ms PRD budget"
    );
}
