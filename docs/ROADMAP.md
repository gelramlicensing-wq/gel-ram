# Development priorities

This is a direction, not a delivery-date promise. GEL's intended role is an
AI knowledge bank. The public binary engine is one component, not that entire
system. Private research archives are not a list of shipped features.

## Next: independently reproduce the public core

- Collect complete portable and hardware-specific comparison logs on other CPUs.
- Investigate K=1 regressions without weakening exactness or hiding slower cases.
- Extend actual runtime testing to macOS/Windows; current CI only compiles there.
- Preserve byte-exact reconstruction, deterministic ranking and bounded resources.

Acceptance: publish the hardware, commands, full result matrix and failure
cases. A speedup on one selected case is not a universal improvement.

## Then: a small end-to-end knowledge-bank evaluation

Define a redistributable corpus, explicit encoder, held-out queries, relevant
source records and fixed K before tuning. Test paraphrases, distractors and
queries whose answer is absent. Prevent exact query text or labels leaking
into the indexed representation. Return evidence references, not unsupported
answers; evaluate an abstention policy separately.

Compare FP16, a specified Q8 format and GEL only with a documented mapping of
the same knowledge and the same queries. Count encoder/context storage and
query costs. Report Recall@K, latency and total memory separately. The target
of 0.99 knowledge retrieval is not achieved by current lexical results
(Recall@10 0.480 / 0.270). Byte-exactness remains 1.0 on all tested cases.

Publishing a demonstration does not require publishing every private research
module. However, any claim advertised as independently reproducible needs
enough public data, code and instructions to reproduce that particular claim.

## Existing engineering gates retained

The original F0–F4 direction remains in scope; the nearer-term priorities
above do not mark these gates complete or replace them.

Already implemented in v0.2.0: the dependency-free F0 memory-physics harness,
F1 contingency kernel/Reader16/exact ranking, F2 structural XOR codec with
literal fallback and depth at most two, v2 CRC-protected persistence,
exact threaded Top-1 and aligned sequential/random-fetch probes.

- **F0.5 topology closure:** measure 4 KiB versus transparent/explicit huge
  pages, per-domain L3 bandwidth, RAM bandwidth above LLC, batch 8/16/32
  random ORB fetches, pinned per-domain thread scaling and dTLB/LLC counters.
- **F1.5 information separability:** measure the conditional value of each
  Reader16 judgment on a declared dataset. Coordinate isometries do not
  constitute new information.
- **F2.5 capacity:** report exact DER, residual-popcount distributions and
  context-touch cost on real ORBs. No fixed compression-factor claim without
  its denominator and all context costs.
- **F3 locator and sketch:** establish the F0/F2 physical budget before a
  scale-selective path; perfect-hash locators still need membership checks.
- **F4 end-to-end scale:** evaluate ten million ORBs before one billion.
  Latency, capacity and exactness form a combined gate, not independent
  marketing claims.

## Not claimed or scheduled

No arbitrary FP16-to-128-byte lossless compression, replacement for a complete
LLM, zero-hallucination guarantee, production durability certification, or
universal hardware speedup is promised. Proposed work must show measurable
value and retain correctness before it becomes a release headline.
