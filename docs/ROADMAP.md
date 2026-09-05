# Roadmap

## Implemented in v0.2.0

- F0: dependency-free memory physics harness.
- F1: contingency kernel, Reader16, deterministic Top-K, exact progressive pruning.
- F2: exact structural XOR codec, sparse/dense residual, literal fallback, depth <= 2.
- Store v2: CRC64-ECMA protected metadata+payload, bounded open, rollback rejection.
- Bench: exact multi-thread scan (lowest-index tie handling), Top-1 equal to the single-thread scan.
- F0 v3: sequential and random-fetch probes without a per-element barrier, on 64-byte-aligned buffers.

## Next gates

### F0.5 topology closure

Measure on target hardware:

- 4 KiB vs transparent/explicit huge pages,
- L3 stream bandwidth per cache domain,
- RAM stream bandwidth above LLC,
- batch 8/16/32 random ORB fetches,
- thread scaling per L3 domain with pinning,
- dTLB/LLC hardware counters.

### F1.5 information separability

On a declared dataset, measure conditional value of each Reader16 judgment. Coordinate isometries never count as new information.

### F2.5 capacity

Run structural coding on real ORBs and report distribution of exact DER, residual popcount and context touch. No fixed 4x/8x/16x claim is accepted without the denominator.

### F3 locator + sketch

Build the first scale-selective path only after F0/F2 choose the physical budget. Membership verification must accompany any perfect-hash locator.

### F4 E2E scale

10^7 ORBs before 10^9. Query latency, capacity and exactness are one compound gate, not separate marketing numbers.
