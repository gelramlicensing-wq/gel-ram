# GEL binary format v2

All integer fields are little-endian. The v2 writer emits a 64-byte header followed immediately by contiguous 128-byte ORBs.

| Offset | Bytes | Field |
|---:|---:|---|
| 0 | 8 | magic `GELORB02` |
| 8 | 4 | version = 2 |
| 12 | 4 | orb_bits = 1024 |
| 16 | 4 | record_bytes = 128 |
| 20 | 4 | flags = 0 |
| 24 | 8 | record_count |
| 32 | 8 | generation |
| 40 | 8 | payload CRC64-ECMA |
| 48 | 8 | header CRC64-ECMA |
| 56 | 8 | reserved = 0 |

## CRC contract

Both checksums use **CRC-64/ECMA-182**:

```text
poly   = 0x42F0E1EBA9EA3693
init   = 0
refin  = false
refout = false
xorout = 0
check("123456789") = 0x6C40DF5F0B497347
```

Header CRC is calculated over all 64 header bytes with bytes 48..55 set to zero. Therefore `generation`, `record_count`, format geometry, flags and payload CRC are all covered.

The payload CRC covers exactly `record_count * 128` payload bytes.

CRC is for accidental corruption detection, not authentication. An untrusted remote source requires a cryptographic trust layer outside this hot-path file contract.

## Bounded loading and monotonic publication

Since v0.2.1, `RamStore::open_verified` allows at most 256 MiB of payload
(2,097,152 ORBs), plus the 64-byte header. Larger workloads must supply
`OpenLimits` explicitly or opt into `open_verified_unbounded`. File length
and record budgets are checked before payload allocation.

`write_if_newer` validates the existing v2 header, exact file length and full
payload CRC before accepting its generation. Verification streams through a
128-byte buffer: O(1) extra memory, but O(file size) additional read/checksum
work before a write. This deliberate cost is not a throughput improvement.
The single-writer contract still applies; this is not a multi-process CAS and
does not protect against concurrent mutation by another writer.

## v1 migration

v0.2 can read legacy `GELORB01` stores and verifies their legacy payload
checksum. v1 had no header authentication, so it cannot retroactively detect a
historical metadata bit flip. Verification reports the actual source as v1
separately from the in-memory v2 upgrade header. Any subsequent write emits
protected v2. In v0.2.1 the unprotected v1 generation is discarded and the
in-memory upgrade starts at generation zero. `write_if_newer` refuses v1 as
the source of monotonic state with `LegacyGenerationUntrusted`. To migrate,
load and verify the payload, explicitly publish v2 with `write_atomic`, and
then use increasing v2 generations. The v2 byte layout is unchanged.
