# Reproducible data-integrity audit

The acceptance threshold for GEL transport and exact reconstruction is **1.0**:
every input byte must be recovered unchanged. A 0.99 byte agreement is not
acceptable for a lossless store. Every assertion failure exits unsuccessfully.
Passing finite tests is evidence for those cases, not proof over every input,
physical memory fault, storage device or operating system.

## Run

From the repository root with Rust 1.85.0 installed:

```text
cargo run --locked --offline -p xtask -- verify
cargo run --locked --offline --release -p gel-cli --example data_integrity -- /tmp/gel-integrity-new-run
```

Append `--large` after the output directory to also verify 16, 256 and 512 MiB
banks. The 512 MiB bank must fail the default allocation budget and succeed
with an explicit budget. Fixtures occupy about 790 MiB and are retained.

The output directory must not exist. Fixtures are retained for inspection;
the command never overwrites an existing directory. Implementation:
`crates/gel-cli/examples/data_integrity.rs`. It has no third-party dependencies.
For the x86-64-v3 profile set `RUSTFLAGS="-C target-cpu=x86-64-v3"` and use
a separate target directory; record these settings with the output.

Optionally append `--fp32-dump PATH` to evaluate caller-supplied numeric states
in addition to the synthetic fixtures. The bounded input format is two
little-endian u32 fields (row count, dimension), row-major FP32 values, then
one u32 label per row. Labels never influence conversion. The file must be
at most 16 MiB, have an exact length and nonzero rows/dimensions, with dimension
divisible by 32. Values must be finite with magnitude at most 65504; a nonzero
Q8 block whose scale underflows to zero is rejected. Malformed input fails
the audit rather than being silently skipped or clipped.

This optional run reports the complete input CRC and byte count, numerical
conversion errors, and exact GEL byte round-trips. It does not regenerate or
authenticate a model dump's provenance, run inference, or measure semantic
recall. Private datasets are not included in the source distribution.

## Tests and metrics

- All 65,536 binary16 bit patterns, including signed zero, infinities and NaN
  payloads, are transported as bytes without numeric conversion.
- 1,048,576 deterministic random 32-bit patterns and explicit binary32 edge
  patterns exercise persistence. Reopened bytes and original bytes are compared
  directly, and the streaming verifier must agree with the loaded header.
- 262,400 structural decode cases cover all 0..1024 bit differences, both
  parent-reference kinds and both permitted parent depths. Literal fallback
  is allowed and is not counted as compression.
- 256 ranking queries compare Top-1, threaded Top-1, Top-K and progressive
  Top-K against an independent per-bit reference, including duplicate ties.
- Four synthetic numeric datasets, each 32,768 finite FP32 values, are used
  unchanged across the three storage representations: uniform [-1,1), wide
  dynamic range, block outliers, and signed zeros. These are not model weights
  or real-world embeddings. Seed and generator are fixed in the source.

The three representations are raw FP32 bytes retained by GEL, IEEE binary16
with nearest-even rounding, and reference Q8: blocks of 32 signed integers
in [-127,127], with one little-endian FP32 absmax/127 scale per block,
nearest-even rounding and no zero point. A zero block uses scale 1. Q8 here
is **not** GGUF Q8_0, GPTQ, AWQ or another engine's format.

Each encoded representation is separately persisted through GEL and must
round-trip byte-exactly. Numerical comparisons decode those recovered bytes.
FP16 conversion uses an exhaustive finite-value lookup table; all 63,488
signed finite values and 31,743 adjacent midpoint ties are checked.
Numeric datasets exclude binary16 overflow; special bit patterns are tested
only as opaque transport data, not as numeric-error samples.

Reported numeric metrics are the exact FP32 bit-match fraction, absolute
RMSE, relative L2 error and maximum absolute error versus the original FP32
values. Exact matching distinguishes +0 and -0; Q8 does not preserve that
distinction. Relative L2 for the all-zero dataset is defined as zero.
There is no invented single "quality score" and no universal 0.99 gate on
lossy formats. Quantization error is not GEL read corruption.

Byte counts include Q8 scales. `.gel` file counts separately include the
64-byte header and zero padding to 128-byte ORBs. Original byte length is
known by the audit; the GEL header stores ORB count, not arbitrary blob length.
Structural codec size estimates are not an implemented structural file format.

## Scope limits

These CPU-only reference conversions are correctness tools, not optimized
FP16/Q8 compute kernels. No GPU/NPU speed, inference quality, semantic recall,
compression of arbitrary FP16 data, or power-failure durability claim follows.
Disk round-trips may use the OS page cache; they are not cold-device throughput
measurements. CRC detects corruption; it does not authenticate hostile input.
Record CPU model, RAM, OS, pinned Rust version, compiler flags, affinity,
load and benchmark sizes with any published timing results.

For the intended AI knowledge-bank role, a separate acceptance test must define
the knowledge corpus, encoder, queries, ground-truth relevant records and K.
Only that test can establish e.g. Recall@K >= 0.99. Comparing FP16, Q8 and GEL
on that task also requires an explicit mapping of the same knowledge into each
representation. This audit deliberately does not claim that result.
