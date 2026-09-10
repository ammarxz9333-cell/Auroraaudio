//! Byte-level audio codec and transport identification.
//!
//! The sniffer is intentionally container-independent: Aurora often receives
//! an IEC 61937 carrier from eARC rather than a file with a useful extension.
//! Probes are conservative and return `Unknown` instead of guessing when the
//! available prefix cannot distinguish codecs safely.

/// Encapsulation surrounding the compressed audio payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encapsulation {
    /// Raw codec elementary stream.
    Elementary,
    /// IEC 61937 burst carrier (typically captured from S/PDIF/eARC I2S).
    Iec61937,
    /// RIFF/RF64/WAVE family.
    Wave,
    /// AIFF/AIFC family.
    Aiff,
    /// Core Audio Format.
    Caf,
    /// Ogg container.
    Ogg,
    /// Unknown or not enough bytes to decide.
    Unknown,
}

/// Codec families Aurora can route to an open decoder backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CodecKind {
    Pcm,
    Ac3,
    Eac3,
    Eac3Joc,
    TrueHd,
    Mlp,
    DolbyMat,
    Dts,
    DtsHd,
    AacAdts,
    AacLatm,
    Flac,
    Opus,
    Vorbis,
    Speex,
    Mp3,
    Alac,
    WavPack,
    MonkeyAudio,
    Tta,
    Musepack,
    AmrNb,
    AmrWb,
    Sbc,
    OggUnknown,
    Unknown,
}

impl CodecKind {
    /// Stable lowercase identifier used by logs/configuration.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pcm => "pcm",
            Self::Ac3 => "ac3",
            Self::Eac3 => "eac3",
            Self::Eac3Joc => "eac3-joc",
            Self::TrueHd => "truehd",
            Self::Mlp => "mlp",
            Self::DolbyMat => "dolby-mat",
            Self::Dts => "dts",
            Self::DtsHd => "dtshd",
            Self::AacAdts => "aac-adts",
            Self::AacLatm => "aac-latm",
            Self::Flac => "flac",
            Self::Opus => "opus",
            Self::Vorbis => "vorbis",
            Self::Speex => "speex",
            Self::Mp3 => "mp3",
            Self::Alac => "alac",
            Self::WavPack => "wavpack",
            Self::MonkeyAudio => "ape",
            Self::Tta => "tta",
            Self::Musepack => "musepack",
            Self::AmrNb => "amr-nb",
            Self::AmrWb => "amr-wb",
            Self::Sbc => "sbc",
            Self::OggUnknown => "ogg-unknown",
            Self::Unknown => "unknown",
        }
    }
}

/// Result of one byte-prefix probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProbeResult {
    pub codec: CodecKind,
    pub encapsulation: Encapsulation,
    /// Confidence in percent, 0..=100.
    pub confidence: u8,
    /// Full IEC 61937 Pc data type/sub-data-type value from bits 0..6.
    pub iec61937_data_type: Option<u8>,
}

impl ProbeResult {
    const fn elementary(codec: CodecKind, confidence: u8) -> Self {
        Self {
            codec,
            encapsulation: Encapsulation::Elementary,
            confidence,
            iec61937_data_type: None,
        }
    }

    const fn container(codec: CodecKind, encapsulation: Encapsulation, confidence: u8) -> Self {
        Self {
            codec,
            encapsulation,
            confidence,
            iec61937_data_type: None,
        }
    }

    const fn unknown() -> Self {
        Self {
            codec: CodecKind::Unknown,
            encapsulation: Encapsulation::Unknown,
            confidence: 0,
            iec61937_data_type: None,
        }
    }
}

/// Probe a compressed/raw audio prefix.
pub fn probe(data: &[u8]) -> ProbeResult {
    if let Some(result) = probe_iec61937(data) {
        return result;
    }

    if data.starts_with(b"fLaC") {
        return ProbeResult::elementary(CodecKind::Flac, 100);
    }
    if data.starts_with(b"OggS") {
        let scan = &data[..data.len().min(512)];
        if contains(scan, b"OpusHead") {
            return ProbeResult::container(CodecKind::Opus, Encapsulation::Ogg, 100);
        }
        if contains(scan, b"\x01vorbis") {
            return ProbeResult::container(CodecKind::Vorbis, Encapsulation::Ogg, 100);
        }
        if contains(scan, b"Speex   ") {
            return ProbeResult::container(CodecKind::Speex, Encapsulation::Ogg, 100);
        }
        if contains(scan, b"fLaC") {
            return ProbeResult::container(CodecKind::Flac, Encapsulation::Ogg, 95);
        }
        return ProbeResult::container(CodecKind::OggUnknown, Encapsulation::Ogg, 80);
    }
    if is_wave(data) {
        return ProbeResult::container(CodecKind::Pcm, Encapsulation::Wave, 90);
    }
    if data.len() >= 12
        && &data[0..4] == b"FORM"
        && (&data[8..12] == b"AIFF" || &data[8..12] == b"AIFC")
    {
        return ProbeResult::container(CodecKind::Pcm, Encapsulation::Aiff, 95);
    }
    if data.starts_with(b"caff") {
        return ProbeResult::container(CodecKind::Pcm, Encapsulation::Caf, 95);
    }
    if data.starts_with(b"wvpk") {
        return ProbeResult::elementary(CodecKind::WavPack, 100);
    }
    if data.starts_with(b"MAC ") {
        return ProbeResult::elementary(CodecKind::MonkeyAudio, 100);
    }
    if data.starts_with(b"TTA1") {
        return ProbeResult::elementary(CodecKind::Tta, 100);
    }
    if data.starts_with(b"MPCK") || data.starts_with(b"MP+") {
        return ProbeResult::elementary(CodecKind::Musepack, 95);
    }
    if data.starts_with(b"#!AMR-WB\n") {
        return ProbeResult::elementary(CodecKind::AmrWb, 100);
    }
    if data.starts_with(b"#!AMR\n") {
        return ProbeResult::elementary(CodecKind::AmrNb, 100);
    }

    // TrueHD and DVD-Audio MLP major sync words.
    if data.starts_with(&[0xF8, 0x72, 0x6F, 0xBA]) {
        return ProbeResult::elementary(CodecKind::TrueHd, 100);
    }
    if data.starts_with(&[0xF8, 0x72, 0x6F, 0xBB]) {
        return ProbeResult::elementary(CodecKind::Mlp, 100);
    }

    // DTS-HD extension substream sync and the four common DTS core word orders.
    if data.starts_with(&[0x64, 0x58, 0x20, 0x25]) {
        return ProbeResult::elementary(CodecKind::DtsHd, 100);
    }
    if data.starts_with(&[0x7F, 0xFE, 0x80, 0x01])
        || data.starts_with(&[0xFE, 0x7F, 0x01, 0x80])
        || data.starts_with(&[0x1F, 0xFF, 0xE8, 0x00])
        || data.starts_with(&[0xFF, 0x1F, 0x00, 0xE8])
    {
        return ProbeResult::elementary(CodecKind::Dts, 100);
    }

    // AC-3 and E-AC-3 share 0x0B77. The 5-bit bsid is at byte 5 in
    // both syntaxes. bsid <= 10 is base AC-3; 11..=16 is Annex E.
    if data.len() >= 6 && data[0] == 0x0B && data[1] == 0x77 {
        let bsid = data[5] >> 3;
        if bsid <= 10 {
            return ProbeResult::elementary(CodecKind::Ac3, 100);
        }
        if bsid <= 16 {
            // JOC is intentionally not guessed from arbitrary coded bytes.
            // The decoder promotes Eac3 -> Eac3Joc only after positive OpenJOC
            // admission and successful speaker rendering.
            return ProbeResult::elementary(CodecKind::Eac3, 100);
        }
    }

    // AAC ADTS: 12-bit sync 0xFFF, layer == 00.
    if data.len() >= 2 && data[0] == 0xFF && (data[1] & 0xF6) == 0xF0 {
        return ProbeResult::elementary(CodecKind::AacAdts, 95);
    }
    // LOAS/LATM: 11-bit syncword 0x2B7 = 0x56E/0x56F prefix.
    if data.len() >= 2 && data[0] == 0x56 && (data[1] & 0xE0) == 0xE0 {
        return ProbeResult::elementary(CodecKind::AacLatm, 95);
    }

    // MP3 may begin with ID3 metadata or directly with an MPEG audio frame.
    if data.starts_with(b"ID3") {
        return ProbeResult::elementary(CodecKind::Mp3, 90);
    }
    if data.len() >= 2 && data[0] == 0xFF && (data[1] & 0xE0) == 0xE0 {
        let layer_bits = (data[1] >> 1) & 0x03;
        if layer_bits != 0 {
            return ProbeResult::elementary(CodecKind::Mp3, 85);
        }
    }

    // SBC frame sync (mSBC uses a different transport/header and should be
    // supplied with an explicit hint rather than guessed here).
    if data.first() == Some(&0x9C) {
        return ProbeResult::elementary(CodecKind::Sbc, 70);
    }

    ProbeResult::unknown()
}

fn probe_iec61937(data: &[u8]) -> Option<ProbeResult> {
    if data.len() < 8 {
        return None;
    }

    // Captured S16_LE carrier words: Pa=0xF872, Pb=0x4E1F. Pc bits 0..6 carry
    // the IEC61937 data-type/sub-data-type value; bit 7 is not part of it.
    if data[0..4] != [0x72, 0xF8, 0x1F, 0x4E] {
        return None;
    }
    let pc = u16::from_le_bytes([data[4], data[5]]);
    let data_type = (pc & 0x7F) as u8;
    let codec = match data_type {
        0x01 => CodecKind::Ac3,
        0x0B | 0x0C | 0x0D => CodecKind::Dts,
        0x11 => CodecKind::DtsHd,
        0x15 => CodecKind::Eac3,
        // IEC61937 type 0x16 is the MAT transport. A MAT payload is not the
        // same byte contract as a raw TrueHD elementary stream, so keep the
        // distinction explicit and let a dedicated MAT adapter own conversion.
        0x16 => CodecKind::DolbyMat,
        _ => CodecKind::Unknown,
    };
    Some(ProbeResult {
        codec,
        encapsulation: Encapsulation::Iec61937,
        confidence: if codec == CodecKind::Unknown { 70 } else { 100 },
        iec61937_data_type: Some(data_type),
    })
}

fn is_wave(data: &[u8]) -> bool {
    data.len() >= 12
        && ((&data[0..4] == b"RIFF" || &data[0..4] == b"RF64") && &data[8..12] == b"WAVE")
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    haystack.windows(needle.len()).any(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifies_iec61937_eac3() {
        let burst = [0x72, 0xF8, 0x1F, 0x4E, 0x15, 0x00, 0x20, 0x00];
        let p = probe(&burst);
        assert_eq!(p.encapsulation, Encapsulation::Iec61937);
        assert_eq!(p.codec, CodecKind::Eac3);
        assert_eq!(p.iec61937_data_type, Some(0x15));
    }

    #[test]
    fn iec61937_probe_preserves_full_seven_bit_type_and_mat_semantics() {
        let mat = [0x72, 0xF8, 0x1F, 0x4E, 0x16, 0x00, 0x00, 0x00];
        let p = probe(&mat);
        assert_eq!(p.codec, CodecKind::DolbyMat);
        assert_eq!(p.iec61937_data_type, Some(0x16));

        // A non-zero sub-data-type bit must not alias back to base AC-3.
        let extended = [0x72, 0xF8, 0x1F, 0x4E, 0x21, 0x00, 0x08, 0x00];
        let p = probe(&extended);
        assert_eq!(p.codec, CodecKind::Unknown);
        assert_eq!(p.iec61937_data_type, Some(0x21));
    }

    #[test]
    fn distinguishes_ac3_and_eac3_by_bsid() {
        let mut ac3 = [0u8; 8];
        ac3[0..2].copy_from_slice(&[0x0B, 0x77]);
        ac3[5] = 8 << 3;
        assert_eq!(probe(&ac3).codec, CodecKind::Ac3);

        let mut eac3 = ac3;
        eac3[5] = 16 << 3;
        assert_eq!(probe(&eac3).codec, CodecKind::Eac3);
    }

    #[test]
    fn identifies_lossless_and_object_carriers() {
        assert_eq!(probe(b"fLaCrest").codec, CodecKind::Flac);
        assert_eq!(probe(&[0xF8, 0x72, 0x6F, 0xBA, 0, 0]).codec, CodecKind::TrueHd);
        assert_eq!(probe(&[0x64, 0x58, 0x20, 0x25, 0, 0]).codec, CodecKind::DtsHd);
    }

    #[test]
    fn ogg_codec_headers_win_over_generic_ogg() {
        let opus = b"OggS000000000000OpusHead";
        assert_eq!(probe(opus).codec, CodecKind::Opus);
        let unknown = b"OggS000000000000something";
        assert_eq!(probe(unknown).codec, CodecKind::OggUnknown);
    }
}
