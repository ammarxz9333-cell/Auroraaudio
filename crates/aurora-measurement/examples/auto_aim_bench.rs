//! End-to-end SYNTHETIC proof; no phone microphone or physical motor connected.
//! Run: cargo run -p aurora-measurement --example auto_aim_bench
use aurora_measurement::auto_aim::{
    AimModulePlan, AimObjective, PhoneCaptureQuality,
};
use aurora_measurement::auto_aim_host::{
    calibrate_on_request, AutoAimRig, MotorFeedback, RecordedSweep,
};
use aurora_measurement::auto_aim_store::AtomicPresetFile;
use std::collections::BTreeMap;

#[derive(Default)]
struct SyntheticRig {
    angles: BTreeMap<String, i16>,
    calibration_on: bool,
    muted: bool,
}

impl AutoAimRig for SyntheticRig {
    fn begin_isolated_calibration(&mut self) -> Result<(), String> {
        self.calibration_on = true;
        self.muted = false;
        Ok(())
    }

    fn move_motor(&mut self, id: &str, angle: i16) -> Result<MotorFeedback, String> {
        self.angles.insert(id.to_owned(), angle);
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
        if !self.calibration_on {
            return Err("no explicit calibration session".into());
        }
        let mut samples = vec![0.0; reference.len() + 12_000];
        let (direct, early) = if id == "left" && angle == 20 {
            (0.1, 0.85)
        } else if id == "right" && angle == 0 {
            (0.85, 0.1)
        } else {
            (0.5, 0.3)
        };
        for (index, sample) in reference.iter().enumerate() {
            samples[index + 400] += direct * sample;
            samples[index + 1_050] += early * sample;
        }
        Ok(RecordedSweep {
            samples,
            quality: PhoneCaptureQuality {
                phone_session_id: "bench-explicit-run".into(),
                sample_rate_hz: 48_000,
                snr_db: 50.0,
                clipped_fraction: 0.0,
                unprocessed_capture: true, // only a simulation fixture
            },
        })
    }

    fn halt_output(&mut self) {
        self.muted = true;
    }

    fn finish_calibration(&mut self) {
        self.calibration_on = false;
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = std::env::temp_dir().join(format!(
        "aurora-auto-aim-bench-{}.json",
        std::process::id()
    ));
    let mut rig = SyntheticRig::default();
    let mut preset = AtomicPresetFile::new(&file);
    let plans = vec![
        AimModulePlan {
            module_id: "left".into(),
            previous_angle_deg: 0,
            candidates_deg: vec![0, 20],
            objective: AimObjective::EarlyReflection,
        },
        AimModulePlan {
            module_id: "right".into(),
            previous_angle_deg: 0,
            candidates_deg: vec![-20, 0],
            objective: AimObjective::DirectArrival,
        },
    ];
    let result = calibrate_on_request(
        &mut rig,
        &mut preset,
        plans,
        "bench-explicit-run".into(),
    ).map_err(|error| format!("synthetic calibration failed: {error:?}"))?;
    let saved = preset.load().map_err(|e| format!("preset read failed: {e}"))?
        .ok_or("missing saved preset")?;
    assert_eq!(saved.angles.len(), 2);
    assert!(rig.muted && !rig.calibration_on);
    assert_eq!(result.observed_angles, 4);
    println!("SIMULATION-ONLY: {} measured angles, {} saved motor poses",
        result.observed_angles, saved.angles.len());
    for angle in &saved.angles {
        println!("  {}: {} degrees", angle.module_id, angle.angle_deg);
    }
    let _ = std::fs::remove_file(file);
    Ok(())
}
