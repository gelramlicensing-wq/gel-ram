#![forbid(unsafe_code)]

//! Same-binary comparison, not a semantic quality benchmark.
#[path = "support/v020_topk.rs"]
mod baseline;

use gel_core::{splitmix64, Crc64Ecma};
use gel_orb::Orb1024;
use gel_reader::{top_k, top_k_progressive};
use std::hint::black_box;
use std::io::Read;
use std::time::Instant;

fn next(state: &mut u64) -> u64 {
    *state = splitmix64(*state);
    *state
}

fn random_orb(state: &mut u64) -> Orb1024 {
    Orb1024::from_words(std::array::from_fn(|_| next(state)))
}

fn bank(kind: usize, len: usize) -> Vec<Orb1024> {
    let mut seed = 0x2026_0906_544f_504b;
    let centers: Vec<_> = (0..16).map(|_| random_orb(&mut seed)).collect();
    (0..len)
        .map(|i| match kind {
            0 => random_orb(&mut seed),
            1 => {
                let mut orb = centers[i % centers.len()];
                for _ in 0..16 {
                    let bit = next(&mut seed) as usize % 1024;
                    orb.words_mut()[bit / 64] ^= 1 << (bit % 64);
                }
                orb
            }
            _ => centers[0],
        })
        .collect()
}

#[inline(never)]
fn run(
    mode: usize,
    bank: &[Orb1024],
    query: &Orb1024,
    k: usize,
) -> (Vec<(usize, u16)>, [usize; 3]) {
    match mode {
        0 => (baseline::top_k(bank, query, k), [0; 3]),
        1 => (top_k(bank, query, k), [0; 3]),
        2 => {
            let (out, s) = baseline::top_k_progressive(bank, query, k);
            (
                out,
                [
                    s.rejected_after_32b,
                    s.rejected_after_64b,
                    s.full_128b_scores,
                ],
            )
        }
        _ => {
            let (out, s) = top_k_progressive(bank, query, k);
            (
                out,
                [
                    s.rejected_after_32b,
                    s.rejected_after_64b,
                    s.full_128b_scores,
                ],
            )
        }
    }
}

fn measure(label: &str, bank: &[Orb1024], queries: Option<&[Orb1024]>) {
    let mut crc = Crc64Ecma::new();
    for orb in bank {
        crc.update(&orb.to_le_bytes());
    }
    println!(
        "DATASET name={label} orbs={} bytes={} crc64={:016x}",
        bank.len(),
        bank.len() * 128,
        crc.finish()
    );
    if let Some(queries) = queries {
        let mut crc = Crc64Ecma::new();
        for query in queries {
            crc.update(&query.to_le_bytes());
        }
        println!(
            "QUERIES count={} crc64={:016x} schedule=ordinal_17_plus_trial_43_mod_count",
            queries.len(),
            crc.finish()
        );
    }
    for k in [1, 8, 32, 256] {
        for trial in 0..3 {
            let mut times: [Vec<u128>; 4] = std::array::from_fn(|_| Vec::new());
            let mut checked = 0;
            for ordinal in 0..13 {
                let mut query = bank[(ordinal * 65537 + trial * 17) % bank.len()];
                if ordinal % 3 == 1 {
                    for j in 0..32 {
                        let bit = (j * 29 + ordinal) % 1024;
                        query.words_mut()[bit / 64] ^= 1 << (bit % 64);
                    }
                } else if ordinal % 3 == 2 {
                    query = random_orb(&mut (ordinal as u64 + 773));
                }
                if let Some(queries) = queries {
                    query = queries[(ordinal * 17 + trial * 43) % queries.len()];
                }
                let expected = baseline::top_k(bank, &query, k);
                let mut stage_counts = [None, None];
                for offset in 0..4 {
                    let mode = (ordinal + trial + offset) % 4;
                    let start = Instant::now();
                    let (output, stats) =
                        run(mode, black_box(bank), black_box(&query), black_box(k));
                    black_box(&output);
                    let elapsed = start.elapsed().as_nanos();
                    assert_eq!(output, expected, "dataset={label} k={k} mode={mode}");
                    if mode >= 2 {
                        stage_counts[mode - 2] = Some(stats);
                    }
                    if ordinal >= 2 {
                        times[mode].push(elapsed);
                        println!("SAMPLE dataset={label} k={k} trial={} query={} mode={mode} ns={elapsed}", trial + 1, ordinal - 2);
                    }
                    checked += 1;
                }
                assert_eq!(stage_counts[0], stage_counts[1]);
            }
            let medians: Vec<_> = times
                .iter_mut()
                .map(|v| {
                    v.sort_unstable();
                    v[v.len() / 2]
                })
                .collect();
            println!("RESULT dataset={label} k={k} trial={} samples=11 old_full_ns={} new_full_ns={} old_progressive_ns={} new_progressive_ns={} full_speedup={:.6} progressive_speedup={:.6} exact={checked}/{checked} stages=PASS",
                trial + 1, medians[0], medians[1], medians[2], medians[3],
                medians[0] as f64 / medians[1] as f64, medians[2] as f64 / medians[3] as f64);
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut len = 131072usize;
    let mut raw = None;
    let mut raw_queries = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--orbs" => len = args.next().ok_or("missing count")?.parse()?,
            "--raw-orbs" => raw = Some(args.next().ok_or("missing path")?),
            "--raw-queries" => raw_queries = Some(args.next().ok_or("missing query path")?),
            _ => return Err("expected --orbs COUNT, --raw-orbs PATH or --raw-queries PATH".into()),
        }
    }
    if !(1..=524288).contains(&len) {
        return Err("orbs must be 1..524288".into());
    }
    if raw_queries.is_some() && raw.is_none() {
        return Err("--raw-queries requires --raw-orbs".into());
    }
    println!("GEL_TOPK_COMPARE_V1 baseline=04a4d21 warmup=2 samples=11 trials=3 order=rotating modes=old_full,new_full,old_progressive,new_progressive");
    println!(
        "backend={} arch={} semantic_accuracy=NOT_MEASURED",
        gel_kernel::backend().describe(),
        std::env::consts::ARCH
    );
    let queries = raw_queries.map(read_raw).transpose()?;
    if let Some(path) = raw {
        let records = read_raw(path)?;
        measure("external", &records, queries.as_deref());
    } else {
        for (kind, label) in ["uniform", "clustered", "ties"].into_iter().enumerate() {
            measure(label, &bank(kind, len), None);
        }
    }
    println!("TOPK_COMPARE_EXACT=PASS");
    Ok(())
}

fn read_raw(path: String) -> Result<Vec<Orb1024>, Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.is_empty() || bytes.len() % 128 != 0 || bytes.len() > 64 * 1024 * 1024 {
        return Err("raw ORBs must be nonempty, 128-byte aligned, at most 64 MiB".into());
    }
    Ok(bytes
        .chunks_exact(128)
        .map(Orb1024::from_le_bytes)
        .collect::<Result<Vec<_>, _>>()?)
}
