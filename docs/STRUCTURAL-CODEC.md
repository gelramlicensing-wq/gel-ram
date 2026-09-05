# Structural exact codec F2

The codec uses a bitwise predictor, never numeric F16 arithmetic:

```text
delta = target XOR predictor
target = predictor XOR delta
```

This makes exactness a construction property.

## Residual representation

The codec considers two residual forms:

- dense: 128 bytes of XOR delta plus mode byte,
- sparse: count + sorted 10-bit bit positions plus mode byte.

The residual-only boundary is measured by the exact byte-size model used by
the in-memory representation:

```text
100 changed bits -> sparse residual = 128 B
101 changed bits -> dense residual  = 129 B
```

A full encoded delta also contains parent kind/id and depth. Therefore literal-vs-delta break-even is stricter:

```text
prototype parent:    <= 94 changed bits can beat literal
segment-local parent <= 96 changed bits can beat literal
```

These are format facts, not tuning guesses, and unit tests freeze them.

## Chain depth

`MAX_DELTA_DEPTH = 2` is a hard decoder/encoder contract. An attempted child of an already depth-2 parent is rejected.

## Metrics

At minimum report:

- exact bytes,
- physical encoded bytes,
- residual popcount,
- predictor/context bytes touched,
- local DER = exact bytes / physical encoded bytes,
- exact reconstruction equality.

Shared prototype pools, graphs and locator costs must be amortized separately before making total-capacity claims.

Version 0.2 exposes exact in-memory encode/decode and representation-size
accounting. Structural records are not yet serialized into the flat `.gel` v2
store. A future on-disk structural format requires its own canonical byte
encoding, parser, integrity contract and compatibility tests.
