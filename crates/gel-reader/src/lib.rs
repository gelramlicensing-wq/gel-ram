#![forbid(unsafe_code)]

//! GEL readers over one immutable 1024-bit ORB.
//! Reversible geometries are kept separate from the information-bearing
//! Reader16 judgments so coordinate changes are never miscounted as new data.

use gel_core::{splitmix64, GelError, ORB_BITS, ORB_WORDS};
use gel_kernel::{
    contingency, progressive_xnor_bound, subspace_xnor8, xnor_matches as kernel_xnor_matches,
    Contingency,
};
use gel_orb::Orb1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ViewKind {
    Identity,
    Reverse,
    Rotate,
    XorMask,
    AffinePermutation,
    AffineMasked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ViewSpec {
    pub kind: ViewKind,
    pub a: u16,
    pub b: u16,
    pub seed: u64,
}

impl ViewSpec {
    pub const fn identity() -> Self {
        Self {
            kind: ViewKind::Identity,
            a: 1,
            b: 0,
            seed: 0,
        }
    }

    pub const fn reverse() -> Self {
        Self {
            kind: ViewKind::Reverse,
            a: 1,
            b: 0,
            seed: 0,
        }
    }

    pub fn rotate(bits: u16) -> Self {
        Self {
            kind: ViewKind::Rotate,
            a: 1,
            b: bits & 1023,
            seed: 0,
        }
    }

    pub fn xor_mask(seed: u64) -> Self {
        Self {
            kind: ViewKind::XorMask,
            a: 1,
            b: 0,
            seed,
        }
    }

    pub fn affine(a: u16, b: u16) -> Result<Self, GelError> {
        validate_odd(a)?;
        Ok(Self {
            kind: ViewKind::AffinePermutation,
            a: a & 1023,
            b: b & 1023,
            seed: 0,
        })
    }

    pub fn affine_masked(a: u16, b: u16, seed: u64) -> Result<Self, GelError> {
        validate_odd(a)?;
        Ok(Self {
            kind: ViewKind::AffineMasked,
            a: a & 1023,
            b: b & 1023,
            seed,
        })
    }
}

#[inline]
fn validate_odd(a: u16) -> Result<(), GelError> {
    if a == 0 || (a & 1) == 0 {
        Err(GelError::InvalidView(
            "affine multiplier must be odd modulo 1024",
        ))
    } else {
        Ok(())
    }
}

#[inline]
fn get(words: &[u64; ORB_WORDS], bit: usize) -> bool {
    ((words[bit >> 6] >> (bit & 63)) & 1) != 0
}

#[inline]
fn set(words: &mut [u64; ORB_WORDS], bit: usize, value: bool) {
    let mask = 1u64 << (bit & 63);
    if value {
        words[bit >> 6] |= mask;
    } else {
        words[bit >> 6] &= !mask;
    }
}

#[inline]
fn mask_word(seed: u64, word: usize) -> u64 {
    splitmix64(seed ^ (word as u64).wrapping_mul(0xD6E8_FEB8_6659_FD93))
}

pub fn apply_view(orb: &Orb1024, spec: ViewSpec) -> Result<Orb1024, GelError> {
    match spec.kind {
        ViewKind::Identity => Ok(*orb),
        ViewKind::Reverse => {
            let mut out = [0u64; ORB_WORDS];
            for (dst, slot) in out.iter_mut().enumerate() {
                *slot = orb.words()[ORB_WORDS - 1 - dst].reverse_bits();
            }
            Ok(Orb1024::from_words(out))
        }
        ViewKind::Rotate => {
            let r = spec.b as usize & 1023;
            if r == 0 {
                return Ok(*orb);
            }
            let word_shift = r >> 6;
            let bit_shift = r & 63;
            let mut out = [0u64; ORB_WORDS];
            for (dst, slot) in out.iter_mut().enumerate() {
                let a = orb.words()[(dst + word_shift) & 15];
                *slot = if bit_shift == 0 {
                    a
                } else {
                    let b = orb.words()[(dst + word_shift + 1) & 15];
                    (a >> bit_shift) | (b << (64 - bit_shift))
                };
            }
            Ok(Orb1024::from_words(out))
        }
        ViewKind::XorMask => {
            let mut out = *orb.words();
            for (i, word) in out.iter_mut().enumerate() {
                *word ^= mask_word(spec.seed, i);
            }
            Ok(Orb1024::from_words(out))
        }
        ViewKind::AffinePermutation | ViewKind::AffineMasked => {
            validate_odd(spec.a)?;
            let mut out = [0u64; ORB_WORDS];
            for dst in 0..ORB_BITS {
                let src = (spec.a as usize * dst + spec.b as usize) & 1023;
                let mut value = get(orb.words(), src);
                if matches!(spec.kind, ViewKind::AffineMasked) {
                    value ^= ((mask_word(spec.seed, dst >> 6) >> (dst & 63)) & 1) != 0;
                }
                set(&mut out, dst, value);
            }
            Ok(Orb1024::from_words(out))
        }
    }
}

pub fn invert_view(view: &Orb1024, spec: ViewSpec) -> Result<Orb1024, GelError> {
    match spec.kind {
        ViewKind::Identity => Ok(*view),
        ViewKind::Reverse => apply_view(view, spec),
        ViewKind::Rotate => apply_view(
            view,
            ViewSpec::rotate(((1024 - spec.b as usize) & 1023) as u16),
        ),
        ViewKind::XorMask => apply_view(view, spec),
        ViewKind::AffinePermutation | ViewKind::AffineMasked => {
            validate_odd(spec.a)?;
            let mut base = [0u64; ORB_WORDS];
            for dst in 0..ORB_BITS {
                let mut value = get(view.words(), dst);
                if matches!(spec.kind, ViewKind::AffineMasked) {
                    value ^= ((mask_word(spec.seed, dst >> 6) >> (dst & 63)) & 1) != 0;
                }
                let src = (spec.a as usize * dst + spec.b as usize) & 1023;
                set(&mut base, src, value);
            }
            Ok(Orb1024::from_words(base))
        }
    }
}

pub const READER16_NAMES: [&str; 16] = [
    "xnor_global",
    "jaccard_positive",
    "dice_positive",
    "a_implies_b",
    "b_implies_a",
    "phi_signed",
    "contradiction",
    "asymmetry",
    "local_0",
    "local_1",
    "local_2",
    "local_3",
    "local_4",
    "local_5",
    "local_6",
    "local_7",
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reader16Output {
    pub values: [f32; 16],
    pub contingency: Contingency,
}

#[inline]
fn ratio(num: u32, den: u32) -> f32 {
    if den == 0 {
        1.0
    } else {
        num as f32 / den as f32
    }
}

/// Fused 16-judgment read. The first eight values are different global
/// relations derived from n00/n01/n10/n11. The second eight are disjoint
/// 128-bit local agreement views, so they can expose where agreement lives.
pub fn reader16(a: &Orb1024, b: &Orb1024) -> Reader16Output {
    let c = contingency(a, b);
    let local = subspace_xnor8(a, b);
    let union_positive = c.n11 as u32 + c.n10 as u32 + c.n01 as u32;
    let a_positive = c.n11 as u32 + c.n10 as u32;
    let b_positive = c.n11 as u32 + c.n01 as u32;
    let phi_num = c.n11 as f64 * c.n00 as f64 - c.n10 as f64 * c.n01 as f64;
    let phi_den = ((c.n11 as f64 + c.n10 as f64)
        * (c.n01 as f64 + c.n00 as f64)
        * (c.n11 as f64 + c.n01 as f64)
        * (c.n10 as f64 + c.n00 as f64))
        .sqrt();
    let phi = if phi_den == 0.0 {
        0.0
    } else {
        (phi_num / phi_den) as f32
    };
    let mut values = [0.0f32; 16];
    values[0] = c.matches() as f32 / ORB_BITS as f32;
    values[1] = ratio(c.n11 as u32, union_positive);
    values[2] = ratio(
        2 * c.n11 as u32,
        2 * c.n11 as u32 + c.n10 as u32 + c.n01 as u32,
    );
    values[3] = ratio(c.n11 as u32, a_positive);
    values[4] = ratio(c.n11 as u32, b_positive);
    values[5] = phi;
    values[6] = c.mismatches() as f32 / ORB_BITS as f32;
    values[7] = (c.n10 as f32 - c.n01 as f32) / ORB_BITS as f32;
    for (i, value) in local.iter().enumerate() {
        values[8 + i] = *value as f32 / 128.0;
    }
    Reader16Output {
        values,
        contingency: c,
    }
}

#[inline]
pub fn score_xnor(candidate: &Orb1024, query: &Orb1024) -> u16 {
    kernel_xnor_matches(candidate, query)
}

pub fn top1<'a>(bank: &'a [Orb1024], query: &Orb1024) -> Option<(usize, u16, &'a Orb1024)> {
    let mut best: Option<(usize, u16, &Orb1024)> = None;
    for (index, orb) in bank.iter().enumerate() {
        let score = score_xnor(orb, query);
        if best.as_ref().is_none_or(|x| score > x.1) {
            best = Some((index, score, orb));
        }
    }
    best
}

/// Contiguous `(start, len)` chunks covering `0..len` in order. The first
/// `len % threads` chunks are one element longer; every chunk is non-empty
/// when `len >= threads`. Chunk `t` is a pure function of `t`, so the
/// iterator can be walked from either end.
fn chunk_bounds(len: usize, threads: usize) -> impl DoubleEndedIterator<Item = (usize, usize)> {
    let base = len / threads;
    let extra = len % threads;
    (0..threads).map(move |t| (t * base + t.min(extra), base + usize::from(t < extra)))
}

/// Exact multi-threaded Top-1. The bank is split into `threads` contiguous
/// chunks; each chunk is scanned with [`top1`] on its own thread inside
/// `std::thread::scope` (the last chunk on the calling thread), and the
/// per-chunk winners are merged: highest score wins, equal scores resolve to
/// the lowest global ORB index. The result is identical to `top1(bank, query)`
/// for every bank, query and thread count.
///
/// Contract: the parallel path runs only when `2 <= threads <= bank.len() / 2`,
/// so every chunk holds at least two ORBs and exactly `threads - 1` threads
/// are spawned. Any other `threads` (`0`, `1`, more than half the bank, and
/// in particular every value above `bank.len()` up to `usize::MAX`) scans on
/// the calling thread without spawning. The guard divides instead of
/// multiplying, so it cannot overflow. Panics only if the operating system
/// refuses to create a thread.
pub fn top1_threads<'a>(
    bank: &'a [Orb1024],
    query: &Orb1024,
    threads: usize,
) -> Option<(usize, u16, &'a Orb1024)> {
    if threads <= 1 || threads > bank.len() / 2 {
        return top1(bank, query);
    }
    let scan = |(start, len): (usize, usize)| {
        top1(&bank[start..start + len], query)
            .map(|(index, score, orb)| (start + index, score, orb))
    };
    let mut chunks = chunk_bounds(bank.len(), threads);
    let last = chunks.next_back().expect("threads >= 2");
    std::thread::scope(|scope| {
        let handles = chunks
            .map(|chunk| scope.spawn(move || scan(chunk)))
            .collect::<Vec<_>>();
        let mut best: Option<(usize, u16, &'a Orb1024)> = None;
        let hits = handles
            .into_iter()
            .map(|handle| handle.join().expect("scan thread panicked"))
            .chain(std::iter::once(scan(last)));
        for hit in hits.flatten() {
            let better = best
                .is_none_or(|(index, score, _)| hit.1 > score || (hit.1 == score && hit.0 < index));
            if better {
                best = Some(hit);
            }
        }
        best
    })
}

/// Deterministic descending Top-K; ties are resolved by lower ORB index.
pub fn top_k(bank: &[Orb1024], query: &Orb1024, k: usize) -> Vec<(usize, u16)> {
    if k == 0 {
        return Vec::new();
    }
    let mut best = Vec::<(usize, u16)>::with_capacity(k.min(bank.len()));
    for (index, orb) in bank.iter().enumerate() {
        insert_top_k(&mut best, (index, score_xnor(orb, query)), k);
    }
    best
}

fn insert_top_k(best: &mut Vec<(usize, u16)>, candidate: (usize, u16), k: usize) {
    let pos = best
        .iter()
        .position(|&(index, score)| {
            candidate.1 > score || (candidate.1 == score && candidate.0 < index)
        })
        .unwrap_or(best.len());
    if pos < k {
        best.insert(pos, candidate);
        if best.len() > k {
            best.pop();
        }
    } else if best.len() < k {
        best.push(candidate);
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProgressiveStats {
    pub candidates: usize,
    pub rejected_after_32b: usize,
    pub rejected_after_64b: usize,
    pub full_128b_scores: usize,
}

/// Exact progressive Top-K. It uses mathematically safe upper bounds after the
/// first 32 and 64 bytes; only `upper < current kth score` is pruned.
pub fn top_k_progressive(
    bank: &[Orb1024],
    query: &Orb1024,
    k: usize,
) -> (Vec<(usize, u16)>, ProgressiveStats) {
    if k == 0 {
        return (
            Vec::new(),
            ProgressiveStats {
                candidates: bank.len(),
                ..ProgressiveStats::default()
            },
        );
    }
    let mut best = Vec::<(usize, u16)>::with_capacity(k.min(bank.len()));
    let mut stats = ProgressiveStats {
        candidates: bank.len(),
        ..ProgressiveStats::default()
    };
    for (index, orb) in bank.iter().enumerate() {
        let threshold = if best.len() == k { best[k - 1].1 } else { 0 };
        if best.len() == k {
            let (_, upper32) = progressive_xnor_bound(orb, query, 4).expect("fixed valid prefix");
            if upper32 < threshold {
                stats.rejected_after_32b += 1;
                continue;
            }
            let (_, upper64) = progressive_xnor_bound(orb, query, 8).expect("fixed valid prefix");
            if upper64 < threshold {
                stats.rejected_after_64b += 1;
                continue;
            }
        }
        stats.full_128b_scores += 1;
        insert_top_k(&mut best, (index, score_xnor(orb, query)), k);
    }
    (best, stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(seed: u64) -> Orb1024 {
        let mut words = [0u64; ORB_WORDS];
        for (i, w) in words.iter_mut().enumerate() {
            *w = splitmix64(7 + seed + i as u64);
        }
        Orb1024::from_words(words)
    }

    #[test]
    fn all_public_geometries_are_exactly_invertible() {
        let x = sample(0);
        let views = [
            ViewSpec::identity(),
            ViewSpec::reverse(),
            ViewSpec::rotate(1),
            ViewSpec::rotate(257),
            ViewSpec::xor_mask(0x4745_4c01),
            ViewSpec::affine(3, 11).unwrap(),
            ViewSpec::affine_masked(5, 37, 0x4745_4c02).unwrap(),
        ];
        for spec in views {
            let y = apply_view(&x, spec).unwrap();
            let z = invert_view(&y, spec).unwrap();
            assert_eq!(z, x, "failed for {spec:?}");
        }
    }

    #[test]
    fn coordinate_geometries_preserve_xnor_when_applied_to_both_sides() {
        let a = sample(1);
        let b = sample(2);
        let baseline = score_xnor(&a, &b);
        for spec in [
            ViewSpec::reverse(),
            ViewSpec::rotate(257),
            ViewSpec::xor_mask(9),
            ViewSpec::affine(5, 3).unwrap(),
        ] {
            assert_eq!(
                score_xnor(
                    &apply_view(&a, spec).unwrap(),
                    &apply_view(&b, spec).unwrap()
                ),
                baseline
            );
        }
    }

    #[test]
    fn reader16_exposes_local_and_asymmetric_judgments() {
        let a = Orb1024::from_words([u64::MAX; ORB_WORDS]);
        let mut b_words = [u64::MAX; ORB_WORDS];
        b_words[0] = 0;
        b_words[1] = 0;
        let b = Orb1024::from_words(b_words);
        let r = reader16(&a, &b);
        assert_eq!(r.contingency.n10, 128);
        assert_eq!(r.contingency.n01, 0);
        assert!(r.values[3] < r.values[4]);
        assert_eq!(r.values[8], 0.0);
        assert_eq!(r.values[9], 1.0);
    }

    #[test]
    fn top1_finds_exact_query() {
        let a = sample(3);
        let mut b_words = *a.words();
        b_words[0] ^= 1;
        let bank = [Orb1024::from_words(b_words), a];
        assert_eq!(top1(&bank, &a).unwrap().0, 1);
        assert_eq!(top1(&bank, &a).unwrap().1, 1024);
    }

    const SIZES: [usize; 5] = [1, 2, 7, 64, 1000];
    const THREADS: [usize; 5] = [1, 2, 3, 8, 17];

    fn random_bank(len: usize, seed: u64) -> Vec<Orb1024> {
        (0..len as u64).map(|i| sample(seed + 10_000 * i)).collect()
    }

    fn assert_same_as_top1(bank: &[Orb1024], query: &Orb1024, threads: usize) {
        let expected = top1(bank, query);
        let actual = top1_threads(bank, query, threads);
        assert_eq!(actual, expected, "len={} threads={threads}", bank.len());
        if let (Some(e), Some(a)) = (expected, actual) {
            assert!(
                std::ptr::eq(e.2, a.2),
                "len={} threads={threads}",
                bank.len()
            );
        }
    }

    #[test]
    fn chunk_bounds_partition_the_bank_in_order() {
        for len in SIZES {
            for threads in THREADS.into_iter().filter(|&t| len >= t) {
                let chunks = chunk_bounds(len, threads).collect::<Vec<_>>();
                assert_eq!(chunks.len(), threads);
                let mut next = 0usize;
                for (start, chunk_len) in chunks {
                    assert_eq!(start, next);
                    assert!(chunk_len >= 1);
                    next += chunk_len;
                }
                assert_eq!(next, len);
            }
        }
    }

    #[test]
    fn top1_threads_equals_top1_for_random_banks() {
        assert_eq!(top1_threads(&[], &sample(0), 4), None);
        for len in SIZES {
            let bank = random_bank(len, 500);
            let foreign = sample(999_999);
            for threads in THREADS {
                assert_same_as_top1(&bank, &foreign, threads);
                for query_index in [0, len / 3, len / 2, len - 1] {
                    assert_same_as_top1(&bank, &bank[query_index], threads);
                }
            }
        }
    }

    /// First and last index of every chunk the parallel path would use, or
    /// nothing when `top1_threads` falls through to the single-thread scan.
    fn chunk_edges(len: usize, threads: usize) -> Vec<usize> {
        if threads <= 1 || threads > len / 2 {
            return Vec::new();
        }
        chunk_bounds(len, threads)
            .flat_map(|(start, chunk_len)| [start, start + chunk_len - 1])
            .collect()
    }

    #[test]
    fn top1_threads_tie_resolves_to_lowest_index() {
        let query = sample(42);
        for len in SIZES.into_iter().filter(|&n| n >= 2) {
            for threads in THREADS {
                let mut pairs = vec![(0, len - 1), (len / 2, len / 2 + 1), (0, 1)];
                for edge in chunk_edges(len, threads) {
                    pairs.push((edge.saturating_sub(1), edge));
                    pairs.push((edge, edge + 1));
                    pairs.push((edge, len - 1));
                    pairs.push((0, edge));
                }
                for (lo, hi) in pairs.into_iter().filter(|&(lo, hi)| lo < hi && hi < len) {
                    let mut bank = random_bank(len, 700);
                    bank[lo] = query;
                    bank[hi] = query;
                    let hit = top1_threads(&bank, &query, threads).unwrap();
                    assert_eq!(
                        (hit.0, hit.1),
                        (lo, 1024),
                        "len={len} threads={threads} hi={hi}"
                    );
                    assert_same_as_top1(&bank, &query, threads);
                }
            }
        }
    }

    #[test]
    fn top1_threads_finds_winner_exactly_at_chunk_boundaries() {
        let query = sample(43);
        for len in SIZES.into_iter().filter(|&n| n >= 2) {
            for threads in THREADS {
                let mut positions = vec![0, len - 1];
                positions.extend(chunk_edges(len, threads));
                for position in positions {
                    let mut bank = random_bank(len, 900);
                    bank[position] = query;
                    let hit = top1_threads(&bank, &query, threads).unwrap();
                    assert_eq!(
                        (hit.0, hit.1),
                        (position, 1024),
                        "len={len} threads={threads}"
                    );
                    assert_same_as_top1(&bank, &query, threads);
                }
            }
        }
    }

    /// Thread counts at or above the bank size, up to `usize::MAX`, must take
    /// the single-thread fall-through. The guard is `threads > len / 2`; the
    /// former `len < 2 * threads` wrapped for `threads >= 2^63` and would try
    /// to spawn one thread per ORB, which fails this test with a spawn panic
    /// instead of returning the `top1` result.
    #[test]
    fn oversized_thread_counts_equal_top1_without_spawning() {
        let foreign = sample(1_234);
        for len in SIZES {
            let bank = random_bank(len, 1_100);
            for threads in [len, len + 1, usize::MAX / 2 + 1, usize::MAX] {
                assert!(chunk_edges(len, threads).is_empty());
                assert_same_as_top1(&bank, &foreign, threads);
                assert_same_as_top1(&bank, &bank[len / 2], threads);
                assert_same_as_top1(&bank, &bank[len - 1], threads);
            }
        }
        assert_eq!(top1_threads(&[], &foreign, usize::MAX), None);
    }

    #[test]
    fn top_k_is_deterministic_on_ties() {
        let a = sample(4);
        assert_eq!(top_k(&[a, a, a], &a, 2), vec![(0, 1024), (1, 1024)]);
    }

    #[test]
    fn progressive_top_k_is_exactly_equal_to_full_top_k() {
        let bank = (0..512).map(sample).collect::<Vec<_>>();
        for query_index in [0usize, 17, 255, 511] {
            for k in [1usize, 8, 32] {
                let full = top_k(&bank, &bank[query_index], k);
                let (progressive, stats) = top_k_progressive(&bank, &bank[query_index], k);
                assert_eq!(progressive, full);
                assert_eq!(stats.candidates, bank.len());
                assert!(stats.full_128b_scores <= bank.len());
            }
        }
    }
}
