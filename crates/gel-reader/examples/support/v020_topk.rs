// Frozen Top-K selection/progressive code from public commit 04a4d21.
// Only imports have been adapted. Kept for same-binary, same-kernel comparison.
use gel_kernel::progressive_xnor_bound;
use gel_orb::Orb1024;
use gel_reader::score_xnor;

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
