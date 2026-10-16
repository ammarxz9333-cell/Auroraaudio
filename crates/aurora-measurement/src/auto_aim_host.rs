//! On-demand offline orchestration of acoustic motor aim, measurement and save.
//!
//! All driver methods run on a CONTROL thread. Implementations must validate
//! motor encoders/limits and must not touch Aurora's realtime audio callback.
//! This is deliberately usable with a simulated capture rig before a phone app.
use super::auto_aim::{
    AimAction, AimError, AimModulePlan, AimSelection, AutoAimSession, PhoneCaptureQuality,
};
use super::auto_aim_dsp::{estimate_impulse_response, CaptureDspError};
use super::stimulus::{generate_log_sweep, LogSweepConfig};
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct RecordedSweep {
    pub samples: Vec<f32>,
    pub quality: PhoneCaptureQuality,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MotorFeedback {
    pub measured_angle_deg: i16,
    pub physically_settled: bool,
    pub within_travel_limits: bool,
}

/// Hardware abstraction for a calibrated external microphone and motor rig.
/// Never infer safe/settled from command acknowledgement alone.
pub trait AutoAimRig {
    fn begin_isolated_calibration(&mut self) -> Result<(), String>;
    fn move_motor(&mut self, module_id: &str, angle_deg: i16) -> Result<MotorFeedback, String>;
    fn record_sweep(
        &mut self,
        module_id: &str,
        angle_deg: i16,
        mono_48khz_sweep: &[f32],
    ) -> Result<RecordedSweep, String>;
    /// Must stop stimulus and mute before rollback on failure.
    fn halt_output(&mut self);
    fn finish_calibration(&mut self);
}

/// Store must commit all angles atomically or not at all. It cannot mutate an
/// existing preset before the entire motor set is verified.
pub trait AutoAimPresetStore {
    fn save_atomically(&mut self, choices: &[AimSelection]) -> Result<(), String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoAimHostError {
    InvalidPlan(AimError),
    MotorOrCapture(String),
    MotorUnsafe,
    Processing(CaptureDspError),
    Measurement(AimError),
    Persistence(String),
    /// Motor rollback could not be independently verified. Keep device muted.
    RollbackFailed,
}

#[derive(Debug, Clone)]
pub struct AutoAimReport {
    pub selected: Vec<AimSelection>,
    pub observed_angles: usize,
    /// Explicitly software integration, not physically verified sound quality.
    pub source: &'static str,
}

/// Start ONLY on explicit user authorization. This uses an existing external
/// recording interface, not a continuously active mobile microphone.
pub fn calibrate_on_request<R: AutoAimRig, S: AutoAimPresetStore>(
    rig: &mut R,
    store: &mut S,
    plans: Vec<AimModulePlan>,
    phone_session_id: String,
) -> Result<AutoAimReport, AutoAimHostError> {
    let (mut session, first) = AutoAimSession::begin_from_calibrate_button(plans, phone_session_id)
        .map_err(AutoAimHostError::InvalidPlan)?;
    let mut action = first;
    let mut observed_angles = 0;
    let result = (|| {
        rig.begin_isolated_calibration()
            .map_err(AutoAimHostError::MotorOrCapture)?;
        let stimulus = generate_log_sweep(LogSweepConfig {
            duration_seconds: 0.04,
            start_hz: 300.0,
            end_hz: 12_000.0,
            amplitude: 0.15,
            ..LogSweepConfig::default()
        })
        .map_err(|err| AutoAimHostError::MotorOrCapture(err.to_string()))?;
        loop {
            action = match action {
                AimAction::MoveMotor {
                    ref module_id,
                    angle_deg,
                } => {
                    let feedback = rig
                        .move_motor(module_id, angle_deg)
                        .map_err(AutoAimHostError::MotorOrCapture)?;
                    validate_motor(angle_deg, feedback)?;
                    session
                        .motor_settled(module_id, angle_deg)
                        .map_err(AutoAimHostError::Measurement)?
                }
                AimAction::PlaySweep {
                    ref module_id,
                    angle_deg,
                } => {
                    let mut recorded = rig
                        .record_sweep(module_id, angle_deg, &stimulus)
                        .map_err(AutoAimHostError::MotorOrCapture)?;
                    if recorded.samples.len() > 480_000 {
                        return Err(AutoAimHostError::Processing(CaptureDspError::TooLarge));
                    }
                    let impulse = estimate_impulse_response(
                        &stimulus,
                        &recorded.samples,
                        recorded.quality.sample_rate_hz,
                    )
                    .map_err(AutoAimHostError::Processing)?;
                    // Actual observed clipping always supersedes a client's claim.
                    if !recorded.quality.clipped_fraction.is_finite() {
                        return Err(AutoAimHostError::Measurement(AimError::PoorCapture));
                    }
                    recorded.quality.clipped_fraction = recorded
                        .quality
                        .clipped_fraction
                        .max(impulse.clipped_fraction);
                    observed_angles += 1;
                    session
                        .submit_impulse_response(
                            module_id,
                            angle_deg,
                            &impulse.impulse,
                            0,
                            &recorded.quality,
                        )
                        .map_err(AutoAimHostError::Measurement)?
                }
                AimAction::ProposePreset(ref choices) => {
                    // A proposed preset does not move or save hardware by itself.
                    let mut names = HashSet::new();
                    for selection in choices {
                        if !names.insert(&selection.module_id) {
                            return Err(AutoAimHostError::MotorUnsafe);
                        }
                        let feedback = rig
                            .move_motor(&selection.module_id, selection.angle_deg)
                            .map_err(AutoAimHostError::MotorOrCapture)?;
                        validate_motor(selection.angle_deg, feedback)?;
                    }
                    store
                        .save_atomically(choices)
                        .map_err(AutoAimHostError::Persistence)?;
                    session.saved().map_err(AutoAimHostError::Measurement)?;
                    return Ok(AutoAimReport {
                        selected: choices.clone(),
                        observed_angles,
                        source: "software-integration-not-physical-acoustic-proof",
                    });
                }
                AimAction::RestorePrevious(_) => {
                    return Err(AutoAimHostError::MotorUnsafe);
                }
            };
        }
    })();
    rig.halt_output();
    match result {
        Ok(report) => {
            rig.finish_calibration();
            Ok(report)
        }
        Err(original_error) => {
            // Always verify physical restoration of the old angles.
            let rollback = session.abort().ok().and_then(|a| match a {
                AimAction::RestorePrevious(previous) => Some(previous),
                _ => None,
            });
            let mut restored = rollback.is_some();
            if let Some(previous) = rollback {
                for (module, angle) in previous {
                    match rig.move_motor(&module, angle) {
                        Ok(feedback) if validate_motor(angle, feedback).is_ok() => (),
                        _ => restored = false,
                    }
                }
            }
            rig.finish_calibration();
            if restored {
                Err(original_error)
            } else {
                Err(AutoAimHostError::RollbackFailed)
            }
        }
    }
}

fn validate_motor(requested: i16, feedback: MotorFeedback) -> Result<(), AutoAimHostError> {
    if !feedback.physically_settled
        || !feedback.within_travel_limits
        || feedback.measured_angle_deg != requested
    {
        Err(AutoAimHostError::MotorUnsafe)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auto_aim::AimObjective;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct SimRig {
        angles: BTreeMap<String, i16>,
        running: bool,
        muted: bool,
        fail_capture: bool,
        fail_rollback: bool,
    }

    impl AutoAimRig for SimRig {
        fn begin_isolated_calibration(&mut self) -> Result<(), String> {
            self.running = true;
            self.muted = false;
            Ok(())
        }

        fn move_motor(&mut self, id: &str, angle: i16) -> Result<MotorFeedback, String> {
            if self.fail_rollback && self.muted {
                return Err("broken rollback motor".into());
            }
            self.angles.insert(id.into(), angle);
            Ok(MotorFeedback {
                measured_angle_deg: angle,
                physically_settled: true,
                within_travel_limits: true,
            })
        }

        fn record_sweep(
            &mut self,
            id: &str,
            angle: i16,
            reference: &[f32],
        ) -> Result<RecordedSweep, String> {
            if !self.running || self.fail_capture {
                return Err("capture unavailable".into());
            }
            let mut samples = vec![0.0_f32; reference.len() + 12_000];
            // Synthetic impulse: at angle 20, early return is stronger.
            let first_gain = if angle == 20 { 0.15 } else { 0.7 };
            let echo_gain = if angle == 20 { 0.85 } else { 0.15 };
            for (index, sample) in reference.iter().enumerate() {
                samples[index + 500] += sample * first_gain;
                samples[index + 1_150] += sample * echo_gain;
            }
            Ok(RecordedSweep {
                samples,
                quality: PhoneCaptureQuality {
                    phone_session_id: format!("{id}-calibration"),
                    sample_rate_hz: 48_000,
                    snr_db: 50.0,
                    clipped_fraction: 0.0,
                    unprocessed_capture: true, // simulation mock only
                },
            })
        }
        fn halt_output(&mut self) {
            self.muted = true;
        }
        fn finish_calibration(&mut self) {
            self.running = false;
        }
    }

    #[derive(Default)]
    struct MemStore {
        saved: Vec<AimSelection>,
        fail_save: bool,
    }
    impl AutoAimPresetStore for MemStore {
        fn save_atomically(&mut self, choices: &[AimSelection]) -> Result<(), String> {
            if self.fail_save {
                return Err("disk failure".into());
            }
            self.saved = choices.to_vec();
            Ok(())
        }
    }

    fn plan() -> AimModulePlan {
        AimModulePlan {
            module_id: "left".into(),
            previous_angle_deg: 0,
            candidates_deg: vec![0, 20],
            objective: AimObjective::EarlyReflection,
        }
    }

    #[test]
    fn simulated_end_to_end_capture_deconvolution_motor_and_preset() {
        let mut rig = SimRig::default();
        let mut store = MemStore::default();
        let report = calibrate_on_request(
            &mut rig,
            &mut store,
            vec![plan()],
            "left-calibration".into(),
        )
        .unwrap();
        assert_eq!(report.observed_angles, 2);
        assert_eq!(report.selected[0].angle_deg, 20);
        assert_eq!(store.saved[0].angle_deg, 20);
        assert_eq!(rig.angles["left"], 20);
        assert!(rig.muted);
        assert!(!rig.running);
    }

    #[test]
    fn failure_restores_original_angle_and_preserves_store() {
        let mut rig = SimRig {
            fail_capture: true,
            ..SimRig::default()
        };
        let mut store = MemStore::default();
        assert!(matches!(
            calibrate_on_request(
                &mut rig,
                &mut store,
                vec![plan()],
                "left-calibration".into()
            ),
            Err(AutoAimHostError::MotorOrCapture(_))
        ));
        assert_eq!(rig.angles["left"], 0);
        assert!(store.saved.is_empty());
        assert!(rig.muted);
    }

    #[test]
    fn fails_closed_when_rollback_actuator_unavailable() {
        let mut rig = SimRig {
            fail_capture: true,
            fail_rollback: true,
            ..SimRig::default()
        };
        let mut store = MemStore::default();
        assert!(matches!(
            calibrate_on_request(
                &mut rig,
                &mut store,
                vec![plan()],
                "left-calibration".into()
            ),
            Err(AutoAimHostError::RollbackFailed)
        ));
        assert!(rig.muted);
    }
}
