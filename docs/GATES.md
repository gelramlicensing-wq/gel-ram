# Release gates

A release candidate is green only when all applicable gates pass on the same source tree.

1. Rust-only source/tooling gate.
2. Licensing-mode gate (PolyForm Noncommercial 1.0.0 only).
3. `cargo fmt --check`.
4. `cargo clippy -- -D warnings`.
5. all workspace tests.
6. release build of the complete workspace.
7. rustdoc with warnings denied.
8. integrated `gel-cli selftest`.
9. v2 header mutation: 512/512 single-bit and 130,816/130,816 two-bit flips rejected; v1 single-bit sweep frozen.
10. full Top-K == progressive Top-K.
11. structural original == decode bit-for-bit.
12. rollback generation <= current rejected.
13. pull request CLA acknowledgement (`xtask cla-ack`, CI `pull_request` events incl. edited).
14. multi-thread Top-1 == single-thread Top-1: gel-reader equality tests plus the gel-bench smoke run in verify (`thread_scan_exact=PASS`).
15. docs-refs gate: every backtick-quoted repository path in .md files exists.

Performance results are evidence, not correctness substitutes.
