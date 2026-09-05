# Contributing

GEL RAM is Rust-only.

Before proposing a change:

```text
cargo run --locked --offline -p xtask -- verify
```

This command is the authoritative local gate and runs the Rust-only, licensing
and docs-refs checks, formatting, Clippy, tests, release build, rustdoc, the CLI
selftest and a small `gel-bench` smoke run (8192 ORBs, 3 rounds, 2 threads then
1 thread).

Performance changes must include before/after benchmark output and must not weaken exactness tests.

## Pull request policy

1. Contact the project at `gelram.licensing@gmail.com` first.
2. Complete the project CLA privately; see `CLA.md`.
3. Only then open a pull request.

The pull request template (`.github/PULL_REQUEST_TEMPLATE.md`) contains a CLA
acknowledgement line. CI checks that the line is present and ticked (`[x]` or
`[X]`) and re-checks it when the pull request description is edited. The
`cla-acknowledgement` and `verify` checks are intended to be configured as
required status checks on the default branch. Pull requests without the
acknowledgement, or from contributors without a CLA on file, are closed
unmerged and their content is not incorporated. Opening a pull request is not
acceptance of the CLA.

Do not publish signed agreements, home addresses or other personal data in an
issue or pull request.

## Security reports

Do not report a vulnerability in a public issue or pull request. Follow
`SECURITY.md`.
