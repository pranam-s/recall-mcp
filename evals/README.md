# Retrieval evals

These fixtures measure whether Recall-MCP returns the memory an operator would
pick. They are project-local: no external benchmark (LoCoMo, LongMemEval, or any
other) is claimed or reproduced here.

## Files

| File | What it is |
|---|---|
| `corpus.json` | 100 memories across three namespaces (`product`, `ops`, `research`), with tags and per-memory `age_days` used by temporal cases. |
| `golden.json` | 100 labelled queries: 50 lexical, 30 tag-scoped, 20 temporal. Gate: hit-rate@5 >= 0.85. MRR@10 is reported and gated at >= 0.80. |
| `semantic.json` | 50 paraphrase queries over the same corpus. With the `onnx` feature this is a gated run (hit-rate@5 >= 0.85); the default hash embedder prints a diagnostic and does not gate. |

`crates/recall-core/tests/golden_eval.rs` seeds the corpus verbatim and runs
every query through the real hybrid recall path with default weights and
`k=10`, so MRR@10 sees ten results while the hit-rate gate still reads the
first five.

## Labelling rules

1. One row has one expected id. A query may be satisfied by several corpus
   memories in principle; the label records the intended target, and the metric
   counts only that id.
2. Lexical queries reuse two or three distinctive tokens from the target text.
3. Tag-scoped cases deliberately include a sibling memory that matches the
   query tokens but not the required tag. Only the tag filter can pick the
   target, so the case tests filtering rather than ranking.
4. Temporal cases duplicate the target text under an older id. Identical text
   means identical lexical and vector scores; recency alone must put the newer
   record first.
5. Semantic queries are paraphrases with little or no exact token overlap. They
   are labelled by meaning, not by matching words.
6. Do not edit a query to chase a score without writing down why the old label
   was wrong. A label change is a test change and needs the same review as code.

## Method and recorded results

Measured on 2026-11-08 in this worktree with `cargo test`. Exact output:

```text
golden: 98/100 hit-rate@5 = 0.980 (gate >= 0.85), MRR@10 = 0.975
  embedder: hash-256
semantic (diagnostic): 31/50 hit-rate@5 = 0.620 (gate >= 0.85), MRR@10 = 0.508
  embedder: hash-256
semantic: 50/50 hit-rate@5 = 1.000 (gate >= 0.85), MRR@10 = 0.740
  embedder: onnx:all-minilm-l6-v2-q
```

The ONNX run used fastembed 7.6.1 with onnxruntime 1.28.0 (`ORT_DYLIB_PATH`),
one test thread, and a fresh model cache, on windows-msvc.

The hash diagnostic shows the expected limitation of lexical hashing on
paraphrases. It is not a failure: ONNX owns the semantic gate.

See [../docs/EVALUATION.md](../docs/EVALUATION.md) for the rest of the measured
gates (tests, coverage, p95, accessibility, security).
