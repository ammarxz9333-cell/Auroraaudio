//! Explicitly triggered acoustic auto-aim control; no motor, phone, or Wi-Fi I/O.
//! A single microphone measures relative response, not reflection direction.

use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AimObjective {
    DirectArrival,
    EarlyReflection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AimModulePlan {
    pub module_id: String,
    pub previous_angle_deg: i16,
    pub candidates_deg: Vec<i16>,
    pub objective: AimObjective,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PhoneCaptureQuality {
    pub phone_session_id: String,
    pub sample_rate_hz: u32,
    pub snr_db: f32,
    pub clipped_fraction: f32,
    pub unprocessed_capture: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AimSelection {
    pub module_id: String,
    pub angle_deg: i16,
    pub score: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AimAction {
    MoveMotor { module_id: String, angle_deg: i16 },
    PlaySweep { module_id: String, angle_deg: i16 },
    ProposePreset(Vec<AimSelection>),
    RestorePrevious(Vec<(String, i16)>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AimPhase {
    AwaitMotor,
    AwaitCapture,
    AwaitSave,
    Complete,
    Aborted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AimError {
    InvalidPlan,
    InvalidSession,
    InvalidSequence,
    WrongTarget,
    PoorCapture,
    InvalidImpulse,
}

#[derive(Debug, Clone)]
pub struct AutoAimSession {
    plans: Vec<AimModulePlan>,
    phone_session_id: String,
    module_index: usize,
    angle_index: usize,
    scores: Vec<(i16, f64)>,
    selected: Vec<AimSelection>,
    phase: AimPhase,
}

impl AutoAimSession {
    /// The caller MUST invoke this only on an explicit app recalibration tap.
    pub fn begin_from_calibrate_button(
        plans: Vec<AimModulePlan>,
        phone_session_id: String,
    ) -> Result<(Self, AimAction), AimError> {
        if phone_session_id.trim().is_empty() || phone_session_id.len() > 128 {
            return Err(AimError::InvalidSession);
        }
        if plans.is_empty() || plans.len() > 12 {
            return Err(AimError::InvalidPlan);
        }
        let mut names = HashSet::new();
        for p in &plans {
            if p.module_id.trim().is_empty()
                || !names.insert(p.module_id.as_str())
                || p.previous_angle_deg.abs() > 90
                || p.candidates_deg.len() < 2
                || p.candidates_deg.len() > 25
                || !p.candidates_deg.contains(&p.previous_angle_deg)
            {
                return Err(AimError::InvalidPlan);
            }
            let mut angles = HashSet::new();
            for a in &p.candidates_deg {
                if a.abs() > 90 || !angles.insert(a) {
                    return Err(AimError::InvalidPlan);
                }
            }
        }
        let first = &plans[0];
        let action = AimAction::MoveMotor {
            module_id: first.module_id.clone(),
            angle_deg: first.candidates_deg[0],
        };
        Ok((
            Self {
                plans,
                phone_session_id,
                module_index: 0,
                angle_index: 0,
                scores: Vec::new(),
                selected: Vec::new(),
                phase: AimPhase::AwaitMotor,
            },
            action,
        ))
    }

    pub fn phase(&self) -> AimPhase {
        self.phase
    }

    /// Called by the actual motor adapter only after encoder/settle feedback.
    pub fn motor_settled(
        &mut self,
        module_id: &str,
        angle_deg: i16,
    ) -> Result<AimAction, AimError> {
        if self.phase != AimPhase::AwaitMotor {
            return Err(AimError::InvalidSequence);
        }
        let p = &self.plans[self.module_index];
        if p.module_id != module_id || p.candidates_deg[self.angle_index] != angle_deg {
            return Err(AimError::WrongTarget);
        }
        self.phase = AimPhase::AwaitCapture;
        Ok(AimAction::PlaySweep {
            module_id: module_id.into(),
            angle_deg,
        })
    }

    /// Input MUST be a deconvolved, time-aligned IR from a consented phone mic.
    /// Raw chirp recordings are not an impulse response.
    pub fn submit_impulse_response(
        &mut self,
        module_id: &str,
        angle_deg: i16,
        impulse: &[f32],
        onset_frame: usize,
        quality: &PhoneCaptureQuality,
    ) -> Result<AimAction, AimError> {
        if self.phase != AimPhase::AwaitCapture {
            return Err(AimError::InvalidSequence);
        }
        let p = &self.plans[self.module_index];
        if p.module_id != module_id || p.candidates_deg[self.angle_index] != angle_deg {
            return Err(AimError::WrongTarget);
        }
        if quality.phone_session_id != self.phone_session_id {
            return Err(AimError::InvalidSession);
        }
        if quality.sample_rate_hz != 48_000
            || !quality.unprocessed_capture
            || !quality.snr_db.is_finite()
            || quality.snr_db < 20.0
            || !quality.clipped_fraction.is_finite()
            || !(0.0..=0.001).contains(&quality.clipped_fraction)
        {
            return Err(AimError::PoorCapture);
        }
        let score = score_impulse(impulse, onset_frame, p.objective)?;
        self.scores.push((angle_deg, score));
        self.angle_index += 1;
        if self.angle_index < p.candidates_deg.len() {
            self.phase = AimPhase::AwaitMotor;
            return Ok(AimAction::MoveMotor {
                module_id: p.module_id.clone(),
                angle_deg: p.candidates_deg[self.angle_index],
            });
        }
        // Tie-break prefers the previously saved position.
        let mut angle = p.previous_angle_deg;
        let mut best = f64::NEG_INFINITY;
        for (candidate, value) in &self.scores {
            if *value > best + 1e-6
                || ((*value - best).abs() <= 1e-6
                    && (i32::from(*candidate) - i32::from(p.previous_angle_deg)).abs()
                        < (i32::from(angle) - i32::from(p.previous_angle_deg)).abs())
            {
                angle = *candidate;
                best = *value;
            }
        }
        self.selected.push(AimSelection {
            module_id: p.module_id.clone(),
            angle_deg: angle,
            score: best,
        });
        self.scores.clear();
        self.module_index += 1;
        self.angle_index = 0;
        if self.module_index == self.plans.len() {
            self.phase = AimPhase::AwaitSave;
            return Ok(AimAction::ProposePreset(self.selected.clone()));
        }
        let next = &self.plans[self.module_index];
        self.phase = AimPhase::AwaitMotor;
        Ok(AimAction::MoveMotor {
            module_id: next.module_id.clone(),
            angle_deg: next.candidates_deg[0],
        })
    }

    /// Only called after the driver verifies motors and atomically saves preset.
    pub fn saved(&mut self) -> Result<(), AimError> {
        if self.phase != AimPhase::AwaitSave {
            return Err(AimError::InvalidSequence);
        }
        self.phase = AimPhase::Complete;
        Ok(())
    }

    /// Caller must execute RestorePrevious on cancel, error, or disconnect.
    pub fn abort(&mut self) -> Result<AimAction, AimError> {
        if matches!(self.phase, AimPhase::Complete | AimPhase::Aborted) {
            return Err(AimError::InvalidSequence);
        }
        self.phase = AimPhase::Aborted;
        Ok(AimAction::RestorePrevious(
            self.plans
                .iter()
                .map(|p| (p.module_id.clone(), p.previous_angle_deg))
                .collect(),
        ))
    }
}

/// 48 kHz IR windows: [0,5ms) direct, [5,60ms) early, [60,200ms) late.
/// Does NOT identify a ceiling reflection, nor measure 3D arrival direction.
fn score_impulse(
    impulse: &[f32],
    onset_frame: usize,
    objective: AimObjective,
) -> Result<f64, AimError> {
    const DIRECT: usize = 240;
    const EARLY: usize = 2_880;
    const END: usize = 9_600;
    let end = onset_frame.checked_add(END).ok_or(AimError::InvalidImpulse)?;
    if impulse.len() > 96_000 || end > impulse.len() {
        return Err(AimError::InvalidImpulse);
    }
    let mut energy = [0.0_f64; 3];
    for (index, sample) in impulse[onset_frame..end].iter().enumerate() {
        if !sample.is_finite() {
            return Err(AimError::InvalidImpulse);
        }
        let band = if index < DIRECT {
            0
        } else if index < EARLY {
            1
        } else {
            2
        };
        let value = f64::from(*sample);
        energy[band] += value * value;
    }
    let total = energy.iter().sum::<f64>();
    if total <= 1e-12 || !total.is_finite() {
        return Err(AimError::InvalidImpulse);
    }
    let wanted = match objective {
        AimObjective::DirectArrival => energy[0],
        AimObjective::EarlyReflection => energy[1],
    };
    // Relative energy rather than blindly choosing the loudest angle.
    Ok(100.0 * wanted / total - 35.0 * energy[2] / total)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(objective: AimObjective) -> AimModulePlan {
        AimModulePlan {
            module_id: "left".into(),
            previous_angle_deg: 0,
            candidates_deg: vec![0, 20],
            objective,
        }
    }

    fn quality() -> PhoneCaptureQuality {
        PhoneCaptureQuality {
            phone_session_id: "tap-1".into(),
            sample_rate_hz: 48_000,
            snr_db: 30.0,
            clipped_fraction: 0.0,
            unprocessed_capture: true,
        }
    }

    fn ir(direct: f32, early: f32, late: f32) -> Vec<f32> {
        let mut result = vec![0.0; 10_000];
        result[100] = direct;
        result[600] = early;
        result[5_000] = late;
        result
    }

    #[test]
    fn manual_start_and_direct_optimization_require_ack_and_save() {
        let (mut session, start) = AutoAimSession::begin_from_calibrate_button(
            vec![plan(AimObjective::DirectArrival)],
            "tap-1".into(),
        )
        .unwrap();
        assert_eq!(start, AimAction::MoveMotor { module_id: "left".into(), angle_deg: 0 });
        assert_eq!(session.phase(), AimPhase::AwaitMotor);
        assert_eq!(session.motor_settled("left", 0).unwrap(), AimAction::PlaySweep { module_id: "left".into(), angle_deg: 0 });
        assert_eq!(session.submit_impulse_response("left", 0, &ir(0.1, 0.8, 0.1), 100, &quality()).unwrap(), AimAction::MoveMotor { module_id: "left".into(), angle_deg: 20 });
        session.motor_settled("left", 20).unwrap();
        match session.submit_impulse_response("left", 20, &ir(0.9, 0.1, 0.1), 100, &quality()).unwrap() {
            AimAction::ProposePreset(choices) => assert_eq!(choices[0].angle_deg, 20),
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(session.phase(), AimPhase::AwaitSave);
        session.saved().unwrap();
        assert_eq!(session.phase(), AimPhase::Complete);
        assert_eq!(session.abort(), Err(AimError::InvalidSequence));
    }

    #[test]
    fn reflected_mode_optimizes_early_energy_only() {
        let (mut s, _) = AutoAimSession::begin_from_calibrate_button(vec![plan(AimObjective::EarlyReflection)], "tap-1".into()).unwrap();
        s.motor_settled("left", 0).unwrap();
        s.submit_impulse_response("left", 0, &ir(0.9, 0.1, 0.1), 100, &quality()).unwrap();
        s.motor_settled("left", 20).unwrap();
        match s.submit_impulse_response("left", 20, &ir(0.1, 0.9, 0.1), 100, &quality()).unwrap() {
            AimAction::ProposePreset(choices) => assert_eq!(choices[0].angle_deg, 20),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn stale_bad_captures_do_not_advance_and_abort_restores() {
        let (mut s, _) = AutoAimSession::begin_from_calibrate_button(vec![plan(AimObjective::DirectArrival)], "tap-1".into()).unwrap();
        assert_eq!(s.motor_settled("left", 20), Err(AimError::WrongTarget));
        s.motor_settled("left", 0).unwrap();
        let mut q = quality();
        q.unprocessed_capture = false;
        assert_eq!(s.submit_impulse_response("left", 0, &ir(1.0, 0.0, 0.0), 100, &q), Err(AimError::PoorCapture));
        q = quality();
        q.phone_session_id = "wrong".into();
        assert_eq!(s.submit_impulse_response("left", 0, &ir(1.0, 0.0, 0.0), 100, &q), Err(AimError::InvalidSession));
        assert_eq!(s.submit_impulse_response("left", 0, &[0.0; 10], 0, &quality()), Err(AimError::InvalidImpulse));
        assert_eq!(s.phase(), AimPhase::AwaitCapture);
        assert_eq!(s.abort().unwrap(), AimAction::RestorePrevious(vec![("left".into(), 0)]));
    }

    #[test]
    fn rejects_unsafe_and_duplicate_motor_plans() {
        let mut p = plan(AimObjective::DirectArrival);
        p.candidates_deg = vec![0, 91];
        assert!(matches!(AutoAimSession::begin_from_calibrate_button(vec![p], "tap".into()), Err(AimError::InvalidPlan)));
        let p = plan(AimObjective::DirectArrival);
        assert!(matches!(AutoAimSession::begin_from_calibrate_button(vec![p.clone(), p], "tap".into()), Err(AimError::InvalidPlan)));
    }
}
