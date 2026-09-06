# GEL RAM v0.2.1 — local candidate

This candidate is based on merged main commit 04a4d21, not a replacement
with the older snapshot contained in the supplied v0.2.1 ZIP.

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

No new CLA, patent or trademark terms from the supplied ZIP are adopted in
this technical candidate. The existing legal documents remain unchanged.
Publication and remote multi-platform CI remain separate release steps.
