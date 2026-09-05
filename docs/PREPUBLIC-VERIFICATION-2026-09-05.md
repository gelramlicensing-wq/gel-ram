# Pre-public verification — 2026-09-05

This record applies to the GEL RAM v0.2.0 PolyForm candidate on the local
`pre-public-hardening-v0.2.0` branch. At the time of verification the GitHub
repository remained private. This record is about the source candidate; it is
not a semantic-quality or legal-review certificate.

## Source inventory

- 53 release-tree files, including this verification record and the GitHub
  issue forms added by this branch.
- 10 Rust source files and 4,348 Rust source lines.
- 66 Rust `#[test]` functions.
- zero external Cargo dependencies.
- zero Rust `unsafe` expressions or blocks.
- no symlinks or executable release-tree files.
- no Python, JavaScript, TypeScript, shell, C, C++ or Zig source files.

GitHub Actions contains small fixed shell command blocks in trusted YAML. They
are CI configuration, not shipped shell source or a runtime dependency. The
CLA workflow does not check out or execute pull-request code.

## Authoritative local gates

Both of these complete gates passed with Rust 1.85.0:

```text
cargo run --locked --offline -p xtask -- verify
CARGO_TARGET_DIR=<temporary-directory> RUSTFLAGS="-C target-cpu=x86-64-v3" cargo run --locked --offline -p xtask -- verify
```

Each run passed the Rust-only, licensing, CI-policy and documentation-reference
gates; formatting; Clippy with warnings denied; release build; rustdoc with
warnings denied; all 66 tests; CLI self-test; and exact one- and two-thread
benchmark smoke tests. The portable default run emitted no unstable target
feature warning. The x86-64-v3 run reported POPCNT and AVX2 enabled.

## Independent Rust oracle

A separately compiled Rust-only oracle, outside the workspace and using only
the public crate APIs, produced:

```text
HEADER_1BIT_REJECT=512/512
HEADER_2BIT_REJECT=130816/130816
PAYLOAD_BIT_REJECT=3072/3072
TRUNCATION_REJECT=448/448
TRAILING_REJECT=64/64
STRUCTURAL_ROUNDTRIPS=1025/1025
VIEW_ROUNDTRIPS=1024/1024
RANKING_CHECKS=960/960
LEGACY_V1_SOURCE_REPORTING=PASS
LEGACY_V1_CLI_REPORTING=PASS
INDEPENDENT_ORACLE=PASS
```

The legacy check confirms that a verified `GELORB01` file is reported as
source format 1 while its protected migration/write format is reported as 2.

## 160 MiB single-core scan

The final x86-64-v3 release binary was pinned to logical CPU 0 and run three
times with 1,310,720 ORBs, 16 measured rounds, one warm-up round and one scan
thread. System load was high and variable (`load1` was 15.16 immediately after
the series), so all three results are retained instead of selecting the
fastest:

```text
bank_bytes=167772160
bank_crc64_ecma=72442fd264cb251f
run  ORB/s          GiB/s       p50 ns   p95/p99 ns
1    198201474.198  23.627457   6522355  7569135
2    208094528.597  24.806801   6559234  7251777
3    173384502.609  20.669043   7458326  9930438
```

Every run reported `top1_exact=16/16`, `progressive_top8_exact=PASS`,
`observed_cpu_start=0` and `observed_cpu_end=0`. The median throughput of this
three-run series was 198,201,474 ORB/s (23.627 GiB/s). The controlled historical
measurements remain in `docs/SILICON-2026-09-04.md` and must not be conflated
with this loaded-system spot check.

This is one machine-specific timing sample over deterministic synthetic ORBs.
It demonstrates bit-exact execution and memory-scan throughput; it does not
measure real-data recall, semantic accuracy or compression quality.

## GitHub gates still required

The updated branch had not yet been pushed when this local record was created.
Therefore the updated Linux verification job and new macOS/Windows portable
compile jobs are `NOT_RUN` on GitHub for this exact tree. They must all pass on
the pull request before merging into `main`. Public visibility must remain off
until that pull request and the final repository-settings review are complete.

The PolyForm and commercial-licensing documents have not been certified by a
lawyer. Technical verification cannot replace legal advice.
