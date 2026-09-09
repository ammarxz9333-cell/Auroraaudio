use std::error::Error;
use std::fmt;

/// Selects the 16-bit IEC61937 word inside each captured S32_LE slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarrierWordHalf {
    /// Reference SiI9437/Vibesbox path: useful word is in bits 31..16.
    High,
    /// Alternate packing retained for bring-up and receiver evaluation.
    Low,
}

/// Errors returned while normalizing captured S32_LE carrier slots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CarrierNormalizeError {
    /// At least one serial-audio slot is required per carrier frame.
    ZeroSlots,
    /// Slot count overflowed the byte-size calculation.
    FrameSizeOverflow,
    /// End of stream left an incomplete serial-audio frame.
    TrailingBytes {
        pending: usize,
        frame_bytes: usize,
    },
}

impl fmt::Display for CarrierNormalizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroSlots => write!(f, "carrier slot count must be greater than zero"),
            Self::FrameSizeOverflow => write!(f, "carrier frame byte count overflow"),
            Self::TrailingBytes {
                pending,
                frame_bytes,
            } => write!(
                f,
                "capture ended with {pending} trailing bytes; expected complete {frame_bytes}-byte carrier frames"
            ),
        }
    }
}

impl Error for CarrierNormalizeError {}

/// Stateful S32_LE serial-audio carrier normalizer.
///
/// Linux ALSA capture boundaries do not have to align to I2S/SAI frames. This
/// object buffers only the incomplete tail, emits complete carrier frames as
/// canonical S16_LE IEC61937 words, and never pads missing bytes.
#[derive(Debug)]
pub struct S32LeCarrierNormalizer {
    slots: usize,
    frame_bytes: usize,
    word_half: CarrierWordHalf,
    pending: Vec<u8>,
    carrier_frames: u64,
    output_words: u64,
}

impl S32LeCarrierNormalizer {
    /// Creates a normalizer for a fixed recovered serial-audio layout.
    pub fn new(slots: usize, word_half: CarrierWordHalf) -> Result<Self, CarrierNormalizeError> {
        if slots == 0 {
            return Err(CarrierNormalizeError::ZeroSlots);
        }
        let frame_bytes = slots
            .checked_mul(4)
            .ok_or(CarrierNormalizeError::FrameSizeOverflow)?;
        Ok(Self {
            slots,
            frame_bytes,
            word_half,
            pending: Vec::new(),
            carrier_frames: 0,
            output_words: 0,
        })
    }

    /// Adds arbitrary S32_LE capture bytes and returns every complete normalized
    /// S16_LE carrier word available after this call.
    pub fn push(&mut self, input: &[u8]) -> Vec<u8> {
        self.pending.extend_from_slice(input);
        let complete_bytes = self.pending.len() / self.frame_bytes * self.frame_bytes;
        if complete_bytes == 0 {
            return Vec::new();
        }

        let frames = complete_bytes / self.frame_bytes;
        let mut output = Vec::with_capacity(complete_bytes / 2);
        for raw in self.pending[..complete_bytes].chunks_exact(4) {
            let slot = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
            let word = match self.word_half {
                CarrierWordHalf::High => (slot >> 16) as u16,
                CarrierWordHalf::Low => slot as u16,
            };
            output.extend_from_slice(&word.to_le_bytes());
        }

        self.pending.drain(..complete_bytes);
        self.carrier_frames = self.carrier_frames.saturating_add(frames as u64);
        self.output_words = self
            .output_words
            .saturating_add((frames.saturating_mul(self.slots)) as u64);
        output
    }

    /// Validates that capture ended on a complete serial-audio frame.
    pub fn finish(&self) -> Result<(), CarrierNormalizeError> {
        if self.pending.is_empty() {
            Ok(())
        } else {
            Err(CarrierNormalizeError::TrailingBytes {
                pending: self.pending.len(),
                frame_bytes: self.frame_bytes,
            })
        }
    }

    /// Drops only the incomplete capture tail after a real xrun/unlock/restart.
    pub fn reset(&mut self) {
        self.pending.clear();
    }

    pub fn pending_bytes(&self) -> usize {
        self.pending.len()
    }

    pub fn carrier_frames(&self) -> u64 {
        self.carrier_frames
    }

    pub fn output_words(&self) -> u64 {
        self.output_words
    }

    pub fn frame_bytes(&self) -> usize {
        self.frame_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn high_slot(word: u16) -> [u8; 4] {
        (u32::from(word) << 16).to_le_bytes()
    }

    #[test]
    fn reference_high_half_preserves_sync_words() {
        let mut normalizer = S32LeCarrierNormalizer::new(2, CarrierWordHalf::High).unwrap();
        let mut input = Vec::new();
        input.extend_from_slice(&high_slot(0xF872));
        input.extend_from_slice(&high_slot(0x4E1F));

        assert_eq!(normalizer.push(&input), [0x72, 0xF8, 0x1F, 0x4E]);
        normalizer.finish().unwrap();
        assert_eq!(normalizer.carrier_frames(), 1);
        assert_eq!(normalizer.output_words(), 2);
    }

    #[test]
    fn arbitrary_byte_boundaries_preserve_words() {
        let words = [0xF872_u16, 0x4E1F, 0x0015, 0x0080];
        let mut input = Vec::new();
        for word in words {
            input.extend_from_slice(&high_slot(word));
        }
        let mut normalizer = S32LeCarrierNormalizer::new(2, CarrierWordHalf::High).unwrap();
        let mut output = Vec::new();

        for chunk in input.chunks(5) {
            output.extend(normalizer.push(chunk));
        }

        normalizer.finish().unwrap();
        assert_eq!(
            output,
            [0x72, 0xF8, 0x1F, 0x4E, 0x15, 0x00, 0x80, 0x00]
        );
        assert_eq!(normalizer.carrier_frames(), 2);
    }

    #[test]
    fn low_half_is_explicit_and_bit_exact() {
        let mut normalizer = S32LeCarrierNormalizer::new(2, CarrierWordHalf::Low).unwrap();
        let mut input = Vec::new();
        input.extend_from_slice(&0x1234_ABCD_u32.to_le_bytes());
        input.extend_from_slice(&0x5678_EF01_u32.to_le_bytes());

        assert_eq!(normalizer.push(&input), [0xCD, 0xAB, 0x01, 0xEF]);
        normalizer.finish().unwrap();
    }

    #[test]
    fn incomplete_frame_is_never_padded() {
        let mut normalizer = S32LeCarrierNormalizer::new(2, CarrierWordHalf::High).unwrap();
        assert!(normalizer.push(&[0_u8; 7]).is_empty());
        assert_eq!(normalizer.pending_bytes(), 7);
        assert_eq!(
            normalizer.finish(),
            Err(CarrierNormalizeError::TrailingBytes {
                pending: 7,
                frame_bytes: 8,
            })
        );
    }

    #[test]
    fn reset_discards_partial_frame_after_discontinuity() {
        let mut normalizer = S32LeCarrierNormalizer::new(2, CarrierWordHalf::High).unwrap();
        assert!(normalizer.push(&[1, 2, 3]).is_empty());
        normalizer.reset();
        assert_eq!(normalizer.pending_bytes(), 0);
        normalizer.finish().unwrap();
    }
}
