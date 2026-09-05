# Security

## Fix policy

Fixes, when made, land only on the current 0.2.x line. No fix, response time or support is promised.

## Attack surface

- The `.gel` parser in `gel-store`. `.gel` files are untrusted input.
- Command-line arguments: `gel-cli` takes a fixed command word and a file path; `gel-bench` and `gel-physics` take unsigned integers.
- `gel-cli verify` streams the payload through a fixed 128-byte buffer and holds no payload in memory.
- The `gel-cli` selftest creates a private directory in the OS temporary directory, writes its store there through a sibling temporary file renamed into place, and removes the directory afterwards (best effort).
- `write_atomic` writes a sibling `<name>.tmp-<pid>-<n>` file created with O_EXCL semantics and renames it into place. On Unix a new store is mode `0600`; replacement preserves the existing mode. `write_if_newer` assumes a single writer and is not a compare-and-swap.

## Input validation

Treat `.gel` files as untrusted input. The parser validates magic, version, fixed dimensions, flags, reserved bytes, arithmetic overflow, exact file length and payload checksum before exposing ORBs. Version 2 additionally validates a CRC64-ECMA-protected header before exposing its metadata.

The reader reserves memory for the declared record count without first duplicating the complete payload. Allocation failure is returned as an error. Applications should use `open_verified_with_limits` with a record and file-size budget appropriate to their environment.

Version 2 uses separate CRC64-ECMA fields for the complete header and payload. Legacy v1 protects only its payload and cannot retroactively authenticate historical header metadata. CRC is not a MAC, signature or cryptographic content identifier. Files crossing a trust boundary require an external cryptographic authentication layer.

The verification report distinguishes the actual on-disk source format from
the protected v2 header prepared for a later migration write. A legacy v1 file
is never displayed as if its original header had v2 authentication.

## Hardening in place

- `unsafe_code = "forbid"` in `[workspace.lints.rust]`, applied to every crate through `[lints] workspace = true`, and `#![forbid(unsafe_code)]` at the root of every crate, `xtask` included; the workspace contains no `unsafe`.
- No dependencies outside the workspace: `Cargo.lock` lists only the workspace crates.
- No network code.
- Bounded open API: `open_verified_with_limits` rejects a file above the caller's file-size budget before reading the header, and a record count above the record budget before allocation.
- CRC64-ECMA on the v2 header and on the payload.
- Exact length checks before allocation: the declared record count must match the file length exactly before the record buffer is reserved; the reservation is fallible and returns an error instead of aborting.

## Reporting

Use GitHub Private Vulnerability Reporting where it is enabled for this
repository; otherwise contact `gelram.licensing@gmail.com`. Do not disclose an
unpatched vulnerability in a public issue.
