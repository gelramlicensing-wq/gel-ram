# Performance methodology

GEL performance claims must be reproducible and labelled by memory regime.

## F0 physics

Run:

```text
cargo run --locked --offline --release -p gel-physics -- 5
```

The harness reports:

- pointer-chase ns/step for cache/RAM-sized working sets (dependent latency),
- sequential GiB/s (vectorizable reduction, no per-element barrier),
- random 32/64/128-byte fetch ns (throughput over independent fetches),
- THP policy state where Linux exposes it,
- detected L3 sharing domains where sysfs exposes them.

`ns`, `us`, ORB/s and GiB/s are authoritative. Do not derive cycles from `scaling_cur_freq`.

The first output line names the probe version (`GEL_PHYSICS_F0_V3`); `probe_buffer_alignment=64` states the buffer alignment, which matters as soon as the build emits wide vector loads. Rows from
different probe versions are not comparable; always report the version.

The `sequential_probe=` line names the barrier placement of the sequential
probe (`vectorizable_reduction_no_per_element_barrier` for v2). It describes
the loop, not the result: whether a row is memory-bound is decided by the loop
ceiling comparison below.

The `observed_cpu_start=` and `observed_cpu_end=` lines are described under
Pinning.

The THP line reports the kernel policy file. It does not prove that a specific
benchmark allocation was backed by transparent huge pages.

A record of an F0 run carries the same identification items as a full-scan
record: timestamp (UTC), git commit or archive SHA256 of the source, affinity
and L3 domain, and the probe version.

### Pinning

Measurements must be pinned to one L3 domain, for example
`taskset -c 0-3 cargo run --locked --offline --release -p gel-physics -- 5`.
The report must name the domain and its L3 size. On a CPU whose L3 domains
differ in size, a working set that fits one domain but not another gives a
different number depending on where the thread ran; unpinned mid-size results
are not comparable.

Both harnesses print `observed_cpu_start=` and `observed_cpu_end=`: a
best-effort Linux reading of the CPU the process was running on at the start
and at the end of the run. The pair documents where a run happened. It does
not show migrations between the two readings and is not a substitute for
pinning.

### Loop ceiling reference

The 48 KiB sequential row must always be reported next to the RAM-sized rows.
When a RAM-sized row is close to the 48 KiB (L1) row, the probe is bound by its
own loop, not by memory, and the number is a loop ceiling rather than RAM
bandwidth.

Random fetch is a throughput number over independent fetches with memory-level
parallelism, not a dependent-latency number. Dependent latency is the
pointer-chase row.

## Full scan

Run:

```text
cargo run --locked --offline --release -p gel-bench -- <ORB_COUNT> <ROUNDS> [THREADS]
```

`THREADS` defaults to 1; for `THREADS` > 1 it must satisfy `2 * THREADS <= ORB_COUNT`,
and it must always satisfy `THREADS <= 4 x available parallelism`; other values
are rejected. The public reader API additionally caps the ceiling at 256 and
falls back to a single-thread scan if a worker cannot be created.

Normal builds use rustc's portable target defaults. Select a measurement
profile explicitly: `RUSTFLAGS="-C target-cpu=native"` for the current machine,
or `RUSTFLAGS="-C target-cpu=x86-64-v3"` for an x86-64 POPCNT/AVX2 profile that
does not enable AVX-512. Every record must quote the complete `RUSTFLAGS`, the
`backend=` line and the disassembly counts of `popcnt`, `vpopcntq` and `zmm` in
the binary, as in `docs/SILICON-2026-09-04.md`. Physics rows must come from a
build without AVX-512 unless the record shows that the `zmm` reduction is not
slower than the AVX2 one on that machine.

The output header is `GEL_BENCH_V3`. Output lines to read:

- `backend=count_ones(popcnt=..,avx2=..)`: whether the `popcnt` and `avx2`
  target features were enabled when the binary was compiled. AVX-512 use
  cannot be observed through `cfg` on Rust 1.85; when it matters, check the
  disassembly and state the result in the record.
- `thread_scan=single` or `thread_scan=exact_merge_lowest_index_tie`: the scan
  path taken.
- `thread_scan_exact=PASS` for `THREADS` > 1 (the multi-thread Top-1 equalled
  the single-thread Top-1 in this run) or `thread_scan_exact=SINGLE` for
  `THREADS` = 1.
- `top1_exact=N/M`: rounds whose Top-1 was the exact query.
- `observed_cpu_start=` and `observed_cpu_end=`: see Pinning.

Report:

- timestamp (UTC),
- git commit or archive SHA256 of the source,
- compiler and target CPU,
- CPU/CCX affinity if externally pinned (L3 domain and its size),
- compiled popcount features (`backend=` line),
- thread count, the `thread_scan_exact` line and the `top1_exact` line,
- ORB count and total bank bytes,
- warm-up count,
- p50/p95/p99 query ns,
- scanned ORB/s,
- effective GiB/s,
- exact Top-1 survival,
- progressive pruning counters.

Recommended working sets include at least 16 MiB, 64 MiB, 160 MiB, 512 MiB and 1 GiB where RAM permits. A 16 MiB benchmark must not be described as RAM bandwidth on a CPU whose L3 can contain it.

The generated bank is deterministic and synthetic. Throughput is not semantic retrieval quality.

Exact progressive bounds may legitimately reject zero candidates, especially
for Top-K on random-looking data. Report the counters; never describe the
progressive path as an acceleration unless the measured end-to-end time is lower.
