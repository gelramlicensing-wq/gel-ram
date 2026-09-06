# GEL RAM v0.2.1 — experimental public-core preview

An evaluation release of the Rust binary core for an AI knowledge bank.
The implementation builds on public v0.2.0 commit 04a4d21 and was merged
as commit 95c7dab in [PR #3](https://github.com/gelramlicensing-wq/gel-ram/pull/3).
This is not a complete knowledge encoder or production-readiness claim.

## User-visible value

Exact Top-K avoids scanning the sorted winner list for candidates that cannot
enter it. Progressive Top-K accumulates mismatch counts across 32/64/128-byte
stages instead of rescoring prefixes. The score, tie ordering and strict
pruning contract are unchanged; no approximate routing or extra index is used.
See `docs/TOPK-BENCHMARK.md` for the frozen v0.2.0 comparison and
`docs/VALIDATION-v0.2.1.md` for results, including regressions.

Default store loading now enforces a 256 MiB payload budget before allocation.
Existing-file generation checks validate full v2 integrity before publication;
damaged files cannot silently authorize an overwrite. Legacy v1 payload
migration discards its unprotected generation and monotonic writes reject v1.

The reproducible data audit compares exact GEL storage with clearly defined
FP16 and Q8 conversions on identical synthetic input, independently verifies
binary ranking, and checks structural reconstruction. Its correctness
threshold is 100%, with lossy-conversion errors reported separately.
See `docs/DATA-INTEGRITY.md` for commands and limitations.

## Compatibility and cost

The v2 disk format is unchanged. Larger-than-256-MiB loads need explicit
limits; migrated v1 generations now start at zero. Generation-guarded writes
add one full streaming verification pass over the existing file. New code
must handle `LegacyGenerationUntrusted`. Single-writer assumptions remain.

Top-K improvements are workload-specific, not a universal speed or semantic
accuracy claim. K=1 regressions and small-bank results are disclosed alongside
larger-K gains. Top-1 scanning and the scoring kernel are unchanged.
Prior hardening, APIs, CI jobs, tests,
issue forms and verification history are retained. The licensing gate also
checks the preserved distribution LICENSE fingerprint and catches accidental
non-project Gmail contacts without echoing addresses into CI output.
That check is not a general secret scanner or legal audit.

The existing legal documents remain unchanged.

## Verification and getting started

- 79 tests passed in each local portable/x86-64-v3 verification profile.
- 6240 final comparison outputs matched the frozen exact reference.
- [Post-merge CI](https://github.com/gelramlicensing-wq/gel-ram/actions/runs/34032946492)
  passed Linux verification and the independent integrity audit, plus
  macOS/Windows compilation checks (not runtime testing on those two systems).
- Full Top-32 on the 16 MiB uniform synthetic bank measured 3.67–5.63×
  old/new median latency ratios on Ryzen AI 9 HX 370, x86-64-v3, across
  three trials. This is not a universal gain; K=1 includes regressions.
- Natural-text lexical Recall@10 was 0.480 / 0.270: the 0.99 semantic
  retrieval target remains unmet. Exact byte transport is a separate test.

Start with [README](README.md#quick-start) and [the reproducible demo](docs/TRY-IT.md).
The source distribution includes a SHA-256 manifest; it excludes itself.
For a downloaded release ZIP, verify its separate SHA-256 checksum first,
then check the per-file manifest after extracting. Hashes detect mismatch,
not independent proof of publisher identity.
