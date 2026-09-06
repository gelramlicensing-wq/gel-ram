#![forbid(unsafe_code)]

//! Deterministic CPU-only correctness audit, not an inference benchmark.
//! FP16 and Q8 are explicitly defined reference conversions. GEL transports
//! the encoded bytes unchanged; it does not itself quantize numeric values.

use gel_core::Generation;
use gel_orb::Orb1024;
use gel_reader::{top1, top1_threads, top_k, top_k_progressive};
use gel_store::{verify_file, OpenLimits, RamStore};
use gel_structural::{EncodedOrb, ParentRef};
use std::fs;
use std::io::Read;
use std::path::Path;

const SEED: u64 = 0x47e1_2026_0906_0021;

// Independent generator: do not reuse the production splitmix64 generator.
fn random(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn transport(root: &Path, label: &str, bytes: &[u8]) -> Vec<u8> {
    let path = root.join(format!("{label}.gel"));
    let mut padded = bytes.to_vec();
    padded.resize(bytes.len().div_ceil(128) * 128, 0);
    let records: Vec<_> = padded
        .chunks_exact(128)
        .map(|b| Orb1024::from_le_bytes(b).unwrap())
        .collect();
    RamStore::from_orbs(records.clone(), Generation(1))
        .write_atomic(&path)
        .unwrap();
    let verification = verify_file(&path, OpenLimits::SAFE_DEFAULT).unwrap();
    let loaded = RamStore::open_verified(&path).unwrap();
    assert_eq!(verification.header, loaded.header());
    assert_eq!(loaded.orbs(), records);
    let recovered: Vec<_> = loaded.orbs().iter().flat_map(|o| o.to_le_bytes()).collect();
    let exact_bytes = bytes.iter().zip(&recovered).filter(|(a, b)| a == b).count();
    assert_eq!(exact_bytes, bytes.len());
    assert_eq!(recovered, padded);
    println!("transport={label} input_bytes={} file_bytes={} exact_bytes={exact_bytes} byte_accuracy=1.000000000", bytes.len(), fs::metadata(&path).unwrap().len());
    recovered[..bytes.len()].to_vec()
}

// All finite positive IEEE binary16 values are exactly representable in f32.
fn half_value(bits: u16) -> f32 {
    let exponent = (bits >> 10) & 31;
    let fraction = bits & 1023;
    assert!(bits < 0x7c00);
    if exponent == 0 {
        fraction as f32 * 2.0f32.powi(-24)
    } else {
        (1024 + fraction) as f32 * 2.0f32.powi(exponent as i32 - 25)
    }
}

// Reference nearest-even rounding by searching the complete finite table.
// Inputs outside finite binary16 range are deliberately excluded, not clipped.
fn half_encode(value: f32, table: &[f32]) -> u16 {
    assert!(value.is_finite() && value.abs() <= 65504.0);
    let magnitude = value.abs();
    let hi = table.partition_point(|v| *v < magnitude);
    let code = if hi == 0 || table[hi] == magnitude {
        hi
    } else {
        let low_distance = magnitude as f64 - table[hi - 1] as f64;
        let high_distance = table[hi] as f64 - magnitude as f64;
        if low_distance < high_distance || (low_distance == high_distance && (hi - 1) % 2 == 0) {
            hi - 1
        } else {
            hi
        }
    };
    code as u16 | if value.is_sign_negative() { 0x8000 } else { 0 }
}

fn half_decode(code: u16, table: &[f32]) -> f32 {
    let magnitude = table[(code & 0x7fff) as usize];
    if code & 0x8000 != 0 {
        -magnitude
    } else {
        magnitude
    }
}

fn validate_half_reference(table: &[f32]) {
    assert_eq!(half_value(1), 2.0f32.powi(-24));
    assert_eq!(half_value(0x3c00), 1.0);
    assert_eq!(half_value(0x7bff), 65504.0);
    for code in 0u16..0x7c00 {
        for sign in [0, 0x8000] {
            assert_eq!(
                half_encode(half_decode(code | sign, table), table),
                code | sign
            );
        }
    }
    // Each adjacent finite pair's exact midpoint exercises ties-to-even.
    for low in 0..table.len() - 1 {
        let mid = ((table[low] as f64 + table[low + 1] as f64) / 2.0) as f32;
        let expected = if low % 2 == 0 { low } else { low + 1 };
        assert_eq!(half_encode(mid, table), expected as u16);
    }
    println!("fp16_reference_finite_roundtrips=63488/63488 midpoint_ties=31743/31743");
}

// Reference Q8: blocks of 32, symmetric signed [-127,127], one f32
// absmax/127 scale per block, nearest-even rounding, no zero point.
// NOT GGUF Q8_0, GPTQ, AWQ, or a hardware integer-matmul benchmark.
fn q8_encode(values: &[f32]) -> Vec<u8> {
    assert_eq!(values.len() % 32, 0);
    let mut bytes = Vec::new();
    for block in values.chunks_exact(32) {
        let max = block.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let scale = if max == 0.0 { 1.0 } else { max / 127.0 };
        assert!(scale.is_finite() && scale > 0.0);
        bytes.extend_from_slice(&scale.to_le_bytes());
        for value in block {
            let q = (*value as f64 / scale as f64)
                .round_ties_even()
                .clamp(-127.0, 127.0) as i8;
            bytes.push(q as u8);
        }
    }
    bytes
}

fn q8_decode(bytes: &[u8]) -> Vec<f32> {
    assert_eq!(bytes.len() % 36, 0);
    let mut values = Vec::new();
    for block in bytes.chunks_exact(36) {
        let scale = f32::from_le_bytes(block[..4].try_into().unwrap());
        values.extend(block[4..].iter().map(|q| (*q as i8) as f32 * scale));
    }
    values
}

fn metrics(dataset: &str, format: &str, source: &[f32], decoded: &[f32], encoded_bytes: usize) {
    assert_eq!(source.len(), decoded.len());
    let mut error2 = 0.0;
    let mut source2 = 0.0;
    let mut max_abs = 0.0f64;
    let mut exact = 0usize;
    for (&a, &b) in source.iter().zip(decoded) {
        assert!(a.is_finite() && b.is_finite());
        let error = a as f64 - b as f64;
        error2 += error * error;
        source2 += (a as f64).powi(2);
        max_abs = max_abs.max(error.abs());
        exact += usize::from(a.to_bits() == b.to_bits());
    }
    let relative_l2 = if source2 == 0.0 {
        0.0
    } else {
        (error2 / source2).sqrt()
    };
    println!("dataset={dataset} format={format} values={} encoded_bytes={encoded_bytes} numeric_bit_exact={exact} exact_fraction={:.9} rmse={:.9e} relative_l2={relative_l2:.9e} max_abs_error={max_abs:.9e}", source.len(), exact as f64/source.len() as f64, (error2/source.len() as f64).sqrt());
}

fn numeric_comparison(root: &Path) {
    let table: Vec<_> = (0u16..0x7c00).map(half_value).collect();
    validate_half_reference(&table);
    let mut state = SEED;
    for dataset in ["uniform", "wide_range", "block_outliers", "zeros"] {
        let source: Vec<_> = (0..32768)
            .map(|i| {
                let unit = (random(&mut state) >> 40) as f32 / 16_777_216.0;
                let signed = 2.0 * unit - 1.0;
                match dataset {
                    "wide_range" => signed * 2.0f32.powi((i % 40) - 24),
                    "block_outliers" => {
                        if i % 32 == 0 {
                            signed * 1000.0
                        } else {
                            signed * 0.01
                        }
                    }
                    "zeros" => {
                        if i % 2 == 0 {
                            0.0
                        } else {
                            -0.0
                        }
                    }
                    _ => signed,
                }
            })
            .collect();
        compare_numeric(root, dataset, &source, &table);
    }
}

fn compare_numeric(root: &Path, dataset: &str, source: &[f32], table: &[f32]) {
    let raw: Vec<_> = source.iter().flat_map(|v| v.to_le_bytes()).collect();
    let recovered = transport(root, &format!("{dataset}-fp32"), &raw);
    let decoded: Vec<_> = recovered
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    metrics(dataset, "GEL_FP32_bits", source, &decoded, raw.len());
    let half: Vec<_> = source.iter().map(|v| half_encode(*v, table)).collect();
    let half_bytes: Vec<_> = half.iter().flat_map(|v| v.to_le_bytes()).collect();
    let recovered = transport(root, &format!("{dataset}-fp16"), &half_bytes);
    let decoded: Vec<_> = recovered
        .chunks_exact(2)
        .map(|b| half_decode(u16::from_le_bytes(b.try_into().unwrap()), table))
        .collect();
    metrics(
        dataset,
        "FP16_reference",
        source,
        &decoded,
        half_bytes.len(),
    );
    let quantized = q8_encode(source);
    let recovered = transport(root, &format!("{dataset}-q8"), &quantized);
    metrics(
        dataset,
        "Q8_symmetric32_f32scale",
        source,
        &q8_decode(&recovered),
        quantized.len(),
    );
}

fn structural_and_ranking() {
    let mut state = SEED;
    let mut structural_cases = 0;
    for _ in 0..64 {
        let predictor = Orb1024::from_words(std::array::from_fn(|_| random(&mut state)));
        let mut target = predictor;
        for flips in 0..=1024 {
            if flips > 0 {
                // Odd multiplier visits each of the 1024 bit positions once.
                let bit = ((flips - 1) * 661) % 1024;
                target.words_mut()[bit / 64] ^= 1 << (bit % 64);
            }
            for parent in [ParentRef::Prototype(0), ParentRef::SegmentLocal(0)] {
                for depth in [0, 1] {
                    let encoded =
                        EncodedOrb::encode_against(&target, &predictor, parent, depth).unwrap();
                    assert_eq!(
                        encoded.decode(Some(&predictor)).unwrap().to_le_bytes(),
                        target.to_le_bytes()
                    );
                    structural_cases += 1;
                }
            }
        }
    }
    println!("structural_exact={structural_cases}/{structural_cases} fraction=1.000000000");
    let mut queries = 0;
    for _ in 0..64 {
        let mut bank: Vec<_> = (0..128)
            .map(|_| Orb1024::from_words(std::array::from_fn(|_| random(&mut state))))
            .collect();
        bank[127] = bank[0]; // Explicit tie, including last chunk boundary.
        for query in [
            bank[0],
            bank[63],
            Orb1024::ZERO,
            Orb1024::from_words([u64::MAX; 16]),
        ] {
            // Independent byte/bit oracle, no production popcount or scoring.
            let q = query.to_le_bytes();
            let mut expected: Vec<_> = bank
                .iter()
                .enumerate()
                .map(|(index, orb)| {
                    let bytes = orb.to_le_bytes();
                    let mut score = 0u16;
                    for bit in 0..1024 {
                        score += u16::from(
                            (bytes[bit / 8] >> (bit % 8)) & 1 == (q[bit / 8] >> (bit % 8)) & 1,
                        );
                    }
                    (index, score)
                })
                .collect();
            expected.sort_by_key(|(index, score)| (std::cmp::Reverse(*score), *index));
            let (index, score, _) = top1(&bank, &query).unwrap();
            assert_eq!((index, score), expected[0]);
            for threads in [2, 4, usize::MAX] {
                let (index, score, _) = top1_threads(&bank, &query, threads).unwrap();
                assert_eq!((index, score), expected[0]);
            }
            for k in [0, 1, 8, 128, 129] {
                assert_eq!(top_k(&bank, &query, k), expected[..k.min(bank.len())]);
                assert_eq!(
                    top_k_progressive(&bank, &query, k).0,
                    expected[..k.min(bank.len())]
                );
            }
            queries += 1;
        }
    }
    println!("independent_ranking_queries={queries}/{queries} fraction=1.000000000");
}

fn large_stores(root: &Path) {
    for mib in [16usize, 256, 512] {
        let count = mib * 1024 * 1024 / 128;
        let mut state = SEED;
        let records: Vec<_> = (0..count)
            .map(|_| Orb1024::from_words(std::array::from_fn(|_| random(&mut state))))
            .collect();
        let path = root.join(format!("large-{mib}MiB.gel"));
        let store = RamStore::from_orbs(records, Generation(21));
        store.write_atomic(&path).unwrap();
        drop(store);
        let limits = OpenLimits {
            max_records: count as u64,
            max_file_bytes: (count * 128 + 64) as u64,
        };
        if mib > 256 {
            assert!(matches!(
                RamStore::open_verified(&path),
                Err(gel_core::GelError::LimitExceeded("file bytes"))
            ));
        }
        let loaded = if mib <= 256 {
            RamStore::open_verified(&path)
        } else {
            RamStore::open_verified_with_limits(&path, limits)
        }
        .unwrap();
        let verification = verify_file(&path, limits).unwrap();
        assert_eq!(verification.header, loaded.header());
        assert_eq!(loaded.len(), count);
        let mut state = SEED;
        for orb in loaded.orbs() {
            // Re-generate the original independently of the loaded bytes.
            for word in orb.words() {
                assert_eq!(*word, random(&mut state));
            }
        }
        println!("large_store_mib={mib} exact_records={count}/{count} byte_accuracy=1.000000000 default_budget={}", if mib <= 256 { "ACCEPT" } else { "REJECT_explicit_budget_ACCEPT" });
    }
}

// Caller-supplied corpus format: u32 LE count, u32 LE dimension, contiguous
// FP32 LE values, then one u32 label per row. Labels do not affect conversion.
fn parse_fp32_dump(bytes: &[u8]) -> Result<Vec<f32>, &'static str> {
    if bytes.len() < 8 || bytes.len() > 16 * 1024 * 1024 {
        return Err("dump must be 8 bytes to 16 MiB");
    }
    let count = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
    let dim = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let values = count.checked_mul(dim).ok_or("shape overflow")?;
    let payload = values.checked_mul(4).ok_or("payload overflow")?;
    let labels = count.checked_mul(4).ok_or("labels overflow")?;
    let size = payload
        .checked_add(labels)
        .and_then(|n| n.checked_add(8))
        .ok_or("size overflow")?;
    if count == 0 || dim == 0 || dim % 32 != 0 || size != bytes.len() {
        return Err("invalid shape, size or Q8 block alignment");
    }
    let values: Vec<_> = bytes[8..8 + payload]
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    if values.iter().any(|x| !x.is_finite() || x.abs() > 65504.0) {
        return Err("numeric comparison requires finite binary16-range input");
    }
    if values.chunks_exact(32).any(|b| {
        let m = b.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        m != 0.0 && m / 127.0 == 0.0
    }) {
        return Err("Q8 scale underflow is outside this reference format");
    }
    Ok(values)
}

fn main() {
    let mut args = std::env::args_os().skip(1);
    let root = args
        .next()
        .expect("provide a NEW output directory for audit fixtures");
    let mut large = false;
    let mut dump = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--large") => large = true,
            Some("--fp32-dump") => dump = Some(args.next().expect("missing FP32 dump path")),
            _ => panic!("expected --large or --fp32-dump PATH"),
        }
    }
    let root = Path::new(&root);
    fs::create_dir(root).expect("output directory must not already exist");
    println!("GEL_DATA_INTEGRITY_V1 seed={SEED:#018x} cpu_only=true built_in_fixtures=synthetic");
    println!("threshold=1.0_bit_exact_for_GEL no_inference_or_quantization_speed_claim");
    let half_patterns: Vec<_> = (0u16..=u16::MAX).flat_map(u16::to_le_bytes).collect();
    transport(root, "all-binary16-patterns-including-nan", &half_patterns);
    let mut state = SEED;
    let raw: Vec<_> = (0..1_048_576)
        .flat_map(|_| (random(&mut state) as u32).to_le_bytes())
        .collect();
    transport(root, "random-u32-patterns", &raw);
    let specials: Vec<_> = [
        0u32, 0x80000000, 0x7f800000, 0xff800000, 0x7fc00001, 0x7f800001, 1, 0x007fffff,
        0x00800000, 0x7f7fffff,
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect();
    transport(root, "binary32-special-patterns", &specials);
    numeric_comparison(root);
    if let Some(path) = dump {
        let mut bytes = Vec::new();
        fs::File::open(path)
            .expect("open input dump")
            .take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .unwrap();
        let values = parse_fp32_dump(&bytes).expect("invalid FP32 dump");
        println!(
            "external_fp32_dump_bytes={} crc64={:016x} provenance=caller_supplied_not_regenerated",
            bytes.len(),
            gel_core::crc64_ecma(&bytes)
        );
        let table: Vec<_> = (0u16..0x7c00).map(half_value).collect();
        compare_numeric(root, "external_fp32_dump", &values, &table);
    }
    structural_and_ranking();
    if large {
        large_stores(root);
    }
    println!("GEL_DATA_INTEGRITY_ALL=PASS");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dump_fixture() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&32u32.to_le_bytes());
        for i in 0..64 {
            bytes.extend_from_slice(&(i as f32 / 64.0).to_le_bytes());
        }
        bytes.extend_from_slice(&[0; 8]);
        bytes
    }

    #[test]
    fn external_dump_is_shape_checked_without_using_labels() {
        let mut bytes = dump_fixture();
        let values = parse_fp32_dump(&bytes).unwrap();
        assert_eq!(values, (0..64).map(|i| i as f32 / 64.0).collect::<Vec<_>>());
        let end = bytes.len();
        bytes[end - 8..].fill(255);
        assert_eq!(parse_fp32_dump(&bytes).unwrap(), values);
        for len in 0..bytes.len() {
            assert!(parse_fp32_dump(&bytes[..len]).is_err());
        }
        bytes.push(0);
        assert!(parse_fp32_dump(&bytes).is_err());
    }

    #[test]
    fn external_dump_rejects_bad_shapes_nonfinite_values_and_scale_underflow() {
        for value in [f32::NAN, f32::INFINITY, -f32::INFINITY, 65505.0] {
            let mut bytes = dump_fixture();
            bytes[8..12].copy_from_slice(&value.to_le_bytes());
            assert!(parse_fp32_dump(&bytes).is_err());
        }
        for (count, dim) in [(0u32, 32u32), (2, 0), (2, 31), (u32::MAX, u32::MAX)] {
            let mut bytes = dump_fixture();
            bytes[..4].copy_from_slice(&count.to_le_bytes());
            bytes[4..8].copy_from_slice(&dim.to_le_bytes());
            assert!(parse_fp32_dump(&bytes).is_err());
        }
        let mut bytes = dump_fixture();
        bytes[8..8 + 128].fill(0);
        bytes[8..12].copy_from_slice(&1u32.to_le_bytes());
        assert!(parse_fp32_dump(&bytes).is_err());
    }

    #[test]
    fn q8_reference_has_exact_integer_endpoints_and_accounts_for_scale() {
        let mut values = vec![0.0f32; 32];
        values[0] = -127.0;
        values[1] = 127.0;
        values[2] = 0.5;
        values[3] = 1.5;
        values[4] = -0.5;
        values[5] = -1.5;
        let bytes = q8_encode(&values);
        assert_eq!(bytes.len(), 36);
        assert_eq!(&bytes[..4], &1.0f32.to_le_bytes());
        assert_eq!(
            &q8_decode(&bytes)[..6],
            &[-127.0, 127.0, 0.0, 2.0, 0.0, -2.0]
        );
        assert_eq!(q8_decode(&q8_encode(&[0.0; 32])), vec![0.0; 32]);
    }

    #[test]
    fn fp16_reference_is_exact_for_all_finite_codes_and_midpoints() {
        let table: Vec<_> = (0u16..0x7c00).map(half_value).collect();
        validate_half_reference(&table);
    }
}
