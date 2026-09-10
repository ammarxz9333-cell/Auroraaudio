//! OpenJOC-backed E-AC-3 JOC admission probe.
//!
//! Aurora does not infer JOC/Atmos from IEC 61937 data type 0x15, labels, file
//! extensions, or random compressed bytes. Complete E-AC-3 access units are
//! classified by OpenJOC's own parser and positive-admission rules. OxideAV is
//! retained here only for a lightweight Annex-E header cross-check when the
//! OpenJOC classifier reports invalid/unsupported input; it no longer decodes
//! audio blocks merely to decide whether OpenJOC may run.

use aurora_decoder_api::DecodedFrame;
use openjoc_ffmpeg::{JocClassification, classify_complete_access_unit};
use oxideav_ac3::eac3::{bsi, joc};
use oxideav_core::bits::BitReader;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JocAdmission {
    /// The access unit is ordinary E-AC-3 or has no positively admitted JOC.
    NotJoc,
    /// Extension Type A appears to be signalled, but OpenJOC rejected the
    /// complete access unit as invalid or unsupported.
    SignalledButInvalid,
    /// OpenJOC positively classified the complete access unit as JOC.
    Validated,
}

/// Stateless compatibility wrapper around OpenJOC's complete-AU classifier.
/// Keeping this type preserves Aurora's existing decoder API while removing the
/// previous duplicate OxideAV audblk decode/allocation from the realtime path.
#[derive(Debug, Default)]
pub struct JocAdmissionProbe;

impl JocAdmissionProbe {
    pub const fn new() -> Self {
        Self
    }

    pub fn reset(&mut self) {}

    /// Classify one complete E-AC-3 access unit. `Validated` is only an
    /// admission result; Aurora still claims JOC/Atmos playback only after the
    /// OpenJOC speaker renderer successfully accepts and renders the unit.
    pub fn inspect(&mut self, access_unit: &[u8]) -> JocAdmission {
        match classify_complete_access_unit(access_unit) {
            JocClassification::ConfirmedJoc => JocAdmission::Validated,
            JocClassification::ConfirmedNonJoc => JocAdmission::NotJoc,
            JocClassification::InvalidOrUnsupported | JocClassification::Unknown => {
                if extension_type_a_candidate(access_unit) {
                    JocAdmission::SignalledButInvalid
                } else {
                    JocAdmission::NotJoc
                }
            }
        }
    }
}

/// Return consumed planar storage to whichever reusable open backend currently
/// owns the output path. The storage contains plain `Vec<f32>` planes, so it is
/// safe to reuse across frames only after each backend revalidates its expected
/// channel geometry and capacity. If no recyclable backend is active, dropping
/// the frame remains the fail-safe behavior.
impl super::UniversalOpenDecoder {
    pub fn recycle_decoded_frame(&mut self, frame: DecodedFrame) {
        if let Some(renderer) = self.joc_renderer.as_mut() {
            renderer.recycle_frame(frame);
        } else if let Some(worker) = self.worker.as_mut() {
            worker.recycle_frame(frame);
        }
    }
}

/// Best-effort header-only cross-check used only to preserve Aurora's existing
/// `SignalledButInvalid` diagnostic. This intentionally does not decode audio
/// blocks and is not allowed to positively admit JOC.
fn extension_type_a_candidate(access_unit: &[u8]) -> bool {
    if access_unit.len() < 6 || access_unit[0..2] != [0x0B, 0x77] {
        return false;
    }

    let mut br = BitReader::new(&access_unit[2..]);
    let Ok(bsi) = bsi::parse_with(&mut br) else {
        return false;
    };
    let Some(addbsi) = bsi.addbsi.as_ref() else {
        return false;
    };

    match joc::parse_ec3_extension_type_a(addbsi.payload()) {
        Ok(Some(_)) => true,
        Ok(None) => false,
        Err(_) => true,
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

    #[test]
    fn reset_is_safe_for_stateless_probe() {
        let mut probe = JocAdmissionProbe::new();
        probe.reset();
        assert_eq!(probe.inspect(b"not eac3"), JocAdmission::NotJoc);
    }
}
