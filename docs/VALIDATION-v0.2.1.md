# v0.2.1 local validation — 2026-09-06

Historical measurement status: **local review candidate on 2026-09-06**. Based on public main
04a4d21f5cf9ac77adec5a47b1d0c452bbf5a117. No private encoder or internal
v5.12.1 module is included. The earlier verified-range experiment is excluded.

## Decision

The release has measurable reader value: less repeated counting and less
Top-K selection work, with unchanged exact results. This is not a new semantic
encoder, a compression breakthrough, or proof of 99% knowledge retrieval.

- Full local portable and x86-64-v3 verification gates: **79 tests passed per
  profile**, no failures; formatting, Clippy, rustdoc, build, selftest and
  benchmark smoke checks passed.
- Structural audit: 262400/262400 cases exact per profile.
- Independent per-bit ranking audit: 256/256 queries exact per profile.
- Persistence: 16, 256 and 512 MiB banks exact per profile; default budget
  correctly rejects 512 MiB and the explicit budget accepts it.
- Final Top-K comparison: 6240 method results (including warmups) match the
  frozen full-scan reference; progressive stage counts match too.
- Natural-text retrieval target 0.99: **NOT MET** by the evaluated lexical
  baseline. This is separate from byte and binary-ranking integrity.

Finite tests establish results for those inputs, not universal or physical
fault guarantees. Linux was tested locally. Subsequent publication check:
[PR #3](https://github.com/gelramlicensing-wq/gel-ram/pull/3) merged the implementation
as 95c7dab. Its [post-merge CI](https://github.com/gelramlicensing-wq/gel-ram/actions/runs/34032946492)
passed Linux verification and the synthetic integrity audit, plus fresh
Windows/macOS compilation checks. The latter do not establish runtime test
coverage on Windows/macOS. The local measurements below are unchanged.
Local logs: [portable verification](evidence-v0.2.1/final-verify-portable.txt)
and [v3 verification](evidence-v0.2.1/final-verify-v3.txt). Absolute checkout
paths in these logs are replaced by {CHECKOUT}; test results are unchanged.
The updated Linux CI job also runs the independent synthetic integrity audit;
no private external input is required by CI.

## Hardware and measurement

AMD Ryzen AI 9 HX 370, 12 cores / 24 threads, 93 GiB RAM, Linux
6.17.0-1030-oem. Rust 1.85.0, LLVM 19.1.7, release profile; portable defaults
and explicit -C target-cpu=x86-64-v3. Benchmarks pinned to CPU 2 in the 16 MiB
L3 domain; governor powersave, EPP balance_performance, boost enabled.
No system tuning, GPU/NPU use or cloud execution. Shared, non-isolated host;
pre-existing swap usage was about 16 GiB, available RAM about 68 GiB.
Initial load1 was 3.68; final-instrument sweep began at 11:41:18 UTC, load1 2.67.
Background load and power management can affect timing.

The [protocol](TOPK-BENCHMARK.md) was recorded before measurement.
Three trials of eleven measured queries and two warmups each; all four
methods rotate order. Allocation is timed; data generation, correctness
checks and printing are not. These are warm-RAM latency medians, not reliable
tail estimates, sustained DRAM bandwidth, cold-device I/O or inference speed.

## Full planned performance matrix

Each cell is the minimum–maximum of the three trial ratios:
old median latency / new median latency. Above 1 is faster; below 1 is slower.
Do not quote the best row as a speedup of the entire system.

| Bank / queries | Profile | K | Full old/new | Progressive old/new |
|---|---|---:|---:|---:|
| uniform / synthetic | portable | 1 | 0.94–1.13× | 1.04–1.37× |
| uniform / synthetic | portable | 8 | 1.27–1.33× | 1.26–1.29× |
| uniform / synthetic | portable | 32 | 2.67–3.24× | 2.91–3.21× |
| uniform / synthetic | portable | 256 | 16.02–16.50× | 14.97–15.74× |
| clustered / synthetic | portable | 1 | 0.99–1.00× | 0.98–1.24× |
| clustered / synthetic | portable | 8 | 1.15–1.28× | 1.05–1.19× |
| clustered / synthetic | portable | 32 | 3.11–3.50× | 1.24–1.71× |
| clustered / synthetic | portable | 256 | 14.09–16.82× | 2.24–3.96× |
| ties / synthetic | portable | 1 | 1.00–1.01× | 1.17–1.19× |
| ties / synthetic | portable | 8 | 1.22–1.27× | 1.27–1.30× |
| ties / synthetic | portable | 32 | 2.20–2.52× | 2.38–2.52× |
| ties / synthetic | portable | 256 | 13.87–16.99× | 12.74–15.48× |
| uniform / synthetic | x86-64-v3 | 1 | 1.08–1.10× | 1.10–1.19× |
| uniform / synthetic | x86-64-v3 | 8 | 1.49–1.92× | 1.54–1.80× |
| uniform / synthetic | x86-64-v3 | 32 | 3.67–5.63× | 3.90–4.81× |
| uniform / synthetic | x86-64-v3 | 256 | 17.99–21.99× | 16.12–22.24× |
| clustered / synthetic | x86-64-v3 | 1 | 0.98–1.07× | 1.10–1.32× |
| clustered / synthetic | x86-64-v3 | 8 | 1.73–1.84× | 1.02–1.18× |
| clustered / synthetic | x86-64-v3 | 32 | 4.57–5.82× | 1.40–1.46× |
| clustered / synthetic | x86-64-v3 | 256 | 20.30–27.98× | 2.80–3.09× |
| ties / synthetic | x86-64-v3 | 1 | 0.79–1.20× | 1.06–1.15× |
| ties / synthetic | x86-64-v3 | 8 | 1.96–2.21× | 1.82–1.86× |
| ties / synthetic | x86-64-v3 | 32 | 3.43–5.30× | 3.43–5.06× |
| ties / synthetic | x86-64-v3 | 256 | 26.98–30.96× | 24.61–26.49× |
| 509 text ORBs / heldout text | portable | 1 | 1.00–1.00× | 1.17–1.17× |
| 509 text ORBs / heldout text | portable | 8 | 1.18–1.22× | 1.26–1.30× |
| 509 text ORBs / heldout text | portable | 32 | 1.83–1.96× | 1.86–1.98× |
| 509 text ORBs / heldout text | portable | 256 | 1.20–1.31× | 1.20–1.31× |
| 509 text ORBs / heldout text | x86-64-v3 | 1 | 1.01–1.04× | 1.14–1.17× |
| 509 text ORBs / heldout text | x86-64-v3 | 8 | 1.45–1.54× | 1.51–1.63× |
| 509 text ORBs / heldout text | x86-64-v3 | 32 | 1.78–2.35× | 1.59–2.51× |
| 509 text ORBs / heldout text | x86-64-v3 | 256 | 1.49–1.54× | 1.52–1.54× |
| 628 text ORBs / sibling text | portable | 1 | 0.99–1.02× | 1.13–1.15× |
| 628 text ORBs / sibling text | portable | 8 | 1.06–1.16× | 1.20–1.22× |
| 628 text ORBs / sibling text | portable | 32 | 1.80–1.83× | 1.87–1.92× |
| 628 text ORBs / sibling text | portable | 256 | 1.43–1.57× | 1.45–1.57× |
| 628 text ORBs / sibling text | x86-64-v3 | 1 | 1.00–1.22× | 1.10–1.41× |
| 628 text ORBs / sibling text | x86-64-v3 | 8 | 1.31–1.39× | 1.41–1.42× |
| 628 text ORBs / sibling text | x86-64-v3 | 32 | 2.04–2.10× | 2.14–2.23× |
| 628 text ORBs / sibling text | x86-64-v3 | 256 | 1.48–1.56× | 1.51–1.57× |

The synthetic banks each contain 131072 ORBs / 16 MiB. Text banks are much
smaller (509 and 628 ORBs). The final text runs use separately encoded heldout
text queries, not exact self-queries. Their labels are not used by the timing
instrument. The external samples are supplementary local evidence: private
source text, encoders and banks are not in this distribution. The synthetic
benchmark can be reproduced from a clean checkout.

There is **no general K=1 speedup claim**. For example, the final v3 all-tied
full Top-1 row ranges from 0.79 to 1.20× (one trial about 27% slower).
Small samples on a loaded host cannot establish a stable regression magnitude.
All K values and both profiles are retained; no correctness threshold was
relaxed to obtain performance.

Raw final logs are in [portable](evidence-v0.2.1/final-topk-portable.txt),
[v3](evidence-v0.2.1/final-topk-v3.txt), and the four final-topk text-query files
in the same evidence directory. Earlier topk files are retained too: they
precede the optional explicit-query input; their external runs used natural
banks with synthetic queries. They are not silently substituted for the final
natural-query measurements. Benchmark source and frozen reference are shipped.

## FP32 / FP16 / Q8 fidelity

All three representations were separately round-tripped through GEL. **Every
encoded byte was recovered exactly**, including FP16/Q8 bytes. Conversion loss
below happens before storage, not during GEL readout.

A caller-supplied archived dump has 299 rows × 1024 FP32 values = 306176
values. Its complete container is 1225908 bytes, CRC64 cf680585b0473140.
Provenance is archived/caller-supplied, not freshly regenerated model inference.
The conversion tool validates shape and finite-value range before comparison.
The full-size original FP32 representation is the GEL exact control; it is
not a claimed compact replacement for FP16/Q8.

| Representation inside GEL | Numeric bytes (without GEL header) | Relative L2 error vs original | RMSE |
|---|---:|---:|---:|
| Original FP32 bits | 1224704 | 0 | 0 |
| IEEE binary16 nearest-even | 612352 | 0.0002091264554 | 0.0007882930071 |
| Reference Q8 symmetric blocks of 32, FP32 scale | 344448 | 0.008382116999 | 0.03159602262 |

GEL files add a 64-byte header in these three aligned cases. Q8 byte counts
include scales; this is not GGUF Q8_0, GPTQ, AWQ or an optimized compute kernel.
The raw FP32 control uses more bytes because it retains all input bits.
No single invented quality score, semantic accuracy or quantization speed
follows from these numbers. Both hardware profiles produced the same numeric
metrics. All four synthetic datasets, including outliers and signed zeros,
are retained in [portable audit](evidence-v0.2.1/integrity-portable.txt) and
[v3 audit](evidence-v0.2.1/integrity-v3.txt).
See [input contract and reproduction](DATA-INTEGRITY.md).

## Natural-text knowledge gate — not passed

The preserved Wikipedia EN sample comprises the first 1000 eligible nodes,
not a random or representative benchmark. A local gel-lexical-ri-v1 baseline
encodes lexical features into ORBs. Documents exclude the selected query
segment (heldout task), or use other sections of the same node (sibling task).
Two hundred deterministic query targets per task were evaluated against the
entire corresponding bank. No query labels enter the encoder.

| Task | Bank | Recall@1 | Recall@5 | Recall@10 |
|---|---:|---:|---:|---:|
| Heldout segment | 509 | 46/200 = 0.230 | 81/200 = 0.405 | 96/200 = 0.480 |
| Sibling section | 628 | 24/200 = 0.120 | 43/200 = 0.215 | 54/200 = 0.270 |

Persisted and reconstructed ORBs were exact; progressive/full rankings agreed.
Thus making this reader faster does not repair the encoder's retrieval quality.
[Local evaluation output](evidence-v0.2.1/wiki-evaluation-queries.txt) includes
corpus fingerprints. This is a limited article-association task, not an
end-to-end question-answering evaluation. **Do not publish a 99% knowledge
claim or mark the full AI knowledge-bank milestone complete.**

## Release boundary

Ready for source review as a binary-reader/integrity candidate, not for
unconditional release approval. Next steps are fresh remote CI, independent
review, and explicit maintainer approval. Do not include local model dumps,
encoded private banks, credentials, historical experimental trees, build
outputs or the rejected range-read prototype. Licensing documents are unchanged.
