# Exact Top-K comparison protocol

Local candidate; not a semantic knowledge-retrieval result. Protocol recorded
before measurement on 2026-09-06. The candidate avoids rescoring prefix words
and rejects noncompetitive candidates before scanning the sorted Top-K list.
Scores, tie ordering and strict progressive pruning must remain unchanged.

## Acceptance

- Every result must exactly match the frozen v0.2.0 full-scan result (indices,
  scores and ordering). Progressive stage counts must also match v0.2.0.
- Independent per-bit scoring and full sorting in unit tests are a second
  correctness oracle; agreement between two related functions is not enough.
- No weakening of a correctness gate, new lossy approximation, extra index,
  private encoder, dependency, or change to the ORB representation is allowed.
- Report every planned workload, including regressions. Do not select only
  successful datasets, K values, or compilation profiles for a headline.
- A hardware speed ratio is not the user's data-correctness threshold of 0.99.
  Exact ranking requires 1.0; semantic accuracy remains separately unqualified.

## Workloads and execution

Compare frozen v0.2.0 full/progressive functions and candidate full/progressive
functions in one binary, using the same unchanged score kernels and compiler.
The frozen reference is in
`crates/gel-reader/examples/support/v020_topk.rs`.

Use deterministic uniform, clustered and all-tied binary banks, each with
131072 ORBs (16 MiB), K = 1, 8, 32, 256. Query schedule mixes exact bank entries
and perturbed bank entries. Run three trials, each with two warmup queries and
eleven measured queries; rotate execution order of all four methods per query.
Report per-query samples and medians, not reliable p99 from eleven samples.
Generation, oracle validation and printing are outside the timed regions;
result allocation is inside. Input data are resident in RAM. No cold-storage,
energy, GPU/NPU or model-inference measurement is implied.

Run the same protocol in portable and x86-64-v3 profiles. Pin one CPU and record
CPU model, RAM, OS, compiler, flags and load. Optional raw ORB input allows a
separate run on naturally encoded input without bundling private text or encoders.
That optional sample is reported separately; its binary ranking is not a
semantic success metric.

Follow-up protocol: `--raw-queries PATH` together with `--raw-orbs PATH`
uses separately supplied encoded queries, selected by
`(ordinal * 17 + trial * 43) % query_count`. This measures actual encoded
text queries as well as natural banks, without replacing the original
synthetic-query results. Query fingerprints are printed. Labels do not enter
the timing instrument; semantic Recall@K must be evaluated separately.

```text
cargo run --locked --offline --release -p gel-reader --example topk_compare
```

The example also accepts `--orbs COUNT` (1..524288) and `--raw-orbs PATH`.
External files must contain a nonempty multiple of 128 bytes (at most 64 MiB).
Do not upload a private bank to reproduce the synthetic benchmark.
