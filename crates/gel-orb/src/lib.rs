#![forbid(unsafe_code)]

//! Fixed-size binary ORB carrier: 1024 bits / 128 bytes.

use gel_core::{GelError, ORB_BITS, ORB_BYTES, ORB_WORDS};

#[repr(C, align(64))]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct Orb1024 {
    words: [u64; ORB_WORDS],
}

impl Default for Orb1024 {
    fn default() -> Self {
        Self::ZERO
    }
}

impl Orb1024 {
    pub const ZERO: Self = Self {
        words: [0; ORB_WORDS],
    };

    #[inline]
    pub const fn from_words(words: [u64; ORB_WORDS]) -> Self {
        Self { words }
    }

    #[inline]
    pub const fn words(&self) -> &[u64; ORB_WORDS] {
        &self.words
    }

    #[inline]
    pub fn words_mut(&mut self) -> &mut [u64; ORB_WORDS] {
        &mut self.words
    }

    pub fn from_le_bytes(bytes: &[u8]) -> Result<Self, GelError> {
        if bytes.len() != ORB_BYTES {
            return Err(GelError::InvalidLength {
                expected: ORB_BYTES,
                actual: bytes.len(),
            });
        }
        let mut words = [0u64; ORB_WORDS];
        for (i, chunk) in bytes.chunks_exact(8).enumerate() {
            let mut tmp = [0u8; 8];
            tmp.copy_from_slice(chunk);
            words[i] = u64::from_le_bytes(tmp);
        }
        Ok(Self { words })
    }

    pub fn to_le_bytes(self) -> [u8; ORB_BYTES] {
        let mut out = [0u8; ORB_BYTES];
        for (i, word) in self.words.iter().enumerate() {
            out[i * 8..i * 8 + 8].copy_from_slice(&word.to_le_bytes());
        }
        out
    }

    #[inline]
    pub fn bit(&self, index: usize) -> Result<bool, GelError> {
        if index >= ORB_BITS {
            return Err(GelError::InvalidView("bit index outside 0..1023"));
        }
        Ok(((self.words[index >> 6] >> (index & 63)) & 1) != 0)
    }

    #[inline]
    pub fn set_bit(&mut self, index: usize, value: bool) -> Result<(), GelError> {
        if index >= ORB_BITS {
            return Err(GelError::InvalidView("bit index outside 0..1023"));
        }
        let mask = 1u64 << (index & 63);
        let word = &mut self.words[index >> 6];
        if value {
            *word |= mask;
        } else {
            *word &= !mask;
        }
        Ok(())
    }

    #[inline]
    pub fn hamming_distance(&self, other: &Self) -> u16 {
        self.words
            .iter()
            .zip(other.words.iter())
            .map(|(&a, &b)| (a ^ b).count_ones() as u16)
            .sum()
    }

    #[inline]
    pub fn xnor_matches(&self, other: &Self) -> u16 {
        ORB_BITS as u16 - self.hamming_distance(other)
    }

    #[inline]
    pub fn similarity(&self, other: &Self) -> f32 {
        self.xnor_matches(other) as f32 / ORB_BITS as f32
    }
}

const _: () = assert!(core::mem::size_of::<Orb1024>() == ORB_BYTES);
const _: () = assert!(core::mem::align_of::<Orb1024>() == 64);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_byte_roundtrip() {
        let mut bytes = [0u8; ORB_BYTES];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(37);
        }
        let orb = Orb1024::from_le_bytes(&bytes).unwrap();
        assert_eq!(orb.to_le_bytes(), bytes);
    }

    #[test]
    fn similarity_endpoints() {
        let zero = Orb1024::ZERO;
        let ones = Orb1024::from_words([u64::MAX; ORB_WORDS]);
        assert_eq!(zero.similarity(&zero), 1.0);
        assert_eq!(zero.similarity(&ones), 0.0);
    }

    #[test]
    fn bit_access_is_checked_and_reversible() {
        let mut orb = Orb1024::ZERO;
        for index in [0, 63, 64, 1023] {
            assert!(!orb.bit(index).unwrap());
            orb.set_bit(index, true).unwrap();
            assert!(orb.bit(index).unwrap());
            orb.set_bit(index, false).unwrap();
        }
        assert!(orb.bit(1024).is_err());
        assert!(orb.set_bit(1024, true).is_err());
        assert_eq!(orb, Orb1024::ZERO);
    }

    #[test]
    fn wrong_byte_lengths_are_rejected() {
        assert!(Orb1024::from_le_bytes(&[0; ORB_BYTES - 1]).is_err());
        assert!(Orb1024::from_le_bytes(&[0; ORB_BYTES + 1]).is_err());
    }
}
