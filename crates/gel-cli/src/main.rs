#![forbid(unsafe_code)]

use gel_core::{splitmix64, Generation, ORB_WORDS};
use gel_orb::Orb1024;
use gel_reader::{apply_view, invert_view, reader16, top1, top_k, top_k_progressive, ViewSpec};
use gel_store::{verify_file, OpenLimits, RamStore};
use gel_structural::{EncodedOrb, ParentRef, StructuralMetrics};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

type CliResult<T> = Result<T, Box<dyn std::error::Error>>;

fn sample(seed: u64) -> Orb1024 {
    let mut words = [0u64; ORB_WORDS];
    for (i, w) in words.iter_mut().enumerate() {
        *w = splitmix64(seed.wrapping_add(i as u64));
    }
    Orb1024::from_words(words)
}

/// Creates a private directory `gel-cli-selftest-<pid>-<16 hex>` in the OS
/// temporary directory. The hex token is splitmix64 of the pid XOR the
/// nanoseconds since the Unix epoch, so the name is not guessable from the
/// pid alone; `create_dir` fails on an existing entry and is retried.
fn create_selftest_dir() -> CliResult<PathBuf> {
    let pid = u64::from(std::process::id());
    for attempt in 0..128u64 {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX))
            .unwrap_or(0);
        let token = splitmix64(pid ^ nanos ^ attempt);
        let dir = std::env::temp_dir().join(format!("gel-cli-selftest-{pid}-{token:016x}"));
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err("could not create a private selftest directory".into())
}

fn persistence_selftest(dir: &std::path::Path, bank: &[Orb1024]) -> CliResult<()> {
    let path = dir.join("store.gel");
    let store = RamStore::from_orbs(bank.to_vec(), Generation(1));
    store.write_atomic(&path)?;
    let reopened = RamStore::open_verified(&path)?;
    if reopened.orbs() != bank {
        return Err("persistence selftest failed".into());
    }
    let verified = verify_file(&path, OpenLimits::UNLIMITED)?;
    if verified != reopened.header() {
        return Err("streaming verify diverged from open_verified".into());
    }
    Ok(())
}

fn selftest() -> CliResult<()> {
    let a = sample(1);
    let views = [
        ViewSpec::identity(),
        ViewSpec::reverse(),
        ViewSpec::rotate(257),
        ViewSpec::xor_mask(0x4745_4c01),
        ViewSpec::affine(3, 11)?,
        ViewSpec::affine_masked(5, 37, 0x4745_4c02)?,
    ];
    for view in views {
        let encoded = apply_view(&a, view)?;
        let decoded = invert_view(&encoded, view)?;
        if decoded != a {
            return Err(format!("view roundtrip failed: {view:?}").into());
        }
    }

    let mut near = a;
    for bit in 0..12 {
        near.words_mut()[bit >> 6] ^= 1u64 << (bit & 63);
    }
    let r16 = reader16(&a, &near);
    if r16.contingency.mismatches() != 12 {
        return Err("Reader16 contingency failed".into());
    }

    let bank = vec![sample(10), sample(20), a, sample(40)];
    let hit = top1(&bank, &a).ok_or("empty bank")?;
    if hit.0 != 2 || hit.1 != 1024 {
        return Err("top1 selftest failed".into());
    }
    if top_k(&bank, &a, 2) != top_k_progressive(&bank, &a, 2).0 {
        return Err("progressive Top-K diverged from full Top-K".into());
    }

    let structural = EncodedOrb::encode_against(&near, &a, ParentRef::Prototype(0), 0)?;
    if structural.decode(Some(&a))? != near {
        return Err("structural exact decode failed".into());
    }
    let metrics = StructuralMetrics::from_encoded(&structural, true)?;

    let dir = create_selftest_dir()?;
    let persisted = persistence_selftest(&dir, &bank);
    // Best-effort cleanup; a leftover directory does not change the verdict.
    let _ = std::fs::remove_dir_all(&dir);
    persisted?;

    println!("GEL_SELFTEST_V2=PASS");
    println!("ORB_BITS=1024");
    println!("ORB_BYTES=128");
    println!("VIEW_ROUNDTRIPS={}/{}", views.len(), views.len());
    println!("READER16=PASS");
    println!("PROGRESSIVE_EXACT=PASS");
    println!("STRUCTURAL_EXACT=PASS");
    println!("STRUCTURAL_DER_LOCAL={:.6}", metrics.der_local);
    println!("PERSISTENCE_V2_CRC64=PASS");
    Ok(())
}

fn main() -> CliResult<()> {
    let mut args = std::env::args_os().skip(1);
    let command = args.next();
    let command = match &command {
        None => None,
        Some(word) => Some(
            word.to_str()
                .ok_or("command is not valid UTF-8; use selftest or verify")?,
        ),
    };
    match command {
        None | Some("selftest") => selftest(),
        Some("verify") => {
            // The path stays an OsString so non-UTF-8 file names work.
            let path = PathBuf::from(args.next().ok_or("usage: gel-cli verify FILE.gel")?);
            let header = verify_file(&path, OpenLimits::UNLIMITED)?;
            println!("GEL_VERIFY=PASS");
            println!("format_version={}", header.version);
            println!("records={}", header.record_count);
            println!("generation={}", header.generation.0);
            Ok(())
        }
        Some(other) => Err(format!("unknown command: {other}; use selftest or verify").into()),
    }
}
