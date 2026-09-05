# GEL RAM v0.2.0

This release turns the small binary baseline into the first measured deep-reader/structural-codec generation.

Added:

- F0 memory physics harness,
- true Reader16 judgments separated from reversible isometries,
- exact progressive Top-K bounds at 32/64/128 B,
- F2 XOR structural codec with 10-bit sparse residuals,
- hard delta depth <= 2,
- measured residual/record break-even tests,
- `.gel` format v2 with CRC64-ECMA header+payload protection,
- generation rollback rejection,
- bounded untrusted-store open API,
- one authoritative `xtask verify` path,
- p50/p95/p99 benchmark output and progressive counters.

No arbitrary compression ratio or semantic quality claim is introduced by this release.

The historical performance tree was compiled natively with Rust 1.85.0 and is
recorded in `docs/SILICON-2026-09-04.md`. The current candidate's local gate
results, test count and remaining GitHub checks are recorded in
`docs/PREPUBLIC-VERIFICATION-2026-09-05.md`. See
`docs/SILICON-2026-09-03.md` for the machine-specific diagnostic results of
2026-09-03 and their limitations.

## Release hardening (2026-09-04 through 2026-09-05)

- Licensing-mode gate accepts only PolyForm Noncommercial 1.0.0.
- `Required Notice:` in `NOTICE` now carries the licensor contact.
- CI: `persist-credentials: false` on checkout, job timeouts, concurrency groups, Node.js 24 `actions/checkout` pinned to its complete v7.0.1 SHA, and portable compile jobs on macOS and Windows. The CLA acknowledgement workflow runs on trusted-base `pull_request_target` metadata without checking out or executing pull-request code; local validation remains available through Rust `xtask cla-ack`.
- `xtask verify` gains a docs-refs gate (every backtick-quoted repository path in Markdown files must exist) and a `gel-bench` smoke run (8192 ORBs, 3 rounds, 2 threads then 1 thread).
- F0 v3 probes (header `GEL_PHYSICS_F0_V3`): the sequential probe takes one barrier per round and the random-fetch probe one per fetch, and probe buffers are 64-byte aligned. The sequential reduction keeps eight independent accumulator chains. With AVX-512 enabled the compiled `vpaddq zmm` loop reached only 32-43 GiB/s from L1 on the measurement machine against 305 GiB/s for the AVX2 build, which is why AVX-512 is off in the default build; `docs/SILICON-2026-09-04.md` records the measurements. The v1 sequential probe carried a per-element barrier that capped its 48 KiB (L1-resident) row near 40 GiB/s on one Zen5 core, so the 64/256 MiB sequential rows in `docs/SILICON-2026-09-03.md` (28.0/30.8 GiB/s) cannot be separated from that cap. The v3 random-fetch probe moved the barrier from each element to each fetch, so v1 and v3 random-fetch rows are not comparable. `gel-physics` prints `sequential_probe=`, `observed_cpu_start=` and `observed_cpu_end=`.
- `gel-bench` takes an optional `THREADS` argument (default 1; for `THREADS` > 1, `2 * THREADS <= ORB_COUNT`; always `THREADS <= 4 x available parallelism`), prints `GEL_BENCH_V3`, `backend=count_ones(popcnt=..,avx2=..)`, `thread_scan=single` or `thread_scan=exact_merge_lowest_index_tie`, `thread_scan_exact=PASS|SINGLE`, `observed_cpu_start=` and `observed_cpu_end=`. The multi-thread scan is an exact merge with lowest-index tie handling; its Top-1 must equal the single-thread Top-1.
- API changes: `gel_kernel::KernelBackend::PortablePopcount` replaced by `KernelBackend::CountOnes { popcnt, avx2 }` with `describe()`; `gel_reader::top1_threads` added with an API-level machine resource ceiling and graceful single-thread fallback; `gel_store::verify_file` returns a verification report that distinguishes actual v1/v2 source format from the v2 migration header; `gel-structural` rejects sparse residuals with nonzero padding bits.
- `gel-cli verify` streams the payload with constant memory (`verify_file`). The `gel-cli` selftest writes into a private temporary directory. Binaries read arguments with `args_os`.
- Store parser tests: deterministic mutation sweeps on the v2 header (single-bit 512/512 and all two-bit flips 130,816/130,816 rejected), a v1 header single-bit sweep freezing the documented undetectable generation flips, every payload byte, every truncation length; plus record-count overflow, oversized counts, legacy reserved bytes, nonzero flags.
- `unsafe_code` is forbidden workspace-wide.
- Normal builds now use rustc's portable target defaults and emit no unstable `target-feature` warning. Hardware measurements opt in explicitly with `RUSTFLAGS`. The stable x86-64-v3 profile enables POPCNT/AVX2 without AVX-512. `docs/SILICON-2026-09-04.md` retains the historical comparison: the measured AVX-512-off profile was 2.0x faster than the x86-64 baseline for a single-thread full scan on one Zen5 core, while AVX-512 reduced the physics sequential probe to 32-43 GiB/s in L1 and 4-7 GiB/s in RAM on that machine.
- Unix persistence now creates new stores as mode `0600` and preserves the existing mode during atomic replacement. Legacy v1 verification reports source version 1 rather than presenting the in-memory migration header as the on-disk format.
- The Rust-only release gate rejects symlinks and executable files in addition to enforcing the extension allow-list.
