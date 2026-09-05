# Test-data policy

The release contains no external dataset, model weights or personal data.
Correctness fixtures are generated deterministically in Rust tests, including
a legacy v1 store that is verified and migrated to v2, CRC64-ECMA vectors,
header mutations, exact structural residuals and full/progressive Top-K pairs.

Large synthetic banks are generated at runtime by `gel-bench`; they are not
stored in the repository. A future semantic dataset must be distributed
separately with provenance, its own license and cryptographic hashes.
