#![forbid(unsafe_code)]

//! RAM-resident GEL store with v2 header+payload CRC64 integrity.
//! The v0.2 writer emits only v2. v1 can still be read explicitly for migration.

use gel_core::{
    crc64_ecma, Checksum64, Crc64Ecma, GelError, Generation, GEL_FORMAT_VERSION, GEL_MAGIC_V1,
    GEL_MAGIC_V2, ORB_BITS, ORB_BYTES,
};
use gel_orb::Orb1024;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

pub const HEADER_BYTES: usize = 64;
pub const V2_HEADER_CRC_OFFSET: usize = 48;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreFormat {
    LegacyV1,
    V2,
}

impl StoreFormat {
    pub const fn version(self) -> u32 {
        match self {
            Self::LegacyV1 => 1,
            Self::V2 => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Header {
    pub version: u32,
    pub orb_bits: u32,
    pub record_bytes: u32,
    pub flags: u32,
    pub record_count: u64,
    pub generation: Generation,
    pub payload_crc64: u64,
    pub header_crc64: u64,
}

/// Result of validating an on-disk store.
///
/// `source_format` reports what was actually read. `header` is always the
/// protected v2 header that would be emitted by a subsequent write; legacy v1
/// payloads are upgraded in memory only after their legacy checksum passes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Verification {
    pub source_format: StoreFormat,
    pub header: Header,
}

impl Header {
    pub fn new(record_count: u64, generation: Generation, payload_crc64: u64) -> Self {
        let mut header = Self {
            version: GEL_FORMAT_VERSION,
            orb_bits: ORB_BITS as u32,
            record_bytes: ORB_BYTES as u32,
            flags: 0,
            record_count,
            generation,
            payload_crc64,
            header_crc64: 0,
        };
        header.header_crc64 = header.compute_crc64();
        header
    }

    pub fn encode(self) -> [u8; HEADER_BYTES] {
        let mut out = self.encode_without_header_crc();
        let crc = self.compute_crc64();
        out[V2_HEADER_CRC_OFFSET..V2_HEADER_CRC_OFFSET + 8].copy_from_slice(&crc.to_le_bytes());
        out
    }

    fn encode_without_header_crc(self) -> [u8; HEADER_BYTES] {
        let mut out = [0u8; HEADER_BYTES];
        out[0..8].copy_from_slice(&GEL_MAGIC_V2);
        out[8..12].copy_from_slice(&self.version.to_le_bytes());
        out[12..16].copy_from_slice(&self.orb_bits.to_le_bytes());
        out[16..20].copy_from_slice(&self.record_bytes.to_le_bytes());
        out[20..24].copy_from_slice(&self.flags.to_le_bytes());
        out[24..32].copy_from_slice(&self.record_count.to_le_bytes());
        out[32..40].copy_from_slice(&self.generation.0.to_le_bytes());
        out[40..48].copy_from_slice(&self.payload_crc64.to_le_bytes());
        // 48..56 is zero while CRC is calculated.
        // 56..64 is reserved and must remain zero.
        out
    }

    fn compute_crc64(self) -> u64 {
        crc64_ecma(&self.encode_without_header_crc())
    }

    pub fn decode(bytes: &[u8; HEADER_BYTES]) -> Result<Self, GelError> {
        if bytes[0..8] != GEL_MAGIC_V2 {
            return Err(GelError::InvalidMagic);
        }
        let version = le_u32(bytes, 8);
        if version != GEL_FORMAT_VERSION {
            return Err(GelError::UnsupportedVersion(version));
        }
        let orb_bits = le_u32(bytes, 12);
        let record_bytes = le_u32(bytes, 16);
        let flags = le_u32(bytes, 20);
        if orb_bits as usize != ORB_BITS {
            return Err(GelError::InvalidHeader("orb_bits != 1024"));
        }
        if record_bytes as usize != ORB_BYTES {
            return Err(GelError::InvalidHeader("record_bytes != 128"));
        }
        if flags != 0 {
            return Err(GelError::InvalidHeader("unsupported nonzero flags"));
        }
        if bytes[56..64].iter().any(|&b| b != 0) {
            return Err(GelError::InvalidHeader("reserved bytes must be zero"));
        }
        let header = Self {
            version,
            orb_bits,
            record_bytes,
            flags,
            record_count: le_u64(bytes, 24),
            generation: Generation(le_u64(bytes, 32)),
            payload_crc64: le_u64(bytes, 40),
            header_crc64: le_u64(bytes, 48),
        };
        if header.compute_crc64() != header.header_crc64 {
            return Err(GelError::CorruptHeader);
        }
        Ok(header)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OpenLimits {
    pub max_records: u64,
    pub max_file_bytes: u64,
}

impl OpenLimits {
    pub const UNLIMITED: Self = Self {
        max_records: u64::MAX,
        max_file_bytes: u64::MAX,
    };
}

#[derive(Debug)]
pub struct RamStore {
    header: Header,
    source_format: StoreFormat,
    orbs: Vec<Orb1024>,
}

impl RamStore {
    pub fn from_orbs(orbs: Vec<Orb1024>, generation: Generation) -> Self {
        let payload_crc64 = crc_orbs(&orbs);
        let header = Header::new(orbs.len() as u64, generation, payload_crc64);
        Self {
            header,
            source_format: StoreFormat::V2,
            orbs,
        }
    }

    pub fn header(&self) -> Header {
        self.header
    }
    pub fn source_format(&self) -> StoreFormat {
        self.source_format
    }
    pub fn orbs(&self) -> &[Orb1024] {
        &self.orbs
    }
    pub fn len(&self) -> usize {
        self.orbs.len()
    }
    pub fn is_empty(&self) -> bool {
        self.orbs.is_empty()
    }

    /// Single-writer atomic publish. On Unix the parent directory sync is part
    /// of the success contract. On non-Unix the file itself is synced but the
    /// standard library has no portable directory-fsync primitive.
    pub fn write_atomic(&self, path: impl AsRef<Path>) -> Result<(), GelError> {
        write_atomic_bytes(path.as_ref(), self.header, &self.orbs)
    }

    /// Rejects stale or equal generations before publishing. This assumes the
    /// project single-writer rule; it is not a multi-process compare-and-swap.
    pub fn write_if_newer(&self, path: impl AsRef<Path>) -> Result<(), GelError> {
        let path = path.as_ref();
        if path.exists() {
            let current = read_generation(path)?;
            if self.header.generation.0 <= current.0 {
                return Err(GelError::GenerationRollback {
                    current: current.0,
                    attempted: self.header.generation.0,
                });
            }
        }
        self.write_atomic(path)
    }

    pub fn open_verified(path: impl AsRef<Path>) -> Result<Self, GelError> {
        Self::open_verified_with_limits(path, OpenLimits::UNLIMITED)
    }

    pub fn open_verified_with_limits(
        path: impl AsRef<Path>,
        limits: OpenLimits,
    ) -> Result<Self, GelError> {
        let Validated {
            mut file,
            count,
            mut check,
        } = open_validated(path.as_ref(), limits)?;
        let mut orbs = Vec::new();
        orbs.try_reserve_exact(count)
            .map_err(|_| GelError::AllocationFailed)?;
        let mut bytes = [0u8; ORB_BYTES];
        for _ in 0..count {
            file.read_exact(&mut bytes)?;
            check.update(&bytes);
            orbs.push(Orb1024::from_le_bytes(&bytes)?);
        }
        let verification = check.finish()?;
        Ok(RamStore {
            header: verification.header,
            source_format: verification.source_format,
            orbs,
        })
    }
}

/// Verifies a store file without loading it.
///
/// Runs exactly the header, limit and file-length checks of
/// [`RamStore::open_verified_with_limits`], then streams the payload through
/// one 128-byte stack buffer to check the payload digest. No payload is
/// retained: memory use is constant regardless of file size. The report keeps
/// the actual on-disk source format separate from the protected v2 header a
/// loaded store would carry.
pub fn verify_file(path: impl AsRef<Path>, limits: OpenLimits) -> Result<Verification, GelError> {
    let Validated {
        mut file,
        count,
        mut check,
    } = open_validated(path.as_ref(), limits)?;
    let mut bytes = [0u8; ORB_BYTES];
    for _ in 0..count {
        file.read_exact(&mut bytes)?;
        check.update(&bytes);
    }
    check.finish()
}

/// A file whose header passed every check that precedes the payload.
struct Validated {
    file: File,
    count: usize,
    check: PayloadCheck,
}

/// Header fields the payload is checked against, with the running digests.
enum PayloadCheck {
    V2 {
        header: Header,
        crc: Crc64Ecma,
    },
    V1 {
        record_count: u64,
        generation: Generation,
        expected_checksum: u64,
        checksum: Checksum64,
        crc: Crc64Ecma,
    },
}

impl PayloadCheck {
    fn record_count(&self) -> u64 {
        match self {
            Self::V2 { header, .. } => header.record_count,
            Self::V1 { record_count, .. } => *record_count,
        }
    }

    fn update(&mut self, bytes: &[u8; ORB_BYTES]) {
        match self {
            Self::V2 { crc, .. } => crc.update(bytes),
            Self::V1 { checksum, crc, .. } => {
                checksum.update(bytes);
                crc.update(bytes);
            }
        }
    }

    /// Compares the digests with the header and returns the header a loaded
    /// store carries. A v1 header is upgraded so any subsequent write emits
    /// protected v2.
    fn finish(self) -> Result<Verification, GelError> {
        match self {
            Self::V2 { header, crc } => {
                if crc.finish() != header.payload_crc64 {
                    return Err(GelError::CorruptStore);
                }
                Ok(Verification {
                    source_format: StoreFormat::V2,
                    header,
                })
            }
            Self::V1 {
                record_count,
                generation,
                expected_checksum,
                checksum,
                crc,
            } => {
                if checksum.finish() != expected_checksum {
                    return Err(GelError::CorruptStore);
                }
                Ok(Verification {
                    source_format: StoreFormat::LegacyV1,
                    header: Header::new(record_count, generation, crc.finish()),
                })
            }
        }
    }
}

/// Opens the file and runs every check that precedes the payload, in this
/// order: file-size budget, magic, version, geometry, flags, reserved bytes,
/// header CRC (v2 only), record budget, exact file length. Shared by loading
/// and by streaming verification so both reject identically.
fn open_validated(path: &Path, limits: OpenLimits) -> Result<Validated, GelError> {
    let mut file = File::open(path)?;
    let actual_file = file.metadata()?.len();
    if actual_file > limits.max_file_bytes {
        return Err(GelError::LimitExceeded("file bytes"));
    }
    let mut header_bytes = [0u8; HEADER_BYTES];
    file.read_exact(&mut header_bytes)?;
    let check = if header_bytes[0..8] == GEL_MAGIC_V2 {
        PayloadCheck::V2 {
            header: Header::decode(&header_bytes)?,
            crc: Crc64Ecma::new(),
        }
    } else if header_bytes[0..8] == GEL_MAGIC_V1 {
        parse_v1_header(&header_bytes)?
    } else {
        return Err(GelError::InvalidMagic);
    };
    let record_count = check.record_count();
    validate_count_and_size(record_count, actual_file, limits)?;
    let count = usize::try_from(record_count)
        .map_err(|_| GelError::InvalidHeader("record_count too large"))?;
    Ok(Validated { file, count, check })
}

fn parse_v1_header(header_bytes: &[u8; HEADER_BYTES]) -> Result<PayloadCheck, GelError> {
    let version = le_u32(header_bytes, 8);
    if version != 1 {
        return Err(GelError::UnsupportedVersion(version));
    }
    if le_u32(header_bytes, 12) as usize != ORB_BITS
        || le_u32(header_bytes, 16) as usize != ORB_BYTES
    {
        return Err(GelError::InvalidHeader("legacy geometry mismatch"));
    }
    if header_bytes[20..24].iter().any(|&b| b != 0) || header_bytes[48..64].iter().any(|&b| b != 0)
    {
        return Err(GelError::InvalidHeader(
            "legacy reserved bytes must be zero",
        ));
    }
    Ok(PayloadCheck::V1 {
        record_count: le_u64(header_bytes, 24),
        generation: Generation(le_u64(header_bytes, 32)),
        expected_checksum: le_u64(header_bytes, 40),
        checksum: Checksum64::new(),
        crc: Crc64Ecma::new(),
    })
}

fn validate_count_and_size(
    record_count: u64,
    actual_file: u64,
    limits: OpenLimits,
) -> Result<(), GelError> {
    if record_count > limits.max_records {
        return Err(GelError::LimitExceeded("record count"));
    }
    let payload = record_count
        .checked_mul(ORB_BYTES as u64)
        .ok_or(GelError::InvalidHeader("payload size overflow"))?;
    let expected = (HEADER_BYTES as u64)
        .checked_add(payload)
        .ok_or(GelError::InvalidHeader("file size overflow"))?;
    if actual_file != expected {
        return Err(GelError::InvalidLength {
            expected: usize::try_from(expected).unwrap_or(usize::MAX),
            actual: usize::try_from(actual_file).unwrap_or(usize::MAX),
        });
    }
    Ok(())
}

fn write_atomic_bytes(path: &Path, header: Header, orbs: &[Orb1024]) -> Result<(), GelError> {
    let (tmp, mut file) = create_temp_file(path)?;
    let write_result = (|| -> Result<(), GelError> {
        file.write_all(&header.encode())?;
        for orb in orbs {
            file.write_all(&orb.to_le_bytes())?;
        }
        file.sync_all()?;
        Ok(())
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    if let Err(error) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(error.into());
    }
    sync_parent_dir(path)?;
    Ok(())
}

#[cfg(unix)]
fn sync_parent_dir(path: &Path) -> Result<(), GelError> {
    if let Some(parent) = path.parent() {
        let parent = if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        };
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn sync_parent_dir(_path: &Path) -> Result<(), GelError> {
    Ok(())
}

fn read_generation(path: &Path) -> Result<Generation, GelError> {
    let mut file = File::open(path)?;
    let mut bytes = [0u8; HEADER_BYTES];
    file.read_exact(&mut bytes)?;
    if bytes[0..8] == GEL_MAGIC_V2 {
        Ok(Header::decode(&bytes)?.generation)
    } else if bytes[0..8] == GEL_MAGIC_V1 {
        if le_u32(&bytes, 8) != 1 {
            return Err(GelError::UnsupportedVersion(le_u32(&bytes, 8)));
        }
        Ok(Generation(le_u64(&bytes, 32)))
    } else {
        Err(GelError::InvalidMagic)
    }
}

fn crc_orbs(orbs: &[Orb1024]) -> u64 {
    let mut crc = Crc64Ecma::new();
    for orb in orbs {
        crc.update(&orb.to_le_bytes());
    }
    crc.finish()
}

fn temp_path(path: &Path) -> PathBuf {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(
        ".tmp-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    PathBuf::from(tmp)
}

fn create_temp_file(path: &Path) -> Result<(PathBuf, File), GelError> {
    #[cfg(unix)]
    let existing_mode = match fs::metadata(path) {
        Ok(metadata) => Some(metadata.permissions().mode() & 0o7777),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };

    for _ in 0..128 {
        let tmp = temp_path(path);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        match options.open(&tmp) {
            Ok(file) => {
                #[cfg(unix)]
                if let Some(mode) = existing_mode {
                    if let Err(error) = file.set_permissions(fs::Permissions::from_mode(mode)) {
                        drop(file);
                        let _ = fs::remove_file(&tmp);
                        return Err(error.into());
                    }
                }
                return Ok((tmp, file));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(GelError::Io(
        "could not allocate a unique GEL temporary path".into(),
    ))
}

#[inline]
fn le_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("fixed header slice"),
    )
}

#[inline]
fn le_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("fixed header slice"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gel_core::{checksum64, splitmix64, GEL_MAGIC_V1, ORB_WORDS};

    fn orb(seed: u64) -> Orb1024 {
        let mut words = [0u64; ORB_WORDS];
        for (i, w) in words.iter_mut().enumerate() {
            *w = splitmix64(seed + i as u64);
        }
        Orb1024::from_words(words)
    }

    fn path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "gel-store-v2-{name}-{}-{}.gel",
            std::process::id(),
            splitmix64(9)
        ))
    }

    #[test]
    fn v2_header_is_exactly_64_bytes_and_self_protected() {
        let header = Header::new(3, Generation(7), 0x1234);
        assert_eq!(header.encode().len(), 64);
        assert_eq!(Header::decode(&header.encode()).unwrap(), header);
    }

    #[test]
    fn every_single_header_bit_flip_is_rejected() {
        let header = Header::new(3, Generation(0x1122_3344_5566_7788), 0x8877_6655_4433_2211);
        let original = header.encode();
        let mut rejected = 0usize;
        for bit in 0..HEADER_BYTES * 8 {
            let mut mutated = original;
            mutated[bit >> 3] ^= 1u8 << (bit & 7);
            if Header::decode(&mutated).is_err() {
                rejected += 1;
            }
        }
        assert_eq!(rejected, HEADER_BYTES * 8);
    }

    #[test]
    fn persistence_roundtrip_and_payload_corruption_rejection() {
        let path = path("roundtrip");
        let store = RamStore::from_orbs(vec![orb(1), orb(2), orb(3)], Generation(9));
        store.write_atomic(&path).unwrap();
        let reopened = RamStore::open_verified(&path).unwrap();
        assert_eq!(reopened.orbs(), store.orbs());
        let mut bytes = fs::read(&path).unwrap();
        bytes[HEADER_BYTES + 13] ^= 0x80;
        fs::write(&path, bytes).unwrap();
        assert!(matches!(
            RamStore::open_verified(&path),
            Err(GelError::CorruptStore)
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn generation_rollback_and_equal_generation_are_rejected() {
        let path = path("generation");
        RamStore::from_orbs(vec![orb(1)], Generation(10))
            .write_atomic(&path)
            .unwrap();
        assert!(matches!(
            RamStore::from_orbs(vec![orb(2)], Generation(9)).write_if_newer(&path),
            Err(GelError::GenerationRollback {
                current: 10,
                attempted: 9
            })
        ));
        assert!(matches!(
            RamStore::from_orbs(vec![orb(2)], Generation(10)).write_if_newer(&path),
            Err(GelError::GenerationRollback {
                current: 10,
                attempted: 10
            })
        ));
        RamStore::from_orbs(vec![orb(3)], Generation(11))
            .write_if_newer(&path)
            .unwrap();
        assert_eq!(
            RamStore::open_verified(&path).unwrap().header().generation,
            Generation(11)
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn configured_open_limits_fail_before_large_allocation() {
        let path = path("limits");
        RamStore::from_orbs(vec![orb(1), orb(2)], Generation(1))
            .write_atomic(&path)
            .unwrap();
        let limits = OpenLimits {
            max_records: 1,
            max_file_bytes: u64::MAX,
        };
        assert!(matches!(
            RamStore::open_verified_with_limits(&path, limits),
            Err(GelError::LimitExceeded(_))
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn malformed_lengths_and_trailing_bytes_are_rejected() {
        let base = path("invalid");
        RamStore::from_orbs(vec![orb(1)], Generation(11))
            .write_atomic(&base)
            .unwrap();
        let original = fs::read(&base).unwrap();
        for length in [0usize, HEADER_BYTES - 1, original.len() - 1] {
            let path = base.with_extension(format!("truncated-{length}.gel"));
            fs::write(&path, &original[..length]).unwrap();
            assert!(RamStore::open_verified(&path).is_err());
            let _ = fs::remove_file(path);
        }
        let trailing = base.with_extension("trailing.gel");
        let mut bytes = original;
        bytes.push(0);
        fs::write(&trailing, bytes).unwrap();
        assert!(RamStore::open_verified(&trailing).is_err());
        let _ = fs::remove_file(trailing);
        let _ = fs::remove_file(base);
    }

    #[test]
    fn legacy_v1_store_is_verified_and_migrates_to_v2_on_write() {
        let source = path("legacy-v1");
        let migrated = path("legacy-v2");
        let records = [orb(31), orb(32)];
        let mut payload = Vec::new();
        for record in &records {
            payload.extend_from_slice(&record.to_le_bytes());
        }
        let mut header = [0u8; HEADER_BYTES];
        header[0..8].copy_from_slice(&GEL_MAGIC_V1);
        header[8..12].copy_from_slice(&1u32.to_le_bytes());
        header[12..16].copy_from_slice(&(ORB_BITS as u32).to_le_bytes());
        header[16..20].copy_from_slice(&(ORB_BYTES as u32).to_le_bytes());
        header[24..32].copy_from_slice(&(records.len() as u64).to_le_bytes());
        header[32..40].copy_from_slice(&77u64.to_le_bytes());
        header[40..48].copy_from_slice(&checksum64(&payload).to_le_bytes());
        let mut file = File::create(&source).unwrap();
        file.write_all(&header).unwrap();
        file.write_all(&payload).unwrap();
        file.sync_all().unwrap();

        let reopened = RamStore::open_verified(&source).unwrap();
        assert_eq!(reopened.orbs(), records);
        assert_eq!(reopened.source_format(), StoreFormat::LegacyV1);
        assert_eq!(reopened.header().version, 2);
        assert_eq!(reopened.header().generation, Generation(77));
        reopened.write_atomic(&migrated).unwrap();
        let migrated_bytes = fs::read(&migrated).unwrap();
        assert_eq!(&migrated_bytes[..8], &GEL_MAGIC_V2);
        assert_eq!(RamStore::open_verified(&migrated).unwrap().orbs(), records);
        let _ = fs::remove_file(source);
        let _ = fs::remove_file(migrated);
    }

    fn legacy_v1_file_bytes(records: &[Orb1024], generation: u64) -> Vec<u8> {
        let mut payload = Vec::new();
        for record in records {
            payload.extend_from_slice(&record.to_le_bytes());
        }
        let mut bytes = vec![0u8; HEADER_BYTES];
        bytes[0..8].copy_from_slice(&GEL_MAGIC_V1);
        bytes[8..12].copy_from_slice(&1u32.to_le_bytes());
        bytes[12..16].copy_from_slice(&(ORB_BITS as u32).to_le_bytes());
        bytes[16..20].copy_from_slice(&(ORB_BYTES as u32).to_le_bytes());
        bytes[24..32].copy_from_slice(&(records.len() as u64).to_le_bytes());
        bytes[32..40].copy_from_slice(&generation.to_le_bytes());
        bytes[40..48].copy_from_slice(&checksum64(&payload).to_le_bytes());
        bytes.extend_from_slice(&payload);
        bytes
    }

    #[test]
    fn oversized_record_count_is_rejected_before_allocation() {
        // Both headers carry a valid CRC, so only the size check can reject
        // them. Had try_reserve_exact been reached under UNLIMITED limits the
        // error would be AllocationFailed; the variants below show that
        // validate_count_and_size runs first.
        let overflow = path("count-overflow");
        fs::write(&overflow, Header::new(u64::MAX, Generation(1), 0).encode()).unwrap();
        assert!(matches!(
            RamStore::open_verified(&overflow),
            Err(GelError::InvalidHeader("payload size overflow"))
        ));
        let _ = fs::remove_file(overflow);

        let huge = path("count-huge");
        fs::write(&huge, Header::new(1 << 40, Generation(1), 0).encode()).unwrap();
        match RamStore::open_verified(&huge) {
            Err(GelError::InvalidLength { expected, actual }) => {
                assert_eq!(actual, HEADER_BYTES);
                assert_eq!(
                    expected,
                    usize::try_from((1u64 << 47) + HEADER_BYTES as u64).unwrap_or(usize::MAX)
                );
            }
            other => panic!("expected InvalidLength, got {other:?}"),
        }
        let _ = fs::remove_file(huge);
    }

    #[test]
    fn legacy_v1_nonzero_reserved_bytes_are_rejected() {
        let base = path("legacy-reserved");
        let original = legacy_v1_file_bytes(&[orb(41), orb(42)], 5);
        fs::write(&base, &original).unwrap();
        assert!(RamStore::open_verified(&base).is_ok());
        for offset in (20..24).chain(48..64) {
            let path = base.with_extension(format!("{offset}.gel"));
            let mut bytes = original.clone();
            bytes[offset] = 1;
            fs::write(&path, bytes).unwrap();
            assert!(
                matches!(
                    RamStore::open_verified(&path),
                    Err(GelError::InvalidHeader(
                        "legacy reserved bytes must be zero"
                    ))
                ),
                "offset {offset}"
            );
            let _ = fs::remove_file(path);
        }
        let _ = fs::remove_file(base);
    }

    #[test]
    fn v2_nonzero_flags_are_rejected_before_crc() {
        let valid = Header::new(1, Generation(1), 0).encode();
        // Flags set without recomputing the CRC: the flags check must report
        // before the CRC check would report CorruptHeader.
        let mut stale_crc = valid;
        stale_crc[20] = 1;
        assert!(matches!(
            Header::decode(&stale_crc),
            Err(GelError::InvalidHeader("unsupported nonzero flags"))
        ));
        // Flags set with a CRC recomputed by encode(): still rejected.
        let flagged = Header {
            flags: 1,
            ..Header::new(1, Generation(1), 0)
        }
        .encode();
        assert!(matches!(
            Header::decode(&flagged),
            Err(GelError::InvalidHeader("unsupported nonzero flags"))
        ));
        let path = path("flags");
        let mut bytes = flagged.to_vec();
        bytes.extend_from_slice(&orb(1).to_le_bytes());
        fs::write(&path, bytes).unwrap();
        assert!(matches!(
            RamStore::open_verified(&path),
            Err(GelError::InvalidHeader("unsupported nonzero flags"))
        ));
        let _ = fs::remove_file(path);
    }

    // ---- streaming verification -------------------------------------------

    /// Bytes of a v2 store exactly as `write_atomic` would emit them.
    fn v2_file_bytes(records: &[Orb1024], generation: u64) -> Vec<u8> {
        let store = RamStore::from_orbs(records.to_vec(), Generation(generation));
        let mut bytes = store.header().encode().to_vec();
        for record in records {
            bytes.extend_from_slice(&record.to_le_bytes());
        }
        bytes
    }

    /// The same Ok/Err outcome from `verify_file` and `open_verified`, with
    /// equal headers on success and equal error text on failure.
    fn assert_same_verdict(path: &Path) -> Result<Verification, GelError> {
        let streamed = verify_file(path, OpenLimits::UNLIMITED);
        let loaded = RamStore::open_verified(path);
        match (&streamed, &loaded) {
            (Ok(verification), Ok(store)) => {
                assert_eq!(verification.header, store.header());
                assert_eq!(verification.source_format, store.source_format());
            }
            (Err(a), Err(b)) => assert_eq!(a.to_string(), b.to_string()),
            other => panic!("verify_file and open_verified disagree: {other:?}"),
        }
        streamed
    }

    #[test]
    fn verify_file_agrees_with_open_verified_and_rejects_corruption() {
        let path = path("verify-stream");
        let records = [orb(61), orb(62), orb(63)];
        let original = v2_file_bytes(&records, 44);
        fs::write(&path, &original).unwrap();
        let verification = assert_same_verdict(&path).unwrap();
        assert_eq!(verification.source_format, StoreFormat::V2);
        assert_eq!(verification.header.version, 2);
        assert_eq!(verification.header.record_count, 3);
        assert_eq!(verification.header.generation, Generation(44));

        let mut flipped = original.clone();
        flipped[HEADER_BYTES + 200] ^= 0x01;
        fs::write(&path, &flipped).unwrap();
        assert!(matches!(
            assert_same_verdict(&path),
            Err(GelError::CorruptStore)
        ));

        fs::write(&path, &original[..original.len() - 1]).unwrap();
        assert!(matches!(
            assert_same_verdict(&path),
            Err(GelError::InvalidLength { .. })
        ));

        fs::write(&path, &original).unwrap();
        let limits = OpenLimits {
            max_records: 2,
            max_file_bytes: u64::MAX,
        };
        assert!(matches!(
            verify_file(&path, limits),
            Err(GelError::LimitExceeded("record count"))
        ));
        let limits = OpenLimits {
            max_records: u64::MAX,
            max_file_bytes: original.len() as u64 - 1,
        };
        assert!(matches!(
            verify_file(&path, limits),
            Err(GelError::LimitExceeded("file bytes"))
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn verify_file_reports_legacy_v1_while_providing_upgrade_header() {
        let path = path("verify-legacy");
        let records = [orb(71), orb(72)];
        let original = legacy_v1_file_bytes(&records, 77);
        fs::write(&path, &original).unwrap();
        let verification = assert_same_verdict(&path).unwrap();
        assert_eq!(verification.source_format, StoreFormat::LegacyV1);
        assert_eq!(verification.source_format.version(), 1);
        assert_eq!(verification.header.version, 2);
        assert_eq!(verification.header.record_count, 2);
        assert_eq!(verification.header.generation, Generation(77));
        assert_eq!(
            verification.header.payload_crc64,
            crc64_ecma(&original[HEADER_BYTES..])
        );
        assert_eq!(
            Header::decode(&verification.header.encode()).unwrap(),
            verification.header
        );

        let mut flipped = original.clone();
        flipped[HEADER_BYTES + 5] ^= 0x10;
        fs::write(&path, &flipped).unwrap();
        assert!(matches!(
            assert_same_verdict(&path),
            Err(GelError::CorruptStore)
        ));
        let _ = fs::remove_file(path);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_is_private_by_default_and_preserves_existing_mode() {
        let fresh = path("private-mode-new");
        RamStore::from_orbs(vec![orb(1)], Generation(1))
            .write_atomic(&fresh)
            .unwrap();
        assert_eq!(
            fs::metadata(&fresh).unwrap().permissions().mode() & 0o777,
            0o600
        );

        fs::set_permissions(&fresh, fs::Permissions::from_mode(0o640)).unwrap();
        RamStore::from_orbs(vec![orb(2)], Generation(2))
            .write_if_newer(&fresh)
            .unwrap();
        assert_eq!(
            fs::metadata(&fresh).unwrap().permissions().mode() & 0o777,
            0o640
        );
        assert_eq!(
            RamStore::open_verified(&fresh).unwrap().header().generation,
            Generation(2)
        );
        let _ = fs::remove_file(fresh);
    }

    // ---- deterministic mutation sweeps -------------------------------------

    fn pseudo_random_header(seed: u64) -> [u8; HEADER_BYTES] {
        let mut out = [0u8; HEADER_BYTES];
        for (i, chunk) in out.chunks_exact_mut(8).enumerate() {
            chunk.copy_from_slice(&splitmix64(seed * 8 + i as u64).to_le_bytes());
        }
        out
    }

    #[test]
    fn random_headers_are_rejected_and_never_panic() {
        const N: u64 = 4096;
        let mut rejected_random = 0usize;
        let mut rejected_prefixed = 0usize;
        let mut rejected_crc = 0usize;
        for seed in 0..N {
            // Uniformly pseudo-random bytes.
            assert!(Header::decode(&pseudo_random_header(seed)).is_err());
            rejected_random += 1;

            // Valid magic and version, pseudo-random remainder.
            let mut prefixed = pseudo_random_header(N + seed);
            prefixed[0..8].copy_from_slice(&GEL_MAGIC_V2);
            prefixed[8..12].copy_from_slice(&GEL_FORMAT_VERSION.to_le_bytes());
            assert!(Header::decode(&prefixed).is_err());
            rejected_prefixed += 1;

            // Fully valid header whose stored CRC is replaced by a
            // pseudo-random value: only the CRC comparison can reject it.
            let random = pseudo_random_header(2 * N + seed);
            let valid = Header::new(
                le_u64(&random, 0),
                Generation(le_u64(&random, 8)),
                le_u64(&random, 16),
            )
            .encode();
            let mut bad_crc = valid;
            bad_crc[V2_HEADER_CRC_OFFSET..V2_HEADER_CRC_OFFSET + 8]
                .copy_from_slice(&random[24..32]);
            assert!(matches!(
                Header::decode(&bad_crc),
                Err(GelError::CorruptHeader)
            ));
            rejected_crc += 1;
        }
        assert_eq!(rejected_random, 4096);
        assert_eq!(rejected_prefixed, 4096);
        assert_eq!(rejected_crc, 4096);
    }

    #[test]
    fn every_double_header_bit_flip_is_rejected() {
        let header = Header::new(3, Generation(0x1122_3344_5566_7788), 0x8877_6655_4433_2211);
        let original = header.encode();
        let bits = HEADER_BYTES * 8;
        let mut rejected = 0usize;
        let mut total = 0usize;
        for first in 0..bits {
            for second in first + 1..bits {
                let mut mutated = original;
                mutated[first >> 3] ^= 1u8 << (first & 7);
                mutated[second >> 3] ^= 1u8 << (second & 7);
                total += 1;
                if Header::decode(&mutated).is_err() {
                    rejected += 1;
                }
            }
        }
        assert_eq!(total, 130_816);
        assert_eq!(rejected, 130_816);
    }

    #[test]
    fn legacy_v1_header_single_bit_sweep_freezes_detection_boundary() {
        const GENERATION: u64 = 0x0123_4567_89ab_cdef;
        let path = path("legacy-bit-sweep");
        let records = [orb(81), orb(82)];
        let original = legacy_v1_file_bytes(&records, GENERATION);
        let mut detected = 0usize;
        let mut undetected = 0usize;
        let mut length_mismatch = 0usize;
        let mut size_overflow = 0usize;
        for bit in 0..HEADER_BYTES * 8 {
            let byte = bit >> 3;
            let mut bytes = original.clone();
            bytes[byte] ^= 1u8 << (bit & 7);
            fs::write(&path, &bytes).unwrap();
            let result = assert_same_verdict(&path);
            match byte {
                // magic, version, orb_bits, record_bytes, reserved 20..24
                0..=23 => {
                    assert!(result.is_err(), "bit {bit}");
                    detected += 1;
                }
                // record_count: the file length no longer matches, or the
                // payload size overflows for the seven highest bits.
                24..=31 => {
                    match result {
                        Err(GelError::InvalidLength { .. }) => length_mismatch += 1,
                        Err(GelError::InvalidHeader("payload size overflow")) => size_overflow += 1,
                        other => panic!("bit {bit}: {other:?}"),
                    }
                    detected += 1;
                }
                // generation: v1 has no header authentication, so the store
                // opens with the mutated generation. Documented negative.
                32..=39 => {
                    let verification = result.unwrap_or_else(|e| panic!("bit {bit}: {e}"));
                    assert_eq!(verification.source_format, StoreFormat::LegacyV1);
                    assert_eq!(
                        verification.header.generation.0,
                        GENERATION ^ (1u64 << (bit - 256))
                    );
                    assert_eq!(verification.header.record_count, 2);
                    undetected += 1;
                }
                // legacy checksum
                40..=47 => {
                    assert!(matches!(result, Err(GelError::CorruptStore)), "bit {bit}");
                    detected += 1;
                }
                // reserved 48..64
                _ => {
                    assert!(
                        matches!(
                            result,
                            Err(GelError::InvalidHeader(
                                "legacy reserved bytes must be zero"
                            ))
                        ),
                        "bit {bit}"
                    );
                    detected += 1;
                }
            }
        }
        let _ = fs::remove_file(path);
        assert_eq!(detected + undetected, 512);
        assert_eq!(detected, 448);
        assert_eq!(undetected, 64);
        assert_eq!(length_mismatch, 57);
        assert_eq!(size_overflow, 7);
    }

    #[test]
    fn every_payload_byte_flip_is_rejected() {
        let path = path("payload-byte-sweep");
        let original = v2_file_bytes(&[orb(91), orb(92), orb(93)], 3);
        let payload_len = original.len() - HEADER_BYTES;
        assert_eq!(payload_len, 384);
        let mut inverted = 0usize;
        let mut single_bit = 0usize;
        for index in 0..payload_len {
            let mut bytes = original.clone();
            bytes[HEADER_BYTES + index] ^= 0xFF;
            fs::write(&path, &bytes).unwrap();
            assert!(
                matches!(assert_same_verdict(&path), Err(GelError::CorruptStore)),
                "inverted byte {index}"
            );
            inverted += 1;

            let mut bytes = original.clone();
            bytes[HEADER_BYTES + index] ^= 1u8 << (index & 7);
            fs::write(&path, &bytes).unwrap();
            assert!(
                matches!(assert_same_verdict(&path), Err(GelError::CorruptStore)),
                "bit {} of byte {index}",
                index & 7
            );
            single_bit += 1;
        }
        let _ = fs::remove_file(path);
        assert_eq!(inverted, 384);
        assert_eq!(single_bit, 384);
    }

    #[test]
    fn every_truncation_length_is_rejected() {
        let path = path("truncation-sweep");
        let original = v2_file_bytes(&[orb(101)], 5);
        assert_eq!(original.len(), 192);
        let mut short_header = 0usize;
        let mut length_mismatch = 0usize;
        for length in 0..original.len() {
            fs::write(&path, &original[..length]).unwrap();
            match assert_same_verdict(&path) {
                Err(GelError::Io(_)) if length < HEADER_BYTES => short_header += 1,
                Err(GelError::InvalidLength { expected, actual }) if length >= HEADER_BYTES => {
                    assert_eq!(expected, 192);
                    assert_eq!(actual, length);
                    length_mismatch += 1;
                }
                other => panic!("length {length}: {other:?}"),
            }
        }
        let _ = fs::remove_file(path);
        assert_eq!(short_header, 64);
        assert_eq!(length_mismatch, 128);
        assert_eq!(short_header + length_mismatch, 192);
    }
}
