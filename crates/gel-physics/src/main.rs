#![forbid(unsafe_code)]

//! F0 memory-physics harness. It reports nanoseconds and bytes/s only; cycles
//! are intentionally absent unless measured by an external hardware counter.
//!
//! Barrier placement (output tag `GEL_PHYSICS_F0_V3`): `pointer_chase` keeps a
//! `black_box` on every step because the chain is dependent by design.
//! `sequential_gib_s` takes one barrier per round and `random_orb_fetch_ns`
//! one per fetch, both on the slice itself: the element loops can vectorize;
//! whether a row is memory-bound is decided by comparing it with the 48 KiB
//! row as docs/PERFORMANCE.md requires. Probe buffers are 64-byte aligned
//! (`Line`, `Record`) so wide vector loads never split a cache line.
//! `GEL_PHYSICS_F0_V1` placed a barrier on every element; its 48 KiB
//! (L1-resident) sequential result sat at about 40 GiB/s, a cap from which
//! the RAM-sized rows in docs/SILICON-2026-09-03.md (28.0 and 30.8 GiB/s)
//! cannot be separated. `GEL_PHYSICS_F0_V2` removed that barrier but used
//! 8-byte-aligned buffers; built with `-C target-cpu=native` its 48 KiB row
//! fell to 42 GiB/s against 105 GiB/s for the SSE2 baseline build, because
//! 64-byte vector loads split cache lines. Compare rows only within one tag.

use gel_core::splitmix64;
use std::collections::BTreeSet;
use std::fs;
use std::hint::black_box;
use std::time::{Duration, Instant};

const DEFAULT_SIZES: &[usize] = &[
    48 << 10,
    1 << 20,
    6 << 20,
    12 << 20,
    24 << 20,
    64 << 20,
    256 << 20,
];

fn main() -> Result<(), String> {
    let cpu_start = observed_cpu();
    let repeats = arg_usize(1, 5, "REPEATS")?;
    if repeats < 3 {
        return Err("REPEATS must be >= 3".into());
    }
    println!("GEL_PHYSICS_F0_V3");
    println!("timing_authority=std::time::Instant_ns");
    println!("cycles=NOT_REPORTED");
    println!(
        "sequential_probe=eight_independent_lane_accumulators_no_per_element_barrier_aligned64"
    );
    println!("probe_buffer_alignment=64");
    report_linux_topology();
    println!("observed_cpu_start={cpu_start}");
    println!("--- pointer_chase ---");
    for &bytes in DEFAULT_SIZES {
        let samples = repeat(repeats, || pointer_chase(bytes));
        println!(
            "bytes={bytes} min_ns_per_step={:.3} p50_ns_per_step={:.3} p99_ns_per_step={:.3}",
            samples[0],
            percentile(&samples, 0.50),
            percentile(&samples, 0.99)
        );
    }
    println!("--- sequential_bandwidth ---");
    for &bytes in DEFAULT_SIZES {
        let samples = repeat(repeats, || sequential_gib_s(bytes));
        println!(
            "bytes={bytes} max_gib_s={:.3} p50_gib_s={:.3}",
            samples[samples.len() - 1],
            percentile(&samples, 0.50)
        );
    }
    println!("--- random_orb_fetch ---");
    for fetch_bytes in [32usize, 64, 128] {
        let samples = repeat(repeats, || random_orb_fetch_ns(64 << 20, fetch_bytes));
        println!(
            "working_set_bytes={} fetch_bytes={} min_ns={:.3} p50_ns={:.3} p99_ns={:.3}",
            64 << 20,
            fetch_bytes,
            samples[0],
            percentile(&samples, 0.50),
            percentile(&samples, 0.99)
        );
    }
    println!("observed_cpu_end={}", observed_cpu());
    Ok(())
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
    fs::read_to_string("/proc/self/stat")
        .ok()
        .and_then(|stat| processor_field(&stat))
        .map_or_else(|| "unavailable".to_owned(), |cpu| cpu.to_string())
}

fn arg_usize(index: usize, default: usize, name: &str) -> Result<usize, String> {
    let Some(raw) = std::env::args_os().nth(index) else {
        return Ok(default);
    };
    let raw = raw
        .into_string()
        .map_err(|bad| format!("{name} is not valid UTF-8: {}", bad.to_string_lossy()))?;
    raw.parse::<usize>()
        .map_err(|_| format!("{name} must be a positive integer"))
        .and_then(|v| {
            if v == 0 {
                Err(format!("{name} must be > 0"))
            } else {
                Ok(v)
            }
        })
}

fn repeat<F>(n: usize, mut f: F) -> Vec<f64>
where
    F: FnMut() -> f64,
{
    let mut samples = Vec::with_capacity(n);
    for _ in 0..n {
        samples.push(f());
    }
    samples.sort_by(|a, b| a.total_cmp(b));
    samples
}

fn percentile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let index = ((sorted.len() - 1) as f64 * q).ceil() as usize;
    sorted[index.min(sorted.len() - 1)]
}

fn pointer_chase(bytes: usize) -> f64 {
    let slots = (bytes / core::mem::size_of::<usize>()).max(2);
    let mut order = (0..slots).collect::<Vec<_>>();
    let mut state = 0x4745_4c00_5048_5953u64 ^ bytes as u64;
    for i in (1..slots).rev() {
        state = splitmix64(state);
        let j = (state as usize) % (i + 1);
        order.swap(i, j);
    }
    let mut next = vec![0usize; slots];
    for pair in order.windows(2) {
        next[pair[0]] = pair[1];
    }
    next[*order.last().expect("nonempty")] = order[0];
    let steps = slots.saturating_mul(4).max(32_768);
    let mut cursor = order[0];
    let started = Instant::now();
    for _ in 0..steps {
        cursor = black_box(next[black_box(cursor)]);
    }
    black_box(cursor);
    started.elapsed().as_nanos() as f64 / steps as f64
}

/// One 64-byte-aligned cache line of eight words. Probe buffers are vectors
/// of these so that wide vector loads (AVX2/AVX-512 emitted for
/// `-C target-cpu=native`) never cross a cache line. With an 8-byte-aligned
/// `Vec<u64>` the native build's 48 KiB sequential row fell to 42 GiB/s
/// against 105 GiB/s for the SSE2 baseline build on the same core.
#[repr(C, align(64))]
#[derive(Clone, Copy)]
struct Line([u64; 8]);

/// One 64-byte-aligned 128-byte record, the ORB-sized fetch unit.
#[repr(C, align(64))]
#[derive(Clone, Copy)]
struct Record([u64; 16]);

const _: () = assert!(core::mem::size_of::<Line>() == 64);
const _: () = assert!(core::mem::align_of::<Line>() == 64);
const _: () = assert!(core::mem::size_of::<Record>() == 128);
const _: () = assert!(core::mem::align_of::<Record>() == 64);

fn aligned_lines(lines: usize, salt: u64) -> Vec<Line> {
    (0..lines)
        .map(|l| {
            let mut words = [0u64; 8];
            for (i, word) in words.iter_mut().enumerate() {
                *word = splitmix64((l * 8 + i) as u64 ^ salt);
            }
            Line(words)
        })
        .collect()
}

fn sequential_gib_s(bytes: usize) -> f64 {
    let lines = (bytes / 64).max(1);
    let data = aligned_lines(lines, 0);
    debug_assert_eq!(data.as_ptr() as usize % 64, 0);
    let rounds = ((64usize << 20) / bytes.max(1)).max(1);
    let started = Instant::now();
    let mut sink = 0u64;
    for _ in 0..rounds {
        // One barrier per round on the slice itself: the compiler must assume
        // the buffer changed and re-read it, while the element loop carries no
        // barrier so the reduction can vectorize.
        let d = black_box(data.as_slice());
        sink ^= lane_reduce(d);
    }
    black_box(sink);
    let elapsed = nonzero(started.elapsed());
    let touched = data.len() as f64 * 64.0 * rounds as f64;
    touched / elapsed.as_secs_f64() / (1024.0 * 1024.0 * 1024.0)
}

/// XOR of the lane sums over `d`. Lines are consumed in groups of eight with
/// one accumulator array per group slot, so the compiled loop carries eight
/// independent dependency chains instead of one. With a single accumulator the
/// native build reduced to one `vpaddq zmm` chain per line and reached only
/// 43 GiB/s from L1 on this machine; the SSE2 baseline build, which had four
/// independent chains, reached 105 GiB/s.
#[inline]
fn lane_reduce(d: &[Line]) -> u64 {
    let mut acc = [[0u64; 8]; 8];
    let mut groups = d.chunks_exact(8);
    for group in &mut groups {
        for (a, line) in acc.iter_mut().zip(group.iter()) {
            for (x, &y) in a.iter_mut().zip(line.0.iter()) {
                *x = x.wrapping_add(y);
            }
        }
    }
    for (a, line) in acc.iter_mut().zip(groups.remainder().iter()) {
        for (x, &y) in a.iter_mut().zip(line.0.iter()) {
            *x = x.wrapping_add(y);
        }
    }
    acc.iter().flatten().fold(0u64, |s, &a| s ^ a)
}

fn random_orb_fetch_ns(working_set_bytes: usize, fetch_bytes: usize) -> f64 {
    const ORB_BYTES: usize = 128;
    let records = (working_set_bytes / ORB_BYTES).max(1024);
    let data = (0..records)
        .map(|r| {
            let mut words = [0u64; 16];
            for (i, word) in words.iter_mut().enumerate() {
                *word = splitmix64((r * 16 + i) as u64 ^ 0x4f52_4246_4554_4348);
            }
            Record(words)
        })
        .collect::<Vec<_>>();
    debug_assert_eq!(data.as_ptr() as usize % 64, 0);
    let reads = 65_536usize;
    let mut indices = Vec::with_capacity(reads);
    let mut state = 7u64;
    for _ in 0..reads {
        state = splitmix64(state);
        indices.push((state as usize) % records);
    }
    let started = Instant::now();
    // Throughput over independent fetches (memory-level parallelism), not a
    // dependent-latency measurement. The fetch width is a compile-time constant
    // so the per-fetch reduction is straight-line code.
    let sink = match fetch_bytes {
        32 => fetch_xor::<4>(&data, &indices),
        64 => fetch_xor::<8>(&data, &indices),
        _ => fetch_xor::<16>(&data, &indices),
    };
    black_box(sink);
    started.elapsed().as_nanos() as f64 / reads as f64
}

#[inline]
fn fetch_xor<const N: usize>(data: &[Record], indices: &[usize]) -> u64 {
    let mut sink = 0u64;
    for &index in indices {
        let rec: &[u64; N] = black_box(data[index].0[..N].try_into().expect("N <= 16"));
        sink ^= rec.iter().fold(0u64, |s, &x| s ^ x);
    }
    sink
}

fn nonzero(duration: Duration) -> Duration {
    if duration.is_zero() {
        Duration::from_nanos(1)
    } else {
        duration
    }
}

fn report_linux_topology() {
    let thp = fs::read_to_string("/sys/kernel/mm/transparent_hugepage/enabled")
        .map(|x| x.trim().to_owned())
        .unwrap_or_else(|_| "unavailable".into());
    println!("thp_policy={thp}");
    let mut domains = BTreeSet::new();
    if let Ok(cpus) = fs::read_dir("/sys/devices/system/cpu") {
        for entry in cpus.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if !name
                .strip_prefix("cpu")
                .is_some_and(|x| !x.is_empty() && x.bytes().all(|b| b.is_ascii_digit()))
            {
                continue;
            }
            let cache = entry.path().join("cache/index3");
            let size = fs::read_to_string(cache.join("size"))
                .ok()
                .map(|x| x.trim().to_owned());
            let shared = fs::read_to_string(cache.join("shared_cpu_list"))
                .ok()
                .map(|x| x.trim().to_owned());
            if let (Some(size), Some(shared)) = (size, shared) {
                domains.insert(format!("L3 size={size} cpus={shared}"));
            }
        }
    }
    if domains.is_empty() {
        println!("l3_domains=unavailable");
    }
    for domain in domains {
        println!("l3_domain={domain}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_uses_sorted_upper_rank() {
        let data = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(percentile(&data, 0.50), 3.0);
        assert_eq!(percentile(&data, 0.99), 5.0);
    }

    #[test]
    fn lane_reduce_matches_lane_model() {
        let data = aligned_lines(19, 9);
        let mut acc = [[0u64; 8]; 8];
        for (i, line) in data.iter().enumerate() {
            for (x, &y) in acc[i % 8].iter_mut().zip(line.0.iter()) {
                *x = x.wrapping_add(y);
            }
        }
        assert_eq!(
            lane_reduce(&data),
            acc.iter().flatten().fold(0u64, |s, &a| s ^ a)
        );
        assert_eq!(lane_reduce(&[]), 0);
        assert_eq!(
            lane_reduce(&data[..1]),
            data[0].0.iter().fold(0u64, |s, &x| s ^ x)
        );
    }

    #[test]
    fn probe_buffers_are_64_byte_aligned() {
        let lines = aligned_lines(3, 0);
        assert_eq!(lines.as_ptr() as usize % 64, 0);
        let records = vec![Record([0; 16]); 3];
        assert_eq!(records.as_ptr() as usize % 64, 0);
        assert_eq!(core::mem::size_of::<Line>(), 64);
        assert_eq!(core::mem::size_of::<Record>(), 128);
    }

    #[test]
    fn processor_field_survives_spaces_and_parentheses_in_comm() {
        let mut fields = (3..=52).map(|i| i.to_string()).collect::<Vec<_>>();
        fields[39 - 3] = "17".to_owned();
        let stat = format!("4242 (gel (a) b) c) {}\n", fields.join(" "));
        assert_eq!(processor_field(&stat), Some(17));
        assert_eq!(processor_field("4242 (short) R 1 2 3"), None);
        assert_eq!(processor_field("no parenthesis at all"), None);
    }

    #[test]
    fn tiny_physics_probes_produce_finite_positive_values() {
        assert!(pointer_chase(48 << 10).is_finite());
        assert!(sequential_gib_s(48 << 10) > 0.0);
        assert!(random_orb_fetch_ns(1 << 20, 32) > 0.0);
    }
}
