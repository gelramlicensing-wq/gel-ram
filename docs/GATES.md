# Release gates

A release candidate is green only when all applicable gates pass on the same source tree.

1. Rust-only source/tooling gate.
2. Licensing-mode gate (PolyForm Noncommercial 1.0.0 only).
3. CI policy gate: pinned checkout SHA, read-only checkout credentials and a
   metadata-only CLA workflow that cannot execute pull-request code.
4. `cargo fmt --check`.
5. `cargo clippy -- -D warnings`.
6. all workspace tests.
7. release build of the complete workspace.
8. rustdoc with warnings denied.
9. integrated `gel-cli selftest`.
10. v2 header mutation: 512/512 single-bit and 130,816/130,816 two-bit flips rejected; v1 single-bit sweep frozen.
11. full Top-K == progressive Top-K.
12. structural original == decode bit-for-bit.
13. rollback generation <= current rejected.
14. pull request CLA acknowledgement (`xtask cla-ack` locally; trusted-base CI
    on `pull_request_target`, without checking out or executing pull-request
    code, including description-edit events).
15. multi-thread Top-1 == single-thread Top-1: gel-reader equality tests plus the gel-bench smoke run in verify (`thread_scan_exact=PASS`).
16. docs-refs gate: every backtick-quoted repository path in .md files exists.

Performance results are evidence, not correctness substitutes.
