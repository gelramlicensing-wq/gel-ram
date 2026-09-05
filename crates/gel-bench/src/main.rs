#![forbid(unsafe_code)]

use gel_core::{splitmix64, Crc64Ecma, ORB_BYTES, ORB_WORDS};
use gel_kernel::{backend, KernelBackend};
use gel_orb::Orb1024;
use gel_reader::{top1, top1_threads, top_k_progressive};
use std::hint::black_box;
use std::time::Instant;

fn orb(index: usize, seed: u64) -> Orb1024 {
    let mut words = [0u64; ORB_WORDS];
    words[0] = seed ^ index as u64;
    for (i, w) in words.iter_mut().enumerate().skip(1) {
        *w = splitmix64(seed ^ index as u64 ^ (i as u64).rotate_left(17));
    }
    Orb1024::from_words(words)
}

fn positive_arg(index: usize, default: usize, name: &str) -> Result<usize, String> {
    let Some(raw) = std::env::args_os().nth(index) else {
        return Ok(default);
    };
    let raw = raw
        .into_string()
        .map_err(|bad| format!("{name} is not valid UTF-8: {}", bad.to_string_lossy()))?;
    let value = raw
        .parse::<usize>()
        .map_err(|_| format!("{name} must be a positive integer"))?;
    if value == 0 {
        Err(format!("{name} must be greater than zero"))
    } else {
        Ok(value)
    }
}

fn percentile(sorted: &[u128], q: f64) -> u128 {
    let index = ((sorted.len() - 1) as f64 * q).ceil() as usize;
    sorted[index.min(sorted.len() - 1)]
}

/// Field 39 (`processor`) of a `/proc/<pid>/stat` line: the CPU the task
/// last ran on. The `comm` field (2) may contain spaces and parentheses, so
/// the line is split after its last `)`; field 39 is then the 37th token.
fn processor_field(stat: &str) -> Option<usize> {
    let (_, rest) = stat.rsplit_once(')')?;
    rest.split_ascii_whitespace().nth(36)?.parse().ok()
}

/// `observed_cpu_*=` value: the CPU this thread was last scheduled on, or
/// `unavailable` when `/proc/self/stat` cannot be read or parsed. It records
/// where the process ran at that instant; it does not prove pinning.
fn observed_cpu() -> String {
    std::fs::read_to_string("/proc/self/stat")
        .ok()
        .and_then(|stat| processor_field(&stat))
        .map_or_else(|| "unavailable".to_owned(), |cpu| cpu.to_string())
}

/// Timed full scan. `threads == 1` is the unchanged single-thread path;
/// `threads > 1` is the exact chunked scan whose result is identical to it.
fn scan<'a>(
    bank: &'a [Orb1024],
    query: &Orb1024,
    threads: usize,
) -> Option<(usize, u16, &'a Orb1024)> {
    if threads > 1 {
        top1_threads(bank, query, threads)
    } else {
        top1(bank, query)
    }
}

/// True when two Top-1 hits name the same bank slot with the same score.
fn same_hit(a: (usize, u16, &Orb1024), b: (usize, u16, &Orb1024)) -> bool {
    a.0 == b.0 && a.1 == b.1 && std::ptr::eq(a.2, b.2)
}

fn run() -> Result<(), String> {
    let cpu_start = observed_cpu();
    let count = positive_arg(1, 1_310_720, "ORB_COUNT")?;
    let rounds = positive_arg(2, 16, "ROUNDS")?;
    let threads = positive_arg(3, 1, "THREADS")?;
    let threads_cap = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .saturating_mul(4);
    if threads > threads_cap {
        return Err(format!(
            "THREADS must not exceed 4 * available_parallelism ({threads_cap})"
        ));
    }
    if threads > 1 && threads > count / 2 {
        return Err("THREADS must satisfy THREADS <= ORB_COUNT / 2".into());
    }
    const GENERATOR_SEED: u64 = 0x4745_4C00_4245_4E43;
    let bank_bytes = count.checked_mul(ORB_BYTES).ok_or("bank size overflow")?;
    let mut bank = Vec::new();
    bank.try_reserve_exact(count)
        .map_err(|_| "benchmark bank does not fit in available memory")?;
    bank.extend((0..count).map(|i| orb(i, GENERATOR_SEED)));
    let mut bank_crc = Crc64Ecma::new();
    for orb in &bank {
        bank_crc.update(&orb.to_le_bytes());
    }
    let bank_crc64 = bank_crc.finish();

    let warmup_index = count / 2;
    let warmup =
        scan(black_box(&bank), black_box(&bank[warmup_index]), threads).ok_or("empty bank")?;
    if warmup.0 != warmup_index || warmup.1 != 1024 {
        return Err("warm-up Top-1 mismatch".into());
    }

    let mut durations = Vec::with_capacity(rounds);
    let mut exact = 0usize;
    let started_all = Instant::now();
    for round in 0..rounds {
        let query_index = (warmup_index + round.wrapping_mul(65_537)) % count;
        let started = Instant::now();
        let hit = black_box(scan(
            black_box(&bank),
            black_box(&bank[query_index]),
            threads,
        ))
        .expect("non-empty bank");
        durations.push(started.elapsed().as_nanos());
        exact += usize::from(hit.0 == query_index && hit.1 == 1024);
    }
    let elapsed = started_all.elapsed();
    durations.sort_unstable();
    let scans = count.checked_mul(rounds).ok_or("scan count overflow")?;
    let seconds = elapsed.as_secs_f64();
    let orbs_per_sec = scans as f64 / seconds;
    let gib_per_sec = scans as f64 * ORB_BYTES as f64 / seconds / (1024.0 * 1024.0 * 1024.0);

    let query_index = count / 3;
    let (progressive, progressive_stats) = top_k_progressive(&bank, &bank[query_index], 8);
    if progressive.first().map(|x| x.0) != Some(query_index) {
        return Err("progressive exact Top-K lost the exact query".into());
    }

    let null = orb(0, GENERATOR_SEED ^ 0xA5A5_5A5A_D3C1_B7E9);
    if bank.contains(&null) {
        return Err("null query unexpectedly exists in generated bank".into());
    }
    let null_hit = top1(&bank, &null).ok_or("empty bank")?;

    // With THREADS > 1 the timed `warmup` came from `top1_threads`; check it
    // against `top1` on the same query, and check `top1_threads` against the
    // single-thread null hit, so both directions are covered.
    let thread_scan_exact = if threads > 1 {
        let single_warmup = top1(&bank, &bank[warmup_index]).ok_or("empty bank")?;
        let threaded_null = top1_threads(&bank, &null, threads).ok_or("empty bank")?;
        if !same_hit(warmup, single_warmup) {
            return Err(
                "multi-thread Top-1 differs from single-thread Top-1 on warm-up query".into(),
            );
        }
        if !same_hit(null_hit, threaded_null) {
            return Err("multi-thread Top-1 differs from single-thread Top-1 on null query".into());
        }
        "PASS"
    } else {
        "SINGLE"
    };
    let backend: KernelBackend = backend();

    println!("GEL_BENCH_V3");
    println!("generator=splitmix64_with_unique_word0");
    println!("generator_seed={GENERATOR_SEED}");
    println!("bank_crc64_ecma={bank_crc64:016x}");
    println!("orbs={count}");
    println!("bank_bytes={bank_bytes}");
    println!("orb_bytes={ORB_BYTES}");
    println!("rounds={rounds}");
    println!("warmup_rounds=1");
    println!("query_schedule=varying-exact-self-stride-65537");
    println!("threads={threads}");
    println!(
        "thread_scan={}",
        if threads > 1 {
            "exact_merge_lowest_index_tie"
        } else {
            "single"
        }
    );
    println!("thread_scan_exact={thread_scan_exact}");
    println!("backend={}", backend.describe());
    println!("target_arch={}", std::env::consts::ARCH);
    println!("observed_cpu_start={cpu_start}");
    println!("scanned_orbs={scans}");
    println!("elapsed_ns={}", elapsed.as_nanos());
    println!("query_p50_ns={}", percentile(&durations, 0.50));
    println!("query_p95_ns={}", percentile(&durations, 0.95));
    println!("query_p99_ns={}", percentile(&durations, 0.99));
    println!("orbs_per_sec={orbs_per_sec:.3}");
    println!("effective_gib_per_sec={gib_per_sec:.6}");
    println!("top1_exact={exact}/{rounds}");
    println!("progressive_top8_exact=PASS");
    println!("progressive_candidates={}", progressive_stats.candidates);
    println!(
        "progressive_reject_32b={}",
        progressive_stats.rejected_after_32b
    );
    println!(
        "progressive_reject_64b={}",
        progressive_stats.rejected_after_64b
    );
    println!(
        "progressive_full_128b={}",
        progressive_stats.full_128b_scores
    );

    let quality_index = count / 2;
    for flips in [1usize, 8, 32, 64, 128, 256, 384] {
        let mut words = *bank[quality_index].words();
        for bit in 0..flips {
            // Odd multiplier creates unique positions modulo 1024.
            let position = (405 * bit + 137) & 1023;
            words[position / 64] ^= 1u64 << (position % 64);
        }
        let query = Orb1024::from_words(words);
        let hit = top1(&bank, &query).ok_or("empty bank")?;
        println!(
            "quality_flips_{flips}=target:{quality_index},hit:{},score:{},survived:{}",
            hit.0,
            hit.1,
            hit.0 == quality_index
        );
    }
    println!("null_top1_index={}", null_hit.0);
    println!("null_top1_score={}", null_hit.1);
    println!("observed_cpu_end={}", observed_cpu());
    Ok(())
}

fn main() -> Result<(), String> {
    run()
}

#[cfg(test)]
mod tests {
    use super::processor_field;

    #[test]
    fn processor_field_survives_spaces_and_parentheses_in_comm() {
        let mut fields = (3..=52).map(|i| i.to_string()).collect::<Vec<_>>();
        fields[39 - 3] = "17".to_owned();
        let stat = format!("4242 (gel (a) b) c) {}\n", fields.join(" "));
        assert_eq!(processor_field(&stat), Some(17));
        assert_eq!(processor_field("4242 (short) R 1 2 3"), None);
        assert_eq!(processor_field("no parenthesis at all"), None);
    }
}
