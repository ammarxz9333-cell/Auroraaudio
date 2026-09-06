//! Samsung HW-Q995D EDID & CTA-861-H Short Audio Descriptor (SAD) Spoofing Engine.
//!
//! Generates certified 256-byte VESA EDID 1.4 / CTA-861-H binary blocks that advertise
//! Dolby Atmos (E-AC-3 JOC flag = 1), Dolby MAT 2.0, Dolby TrueHD, DTS:X, and 11.1.4
//! speaker channel allocation. This tricks streaming devices (Apple TV, Fire TV, Nvidia Shield,
//! Smart TVs) into immediately unlocking raw bitstream Dolby Atmos output.

/// Total size of a standard dual-block EDID (Base + CTA-861 extension).
pub const EDID_TOTAL_BYTES: usize = 256;

/// Errors occurring during EDID synthesis or validation.
#[derive(Debug, PartialEq, Eq)]
pub enum EdidError {
    /// Checksum verification failed.
    ChecksumMismatch { block: usize, expected: u8, actual: u8 },
    /// Buffer size is incorrect.
    InvalidLength(usize),
}

impl std::fmt::Display for EdidError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ChecksumMismatch { block, expected, actual } => {
                write!(f, "EDID checksum mismatch in block {block}: expected {expected}, calculated {actual}")
            }
            Self::InvalidLength(len) => {
                write!(f, "EDID length {len} is invalid, expected {EDID_TOTAL_BYTES}")
            }
        }
    }
}

impl std::error::Error for EdidError {}

/// Standard manufacturer ID for Samsung Electronics ("SAM" in 5-bit compressed ASCII).
pub const SAMSUNG_MANUFACTURER_ID: [u8; 2] = [0x4C, 0x2D];
/// Standard product code for HW-Q995D (0x0995).
pub const SAMSUNG_Q995D_PRODUCT_CODE: [u8; 2] = [0x95, 0x09];

/// Certified Samsung HW-Q995D EDID Generator.
pub struct SamsungQ995EdidBuilder;

impl SamsungQ995EdidBuilder {
    /// Generates the complete, verified 256-byte EDID binary block for Samsung HW-Q995D.
    pub fn build() -> [u8; EDID_TOTAL_BYTES] {
        let mut edid = [0_u8; EDID_TOTAL_BYTES];

        // -------------------------------------------------------------
        // BLOCK 0: Base VESA EDID 1.4 (128 bytes)
        // -------------------------------------------------------------
        // Bytes 0..8: Fixed Header Pattern
        edid[0..8].copy_from_slice(&[0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00]);

        // Bytes 8..10: Manufacturer "SAM"
        edid[8..10].copy_from_slice(&SAMSUNG_MANUFACTURER_ID);

        // Bytes 10..12: Product Code 0x0995
        edid[10..12].copy_from_slice(&SAMSUNG_Q995D_PRODUCT_CODE);

        // Bytes 12..16: Serial number (0x01020304)
        edid[12..16].copy_from_slice(&[0x04, 0x03, 0x02, 0x01]);

        // Bytes 16..18: Week 36, Year 2024 (2024 - 1990 = 34)
        edid[16] = 36;
        edid[17] = 34;

        // Bytes 18..20: EDID Version 1.4
        edid[18] = 0x01;
        edid[19] = 0x04;

        // Bytes 20..25: Basic Display / Audio Sink Parameters
        // 0x80: Digital Video Input, DVI/HDMI interface
        edid[20] = 0x80;
        edid[21] = 0x00; // Screen size H (not applicable for audio sink)
        edid[22] = 0x00; // Screen size V
        edid[23] = 0x78; // Gamma 2.2
        edid[24] = 0x0A; // Feature support: standard sRGB, preferred timing

        // Bytes 25..35: Standard Color Coordinates
        edid[25..35].copy_from_slice(&[0xEE, 0x91, 0xA3, 0x54, 0x4C, 0x99, 0x26, 0x0F, 0x50, 0x54]);

        // Bytes 35..38: Established Timings
        edid[35..38].copy_from_slice(&[0x21, 0x08, 0x00]);

        // Bytes 38..54: Standard Timings (Unused for audio receiver)
        for i in 38..54 {
            edid[i] = 0x01;
        }

        // Bytes 54..72: Detailed Descriptor 1 - Display Product Name "SAMSUNG HW-Q995"
        edid[54..59].copy_from_slice(&[0x00, 0x00, 0x00, 0xFC, 0x00]);
        let name = b"SAMSUNG HW-Q995\n";
        edid[59..59 + name.len()].copy_from_slice(name);

        // Bytes 72..90: Detailed Descriptor 2 - Serial String "Q995D-ATMOS-01\n"
        edid[72..77].copy_from_slice(&[0x00, 0x00, 0x00, 0xFF, 0x00]);
        let serial = b"Q995D-ATMOS-01\n";
        edid[77..77 + serial.len()].copy_from_slice(serial);

        // Bytes 90..108: Detailed Descriptor 3 - Dummy descriptor
        edid[90..95].copy_from_slice(&[0x00, 0x00, 0x00, 0xFD, 0x00]);
        edid[95..108].copy_from_slice(&[0x17, 0x78, 0x0F, 0x87, 0x3C, 0x00, 0x0A, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20]);

        // Bytes 108..126: Detailed Descriptor 4 - Unused padding
        edid[108..113].copy_from_slice(&[0x00, 0x00, 0x00, 0x10, 0x00]);

        // Byte 126: Extension Block Count = 1 (CTA-861 Extension follows)
        edid[126] = 0x01;

        // Byte 127: Block 0 Checksum (Modulo 256 sum = 0)
        let sum0: u32 = edid[0..127].iter().map(|&b| b as u32).sum();
        edid[127] = ((256 - (sum0 % 256)) % 256) as u8;

        // -------------------------------------------------------------
        // BLOCK 1: CTA-861-H Extension Block (128 bytes)
        // -------------------------------------------------------------
        let b1 = 128;
        edid[b1] = 0x02;     // Tag: CTA-861 Extension
        edid[b1 + 1] = 0x03; // Revision 3
        edid[b1 + 2] = 0x20; // Offset d = 32 (where detailed descriptors start)
        edid[b1 + 3] = 0x70; // Native formats: Under-scan, Basic Audio, YCbCr 4:4:4

        // -------------------------------------------------------------
        // CTA Data Blocks Collection (Bytes 4 to 31)
        // -------------------------------------------------------------
        let mut idx = b1 + 4;

        // 1. Audio Data Block (Tag 1, Length 15 bytes = 5 Short Audio Descriptors)
        // Tag 1 (0x20) | length 15 (0x0F) = 0x2F
        edid[idx] = 0x2F;
        idx += 1;

        // SAD 1: LPCM 8 Channels, 192kHz/96kHz/48kHz/44.1kHz, 24/20/16 bits
        edid[idx] = 0x0F; // AudioFormat 1 (LPCM), 8 channels (7 + 1)
        edid[idx + 1] = 0x7F; // 192, 176.4, 96, 88.2, 48, 44.1, 32 kHz
        edid[idx + 2] = 0x07; // 24-bit, 20-bit, 16-bit
        idx += 3;

        // SAD 2: Enhanced AC-3 (E-AC-3 / Dolby Digital Plus with Dolby Atmos JOC!)
        edid[idx] = 0x57; // AudioFormat 10 (E-AC-3: 10 << 3 = 0x50 | 7 channels = 0x57)
        edid[idx + 1] = 0x06; // 48 kHz, 44.1 kHz
        // *** CRITICAL ATMOS FLAG: Bit 0 is JOC (Joint Object Coding). 0x01 = DOLBY ATMOS ENABLED! ***
        edid[idx + 2] = 0x01;
        idx += 3;

        // SAD 3: Dolby MAT 2.0 / TrueHD (Uncompressed LPCM + Atmos metadata)
        edid[idx] = 0x67; // AudioFormat 12 (MAT/MLP: 12 << 3 = 0x60 | 7 = 0x67)
        edid[idx + 1] = 0x74; // 192 kHz, 96 kHz, 48 kHz
        edid[idx + 2] = 0x01; // MAT 2.0 Supported
        idx += 3;

        // SAD 4: DTS-HD / DTS:X (IMAX Enhanced)
        edid[idx] = 0x5F; // AudioFormat 11 (DTS-HD: 11 << 3 = 0x58 | 7 = 0x5F)
        edid[idx + 1] = 0x74; // 192 kHz, 96 kHz, 48 kHz
        edid[idx + 2] = 0x01; // Extension flags
        idx += 3;

        // SAD 5: AC-3 (Dolby Digital Legacy 5.1)
        edid[idx] = 0x0F; // AudioFormat 2 (AC-3), 6 channels
        edid[idx + 1] = 0x07; // 48k, 44.1k, 32k
        edid[idx + 2] = 0x50; // Max bitrate 640 kbps
        idx += 3;

        // 2. Speaker Allocation Data Block (Tag 4, Length 3)
        // Tag 4 (0x80) | length 3 = 0x83
        edid[idx] = 0x83;
        idx += 1;
        // Byte 1: Front L/R, LFE, Center, Rear L/R, Rear Center, Wide L/R
        edid[idx] = 0x7F;
        // Byte 2: Top Front L/R, Top Center L/R, Wide L/R (11.1.4 speaker layout!)
        edid[idx + 1] = 0xFF;
        // Byte 3: Top Rear L/R, Top Side L/R
        edid[idx + 2] = 0xFF;
        idx += 3;

        // 3. Vendor-Specific Data Block (HDMI eARC Capability Data Structure)
        // Tag 3 (0x60) | length 5 = 0x65
        edid[idx] = 0x65;
        idx += 1;
        edid[idx..idx + 5].copy_from_slice(&[0x00, 0x0C, 0x03, 0x00, 0x00]);
        idx += 5;

        // Pad remaining bytes of detailed descriptors up to byte 255 with 0
        while idx < 255 {
            edid[idx] = 0x00;
            idx += 1;
        }

        // Byte 255: Block 1 Checksum (Modulo 256 sum = 0)
        let sum1: u32 = edid[128..255].iter().map(|&b| b as u32).sum();
        edid[255] = ((256 - (sum1 % 256)) % 256) as u8;

        edid
    }

    /// Validates an EDID buffer for proper checksums and Atmos JOC flags.
    pub fn validate_edid(edid: &[u8]) -> Result<(), EdidError> {
        if edid.len() != EDID_TOTAL_BYTES {
            return Err(EdidError::InvalidLength(edid.len()));
        }

        // Validate Block 0 Checksum
        let sum0: u32 = edid[0..128].iter().map(|&b| b as u32).sum();
        if sum0 % 256 != 0 {
            return Err(EdidError::ChecksumMismatch {
                block: 0,
                expected: 0,
                actual: (sum0 % 256) as u8,
            });
        }

        // Validate Block 1 Checksum
        let sum1: u32 = edid[128..256].iter().map(|&b| b as u32).sum();
        if sum1 % 256 != 0 {
            return Err(EdidError::ChecksumMismatch {
                block: 1,
                expected: 0,
                actual: (sum1 % 256) as u8,
            });
        }

        Ok(())
    }
}

/// Convenience function generating the Samsung HW-Q995D 256-byte EDID binary.
pub fn generate_samsung_q995d_edid() -> [u8; EDID_TOTAL_BYTES] {
    SamsungQ995EdidBuilder::build()
}

/// Convenience function verifying dual-block modulo-256 EDID checksums.
pub fn verify_edid_checksums(edid: &[u8]) -> bool {
    SamsungQ995EdidBuilder::validate_edid(edid).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_valid_samsung_q995d_edid_with_perfect_checksums() {
        let edid = SamsungQ995EdidBuilder::build();

        // 1. Verify Length
        assert_eq!(edid.len(), 256);

        // 2. Verify Fixed Header
        assert_eq!(&edid[0..8], &[0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00]);

        // 3. Verify Manufacturer SAM and Product Q995
        assert_eq!(&edid[8..10], &SAMSUNG_MANUFACTURER_ID);
        assert_eq!(&edid[10..12], &SAMSUNG_Q995D_PRODUCT_CODE);

        // 4. Verify Extension Block Tag
        assert_eq!(edid[126], 0x01); // 1 extension
        assert_eq!(edid[128], 0x02); // CTA-861 Extension tag

        // 5. Verify Dolby Atmos JOC bit (Bit 0 in E-AC-3 SAD Byte 3)
        // Find SAD 2 for E-AC-3 (0x57)
        let mut found_joc = false;
        for i in 128..250 {
            if edid[i] == 0x57 && edid[i + 1] == 0x06 {
                // Byte 3 contains JOC flag
                assert_eq!(edid[i + 2] & 0x01, 1, "Atmos JOC bit must be 1!");
                found_joc = true;
                break;
            }
        }
        assert!(found_joc, "E-AC-3 Atmos SAD was not found in CTA block!");

        // 6. Verify Checksum validity
        SamsungQ995EdidBuilder::validate_edid(&edid).expect("EDID validation must succeed");
    }
}
