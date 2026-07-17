use std::collections::BTreeSet;

use aurora_config::*;
use aurora_diagnostics::{Severity, TruthSource};

pub fn configuration(kind: LayoutKind, renderer: RendererConfiguration) -> AuroraConfiguration {
    let roles: &[(&str, f32)] = match kind {
        LayoutKind::Stereo => &[("FL", -30.0), ("FR", 30.0)],
        LayoutKind::Surround51 => &[
            ("FL", -30.0),
            ("FR", 30.0),
            ("FC", 0.0),
            ("LFE", 5.0),
            ("SL", -110.0),
            ("SR", 110.0),
        ],
        LayoutKind::Surround71 => &[
            ("FL", -30.0),
            ("FR", 30.0),
            ("FC", 0.0),
            ("LFE", 5.0),
            ("SL", -90.0),
            ("SR", 90.0),
            ("SBL", -150.0),
            ("SBR", 150.0),
        ],
        LayoutKind::CustomHorizontal => &[
            ("west", -132.0),
            ("north-west", -51.5),
            ("east", 47.25),
            ("south-east", 161.0),
        ],
    };
    let inputs = roles
        .iter()
        .enumerate()
        .map(|(index, (role, _))| ChannelIdentity {
            id: format!("in-{index:02}-{role}"),
            label: format!("Input {role}"),
        })
        .collect::<Vec<_>>();
    let outputs = roles
        .iter()
        .enumerate()
        .map(|(index, (role, _))| ChannelIdentity {
            id: format!("out-{index:02}-{role}"),
            label: format!("Output {role}"),
        })
        .collect::<Vec<_>>();
    let routes = inputs
        .iter()
        .zip(&outputs)
        .map(|(input, output)| ChannelRoute {
            input: input.id.clone(),
            output: output.id.clone(),
        })
        .collect();
    let speakers = roles
        .iter()
        .enumerate()
        .map(|(index, (role, azimuth))| SpeakerConfiguration {
            id: format!("speaker-{index:02}-{role}"),
            azimuth_degrees: *azimuth,
            elevation_degrees: None,
            role: (*role).to_owned(),
            label: format!("Speaker {role}"),
            active: true,
        })
        .collect();
    AuroraConfiguration {
        schema: SchemaMetadata {
            schema_name: "aurora.configuration".to_owned(),
            schema_version: CURRENT_SCHEMA_VERSION,
            minimum_reader_version: MINIMUM_READER_VERSION,
            generated_by: Some("aurora-config-tests".to_owned()),
        },
        engine: EngineConfiguration {
            id: "engine-primary".to_owned(),
            operating_mode: OperatingMode::Offline,
            startup_behavior: StartupBehavior::AfterValidation,
            shutdown_behavior: ShutdownBehavior::Drain,
            recovery_policy: "fail-closed".to_owned(),
        },
        audio_format: AudioFormatIntent {
            sample_rate: 48_000,
            channel_count: roles.len() as u16,
            sample_format: SampleFormatIntent::Float32,
            callback_frames: 256,
            fallback_policy: FormatFallbackPolicy::Reject,
        },
        input_device: None,
        output_device: None,
        routing: RoutingConfiguration {
            inputs,
            outputs,
            routes,
            inactive_outputs: Vec::new(),
        },
        speaker_layout: SpeakerLayoutConfiguration {
            id: format!("layout-{kind:?}").to_lowercase(),
            kind,
            speakers,
            elevation_rendering: false,
        },
        renderer,
        buffering: BufferingPolicy {
            target_fill_frames: 512,
            minimum_fill_frames: 256,
            maximum_fill_frames: 1_024,
            ring_capacity_frames: 2_048,
            require_preallocated_storage: true,
        },
        diagnostics: DiagnosticsPolicy {
            severity_threshold: Severity::Info,
            retention_events: 256,
            output_mode: DiagnosticsOutputMode::Json,
            redaction_mode: RedactionMode::Strict,
            allowed_truth_sources: BTreeSet::from([
                TruthSource::UnitTest,
                TruthSource::DeterministicSimulation,
                TruthSource::HostApiObservation,
            ]),
        },
        simulation: Some(SimulationProfile {
            seed: 42,
            scenario_family: "configuration-contract".to_owned(),
            duration_class: SimulationDurationClass::Smoke,
            drift_ppm: 25.0,
            jitter_frames: 3,
            fault_profile: "none".to_owned(),
            replay_id: "seed-42".to_owned(),
        }),
    }
}

pub fn stereo() -> AuroraConfiguration {
    configuration(LayoutKind::Stereo, RendererConfiguration::Basic)
}

#[allow(dead_code)]
pub fn full_preset(id: &str) -> Preset {
    Preset {
        id: id.to_owned(),
        display_name: "Stereo Base".to_owned(),
        schema_version: CURRENT_SCHEMA_VERSION,
        preset_type: PresetType::FullConfiguration,
        payload: PresetPayload::FullConfiguration(Box::new(stereo())),
        description: Some("Validated stereo configuration".to_owned()),
        tags: vec!["stereo".to_owned(), "offline".to_owned()],
        extends: Vec::new(),
    }
}
