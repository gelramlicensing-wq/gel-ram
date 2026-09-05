# Reader16 contract

Reader16 performs one fused binary comparison and returns 16 deterministic judgments.

## Global contingency

For every bit pair `(a,b)` the kernel counts:

```text
n00  a=0 b=0
n01  a=0 b=1
n10  a=1 b=0
n11  a=1 b=1
```

The four counts sum to exactly 1024.

From them Reader16 returns eight global judgments:

1. XNOR/global agreement
2. positive Jaccard
3. positive Dice
4. A→B inclusion
5. B→A inclusion
6. signed phi correlation
7. contradiction rate
8. directional asymmetry

The other eight judgments are XNOR agreement in eight disjoint 128-bit subspaces.

## No fake independence claim

Several global judgments are algebraically related because they originate from the same contingency table. The eight local views expose spatial distribution but are still functions of the same physical ORB pair.

A future F1 dataset gate must measure **conditional/incremental task information**. A view only counts as useful if it improves a declared task or rejection decision after conditioning on earlier views.

## Exact progressive Top-K

The progressive reader calculates an upper bound after 32 B and 64 B:

```text
upper = matches_seen + unseen_bits
```

A candidate is pruned only when `upper < current_kth_score`. Therefore the progressive Top-K must be exactly identical to full 128-byte Top-K, including deterministic tie handling.
