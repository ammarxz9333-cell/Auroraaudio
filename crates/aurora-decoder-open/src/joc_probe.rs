//! Standards-derived E-AC-3 JOC admission probe.
//!
//! JOC is never inferred from the `Atmos` label, file extension, or random
//! compressed bytes. A frame is admitted only when:
//! 1. Annex-E BSI carries EC-3 Extension Type A in `addbsi`;
//! 2. the declared audio-block `skipfld` can be decoded at the real bit cursor;
//! 3. one skip field is a structurally valid EMDF container containing matching
//!    OAMD + JOC payloads accepted by the open TS 103 420 parser.

use oxideav_ac3::audblk::Ac3State;
use oxideav_ac3::eac3::{audfrm, bsi, dsp, joc};
use oxideav_core::bits::BitReader;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JocAdmission {
    /// The frame is ordinary E-AC-3 or has no Extension Type A signal.
    NotJoc,
    /// Extension Type A exists but the EMDF/OAMD/JOC payload is malformed,
    /// unsupported, or cannot be reached safely in this frame.
    SignalledButInvalid,
    /// Extension Type A + a valid matching EMDF OAMD/JOC container were parsed.
    Validated,
}

/// Stateful admission probe. The DSP state persists because E-AC-3 exponent,
/// coupling and overlap state can legally reuse information across frames.
pub struct JocAdmissionProbe {
    state: Ac3State,
    skip_fields: Vec<Vec<u8>>,
}

impl Default for JocAdmissionProbe {
    fn default() -> Self {
        Self {
            state: Ac3State::new(),
            skip_fields: Vec::new(),
        }
    }
}

impl JocAdmissionProbe {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.state = Ac3State::new();
        self.skip_fields.clear();
    }

    /// Validate one independent E-AC-3 syncframe. Concatenated dependent
    /// substreams are ignored here; their compatibility decode remains owned by
    /// the native E-AC-3 decoder.
    pub fn inspect(&mut self, frame: &[u8]) -> JocAdmission {
        if frame.len() < 6 || frame[0..2] != [0x0B, 0x77] {
            return JocAdmission::NotJoc;
        }
        let mut br = BitReader::new(&frame[2..]);
        let Ok(bsi) = bsi::parse_with(&mut br) else {
            return JocAdmission::NotJoc;
        };
        if !matches!(
            bsi.strmtyp,
            bsi::StreamType::Independent | bsi::StreamType::Ac3Convert
        ) {
            return JocAdmission::NotJoc;
        }
        let Some(addbsi) = bsi.addbsi.as_ref() else {
            return JocAdmission::NotJoc;
        };
        let signal = match joc::parse_ec3_extension_type_a(addbsi.payload()) {
            Ok(Some(signal)) => signal,
            Ok(None) => return JocAdmission::NotJoc,
            Err(_) => return JocAdmission::SignalledButInvalid,
        };

        let Ok(audfrm) = audfrm::parse_with(&mut br, &bsi) else {
            return JocAdmission::SignalledButInvalid;
        };
        let sample_count = usize::from(bsi.num_blocks)
            .saturating_mul(256)
            .saturating_mul(usize::from(bsi.nchans));
        if sample_count == 0 || sample_count > 1536 * 16 {
            return JocAdmission::SignalledButInvalid;
        }
        let mut scratch = vec![0.0_f32; sample_count];
        self.skip_fields.clear();
        if dsp::decode_indep_audblks(
            &bsi,
            &audfrm,
            &mut br,
            &mut self.state,
            &mut scratch,
            &mut self.skip_fields,
        )
        .is_err()
        {
            return JocAdmission::SignalledButInvalid;
        }

        if self
            .skip_fields
            .iter()
            .any(|container| joc::parse_joc_emdf(container, signal).is_ok())
        {
            JocAdmission::Validated
        } else {
            JocAdmission::SignalledButInvalid
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_garbage_is_never_promoted_to_joc() {
        let mut probe = JocAdmissionProbe::new();
        assert_eq!(probe.inspect(b"not eac3"), JocAdmission::NotJoc);
    }

    #[test]
    fn bare_syncword_without_valid_bsi_is_not_joc() {
        let mut probe = JocAdmissionProbe::new();
        assert_ne!(
            probe.inspect(&[0x0B, 0x77, 0, 0, 0, 16 << 3]),
            JocAdmission::Validated
        );
    }
}
