#![forbid(unsafe_code)]

//! Hot-path binary kernels specialized for the fixed 1024-bit GEL ORB.

use gel_core::{GelError, ORB_BITS, ORB_WORDS};
use gel_orb::Orb1024;

/// Compile-time facts about how the popcount kernels were built.
///
/// Every kernel in this crate is plain `u64::count_ones()` over the sixteen
/// ORB words. There is no hand-written SIMD and no runtime CPU dispatch.
/// rustc emits `POPCNT` or a SIMD popcount sequence only when the build
/// target enables the corresponding feature (for example
/// `-C target-cpu=native` or `-C target-feature=+popcnt`); otherwise it
/// emits a bit-twiddling fallback. The two flags mirror
/// `cfg!(target_feature = "popcnt")` and `cfg!(target_feature = "avx2")` for
/// the crate that compiled this function, so they describe the build, not
/// the machine it runs on. AVX-512 use cannot be observed through `cfg` on
/// Rust 1.85 and must be checked by disassembling the binary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KernelBackend {
    CountOnes { popcnt: bool, avx2: bool },
}

impl KernelBackend {
    /// Stable one-line form, e.g. `count_ones(popcnt=false,avx2=false)`.
    pub fn describe(&self) -> String {
        match self {
            Self::CountOnes { popcnt, avx2 } => {
                format!("count_ones(popcnt={popcnt},avx2={avx2})")
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Contingency {
    pub n00: u16,
    pub n01: u16,
    pub n10: u16,
    pub n11: u16,
}

impl Contingency {
    #[inline]
    pub const fn total(self) -> u16 {
        self.n00 + self.n01 + self.n10 + self.n11
    }

    #[inline]
    pub const fn matches(self) -> u16 {
        self.n00 + self.n11
    }

    #[inline]
    pub const fn mismatches(self) -> u16 {
        self.n01 + self.n10
    }

    #[inline]
    pub const fn a_ones(self) -> u16 {
        self.n10 + self.n11
    }

    #[inline]
    pub const fn b_ones(self) -> u16 {
        self.n01 + self.n11
    }
}

#[inline(always)]
pub fn contingency(a: &Orb1024, b: &Orb1024) -> Contingency {
    let aw = a.words();
    let bw = b.words();
    let mut out = Contingency::default();
    for (&x, &y) in aw.iter().zip(bw.iter()) {
        out.n11 += (x & y).count_ones() as u16;
        out.n10 += (x & !y).count_ones() as u16;
        out.n01 += (!x & y).count_ones() as u16;
        out.n00 += (!(x | y)).count_ones() as u16;
    }
    debug_assert_eq!(out.total(), ORB_BITS as u16);
    out
}

/// Eight disjoint 128-bit local agreement views. Each element is XNOR matches
/// inside exactly two consecutive u64 words.
#[inline(always)]
pub fn subspace_xnor8(a: &Orb1024, b: &Orb1024) -> [u16; 8] {
    let aw = a.words();
    let bw = b.words();
    let mut out = [0u16; 8];
    for (group, slot) in out.iter_mut().enumerate() {
        let i = group * 2;
        let d = (aw[i] ^ bw[i]).count_ones() + (aw[i + 1] ^ bw[i + 1]).count_ones();
        *slot = 128 - d as u16;
    }
    out
}

#[inline(always)]
pub fn xnor_matches(a: &Orb1024, b: &Orb1024) -> u16 {
    let a = a.words();
    let b = b.words();
    let d0 = (a[0] ^ b[0]).count_ones()
        + (a[1] ^ b[1]).count_ones()
        + (a[2] ^ b[2]).count_ones()
        + (a[3] ^ b[3]).count_ones();
    let d1 = (a[4] ^ b[4]).count_ones()
        + (a[5] ^ b[5]).count_ones()
        + (a[6] ^ b[6]).count_ones()
        + (a[7] ^ b[7]).count_ones();
    let d2 = (a[8] ^ b[8]).count_ones()
        + (a[9] ^ b[9]).count_ones()
        + (a[10] ^ b[10]).count_ones()
        + (a[11] ^ b[11]).count_ones();
    let d3 = (a[12] ^ b[12]).count_ones()
        + (a[13] ^ b[13]).count_ones()
        + (a[14] ^ b[14]).count_ones()
        + (a[15] ^ b[15]).count_ones();
    ORB_BITS as u16 - (d0 + d1 + d2 + d3) as u16
}

/// Exact partial score for the first `words` u64 lanes. The returned upper
/// bound assumes every unseen bit could match, so pruning by this bound cannot
/// remove a true winner.
#[inline]
pub fn progressive_xnor_bound(
    a: &Orb1024,
    b: &Orb1024,
    words: usize,
) -> Result<(u16, u16), GelError> {
    if words == 0 || words > ORB_WORDS {
        return Err(GelError::InvalidView("progressive words must be in 1..=16"));
    }
    let aw = a.words();
    let bw = b.words();
    let mut mismatches = 0u16;
    for (&x, &y) in aw.iter().zip(bw.iter()).take(words) {
        mismatches += (x ^ y).count_ones() as u16;
    }
    let seen_bits = (words * 64) as u16;
    let matches = seen_bits - mismatches;
    let unseen = ORB_BITS as u16 - seen_bits;
    Ok((matches, matches + unseen))
}

#[inline(always)]
pub fn similarity(a: &Orb1024, b: &Orb1024) -> f32 {
    xnor_matches(a, b) as f32 / ORB_BITS as f32
}

/// Backend of the kernels in this build. See [`KernelBackend`].
pub const fn backend() -> KernelBackend {
    KernelBackend::CountOnes {
        popcnt: cfg!(target_feature = "popcnt"),
        avx2: cfg!(target_feature = "avx2"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gel_core::{splitmix64, ORB_WORDS};

    fn sample(seed: u64) -> Orb1024 {
        let mut words = [0u64; ORB_WORDS];
        for (i, word) in words.iter_mut().enumerate() {
            *word = splitmix64(seed ^ i as u64);
        }
        Orb1024::from_words(words)
    }

    #[test]
    fn kernel_matches_reference() {
        for seed in 0..128 {
            let a = sample(seed);
            let b = sample(seed ^ 0x55aa);
            assert_eq!(xnor_matches(&a, &b), a.xnor_matches(&b));
            assert_eq!(contingency(&a, &b).matches(), xnor_matches(&a, &b));
        }
    }

    #[test]
    fn contingency_partitions_all_1024_bits() {
        let a = sample(1);
        let b = sample(2);
        let c = contingency(&a, &b);
        assert_eq!(c.total(), 1024);
        assert_eq!(c.matches() + c.mismatches(), 1024);
    }

    #[test]
    fn subspaces_sum_to_global_matches() {
        let a = sample(3);
        let b = sample(4);
        assert_eq!(
            subspace_xnor8(&a, &b).iter().copied().sum::<u16>(),
            xnor_matches(&a, &b)
        );
    }

    #[test]
    fn backend_describes_compile_time_features() {
        let KernelBackend::CountOnes { popcnt, avx2 } = backend();
        assert_eq!(popcnt, cfg!(target_feature = "popcnt"));
        assert_eq!(avx2, cfg!(target_feature = "avx2"));
        assert_eq!(
            backend().describe(),
            format!("count_ones(popcnt={popcnt},avx2={avx2})")
        );
        assert_eq!(
            KernelBackend::CountOnes {
                popcnt: false,
                avx2: false,
            }
            .describe(),
            "count_ones(popcnt=false,avx2=false)"
        );
        assert_eq!(
            KernelBackend::CountOnes {
                popcnt: true,
                avx2: false,
            }
            .describe(),
            "count_ones(popcnt=true,avx2=false)"
        );
    }

    #[test]
    fn progressive_bound_never_excludes_exact_score() {
        for seed in 0..64 {
            let a = sample(seed);
            let b = sample(seed + 1000);
            let exact = xnor_matches(&a, &b);
            for words in [4usize, 8, 12, 16] {
                let (partial, upper) = progressive_xnor_bound(&a, &b, words).unwrap();
                assert!(partial <= exact);
                assert!(exact <= upper);
                if words == 16 {
                    assert_eq!(partial, exact);
                    assert_eq!(upper, exact);
                }
            }
        }
    }
}
