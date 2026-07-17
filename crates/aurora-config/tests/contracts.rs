mod common;

use std::sync::Arc;

use aurora_config::*;
use aurora_diagnostics::TruthSource;
use common::{configuration, full_preset, stereo};

#[test]
fn valid_minimal_and_standard_configurations() {
    let minimal = configuration(LayoutKind::CustomHorizontal, RendererConfiguration::Basic);
    assert!(ValidatedConfiguration::new(minimal).is_ok());
    assert!(ValidatedConfiguration::new(stereo()).is_ok());
    assert!(ValidatedConfiguration::new(configuration(
        LayoutKind::Surround51,
        RendererConfiguration::PointSourceVbap,
    ))
    .is_ok());
    assert!(ValidatedConfiguration::new(configuration(
        LayoutKind::Surround71,
        RendererConfiguration::HorizontalSpread { spread: 0.6 },
    ))
    .is_ok());
}

#[test]
fn irregular_horizontal_and_phase_renderer_vocabulary_validate() {
    for renderer in [
        RendererConfiguration::PointSourceVbap,
        RendererConfiguration::HorizontalSpread { spread: 0.0 },
        RendererConfiguration::HorizontalSpread { spread: 1.0 },
    ] {
        let validated =
            ValidatedConfiguration::new(configuration(LayoutKind::CustomHorizontal, renderer))
                .unwrap();
        assert!(validated
            .config()
            .speaker_layout
            .speakers
            .iter()
            .all(|speaker| speaker.azimuth_degrees.is_finite()));
    }
}

#[test]
fn canonical_serialization_is_stable_and_semantic() {
    let first = ValidatedConfiguration::new(stereo()).unwrap();
    let mut reordered = stereo();
    reordered.schema.generated_by = Some("different-generator".to_owned());
    reordered.routing.inputs.reverse();
    reordered.routing.outputs.reverse();
    reordered.routing.routes.reverse();
    reordered.speaker_layout.speakers.reverse();
    let second = ValidatedConfiguration::new(reordered).unwrap();
    let expected = first.canonical_json().unwrap();
    assert_eq!(expected, second.canonical_json().unwrap());
    assert!(first.semantically_eq(&second));
    assert!(!expected.contains("generated_by"));
    for _ in 0..3 {
        assert_eq!(
            first.canonical_json().unwrap().as_bytes(),
            expected.as_bytes()
        );
    }
}

#[test]
fn canonical_round_trip_preserves_validated_semantics() {
    let first = ValidatedConfiguration::new(configuration(
        LayoutKind::Surround51,
        RendererConfiguration::HorizontalSpread { spread: 0.25 },
    ))
    .unwrap();
    let bytes = first.canonical_json().unwrap();
    let second = ValidatedConfiguration::from_json(bytes.as_bytes()).unwrap();
    assert!(first.semantically_eq(&second));
    assert_eq!(
        first.human_readable().unwrap(),
        second.human_readable().unwrap()
    );
}

#[test]
fn invalid_numeric_values_and_bounds_are_structured() {
    let mut config = stereo();
    config.speaker_layout.speakers[0].azimuth_degrees = f32::NAN;
    let error = ValidatedConfiguration::new(config).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidNumericValue);

    let mut config = stereo();
    config.buffering.minimum_fill_frames = 600;
    let error = ValidatedConfiguration::new(config).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidBufferBounds);

    let mut config = stereo();
    config.speaker_layout.elevation_rendering = true;
    let error = ValidatedConfiguration::new(config).unwrap_err();
    assert_eq!(error.code, ErrorCode::UnsupportedReservedField);
}

#[test]
fn duplicate_ids_ambiguous_devices_and_invalid_routing_are_rejected() {
    let mut duplicate = stereo();
    duplicate.speaker_layout.speakers[1].id = duplicate.speaker_layout.speakers[0].id.clone();
    assert_eq!(
        ValidatedConfiguration::new(duplicate).unwrap_err().code,
        ErrorCode::DuplicateIdentifier
    );

    let mut ambiguous = stereo();
    ambiguous.output_device = Some(DeviceSelectionIntent {
        stable_id: None,
        friendly_name: Some("Speakers".to_owned()),
        backend: BackendIntent::Cpal,
        direction: DeviceDirection::Output,
        ambiguity_policy: AmbiguityPolicy::AllowFirst,
    });
    assert_eq!(
        ValidatedConfiguration::new(ambiguous).unwrap_err().code,
        ErrorCode::AmbiguousDeviceSelection
    );

    let mut routing = stereo();
    routing.routing.routes.pop();
    assert_eq!(
        ValidatedConfiguration::new(routing).unwrap_err().code,
        ErrorCode::InvalidRouting
    );
}

#[test]
fn unsupported_renderer_and_schema_are_rejected() {
    let mut schema = stereo();
    schema.schema.schema_version = 99;
    assert_eq!(
        ValidatedConfiguration::new(schema).unwrap_err().code,
        ErrorCode::UnsupportedSchemaVersion
    );

    let mut renderer = stereo();
    renderer.renderer = RendererConfiguration::HorizontalSpread { spread: 1.01 };
    assert_eq!(
        ValidatedConfiguration::new(renderer).unwrap_err().code,
        ErrorCode::UnsupportedRenderer
    );

    let json = ValidatedConfiguration::new(stereo())
        .unwrap()
        .canonical_json()
        .unwrap()
        .replace("\"type\":\"basic\"", "\"type\":\"unknown\"");
    assert_eq!(
        ValidatedConfiguration::from_json(json.as_bytes())
            .unwrap_err()
            .code,
        ErrorCode::InvalidJson
    );
}

#[test]
fn preset_creation_composition_and_canonical_order_are_deterministic() {
    let renderer = Preset {
        id: "renderer-wide".to_owned(),
        display_name: "Wide".to_owned(),
        schema_version: CURRENT_SCHEMA_VERSION,
        preset_type: PresetType::Renderer,
        payload: PresetPayload::Renderer(RendererConfiguration::HorizontalSpread { spread: 0.7 }),
        description: None,
        tags: vec!["wide".to_owned()],
        extends: Vec::new(),
    };
    let root = Preset {
        id: "composed".to_owned(),
        display_name: "Composed".to_owned(),
        schema_version: CURRENT_SCHEMA_VERSION,
        preset_type: PresetType::Renderer,
        payload: PresetPayload::Renderer(RendererConfiguration::HorizontalSpread { spread: 0.5 }),
        description: None,
        tags: vec!["two".to_owned(), "one".to_owned()],
        extends: vec!["base".to_owned()],
    };
    let collection = PresetCollection {
        schema_version: CURRENT_SCHEMA_VERSION,
        presets: vec![root, renderer, full_preset("base")],
    };
    let materialized = materialize_preset(&collection, "composed").unwrap();
    assert_eq!(
        materialized.config().renderer,
        RendererConfiguration::HorizontalSpread { spread: 0.5 }
    );
    assert_eq!(
        collection.canonical_json().unwrap(),
        collection.canonical_json().unwrap()
    );
}

#[test]
fn preset_conflicts_cycles_and_depth_are_rejected() {
    let overlay = |id: &str, extends: Vec<String>| Preset {
        id: id.to_owned(),
        display_name: id.to_owned(),
        schema_version: CURRENT_SCHEMA_VERSION,
        preset_type: PresetType::Renderer,
        payload: PresetPayload::Renderer(RendererConfiguration::Basic),
        description: None,
        tags: Vec::new(),
        extends,
    };
    let conflict = PresetCollection {
        schema_version: CURRENT_SCHEMA_VERSION,
        presets: vec![
            full_preset("base"),
            overlay("one", Vec::new()),
            overlay("two", Vec::new()),
            overlay("root", vec!["one".to_owned(), "two".to_owned()]),
        ],
    };
    assert_eq!(
        conflict.validate().unwrap_err().code,
        ErrorCode::PresetConflict
    );

    let cycle = PresetCollection {
        schema_version: CURRENT_SCHEMA_VERSION,
        presets: vec![
            overlay("a", vec!["b".to_owned()]),
            overlay("b", vec!["a".to_owned()]),
        ],
    };
    assert_eq!(
        materialize_preset(&cycle, "a").unwrap_err().code,
        ErrorCode::PresetCycle
    );

    let mut presets = vec![full_preset("base")];
    let mut previous = "base".to_owned();
    for index in 0..=MAX_COMPOSITION_DEPTH {
        let id = format!("depth-{index}");
        presets.push(overlay(&id, vec![previous]));
        previous = id;
    }
    let deep = PresetCollection {
        schema_version: CURRENT_SCHEMA_VERSION,
        presets,
    };
    assert_eq!(
        materialize_preset(&deep, &previous).unwrap_err().code,
        ErrorCode::PresetDepthExceeded
    );
}

#[test]
fn migration_succeeds_warns_and_rejects_ambiguous_input() {
    let source = include_bytes!("../../../fixtures/config/migration-source-v0.json");
    let migrated = migrate_v0_to_v1(source).unwrap();
    assert_eq!(migrated.warnings.len(), 1);
    assert!(migrated.changed_fields.contains("renderer.spread_percent"));
    let expected = ValidatedConfiguration::from_json(include_bytes!(
        "../../../fixtures/config/migration-expected-v1.json"
    ))
    .unwrap();
    assert!(migrated.configuration.semantically_eq(&expected));

    let invalid = br#"{"schema":{"schema_version":2}}"#;
    assert_eq!(
        migrate_v0_to_v1(invalid).unwrap_err().code,
        ErrorCode::UnsupportedMigration
    );
}

#[test]
fn redaction_removes_identifiers_and_reports_truth_source() {
    let mut source = stereo();
    source.output_device = Some(DeviceSelectionIntent {
        stable_id: Some("C:\\Users\\person\\device-id".to_owned()),
        friendly_name: Some("Personal speakers".to_owned()),
        backend: BackendIntent::Cpal,
        direction: DeviceDirection::Output,
        ambiguity_policy: AmbiguityPolicy::RequireStableIdentifier,
    });
    let validated = ValidatedConfiguration::new(source).unwrap();
    let redacted =
        RedactedConfiguration::new(&validated, RedactionMode::Strict, TruthSource::UnitTest);
    let json = redacted.canonical_json().unwrap();
    assert!(!json.contains("person"));
    assert!(!json.contains("Personal speakers"));
    assert!(json.contains("[redacted]"));
    assert!(json.contains("unit_test"));
}

#[test]
fn oversized_input_is_rejected_before_parsing() {
    let bytes = vec![b' '; MAX_SERIALIZED_BYTES + 1];
    assert_eq!(
        ValidatedConfiguration::from_json(&bytes).unwrap_err().code,
        ErrorCode::SerializedSizeExceeded
    );
}

#[test]
fn immutable_validated_configuration_supports_concurrent_reads() {
    let configuration = Arc::new(ValidatedConfiguration::new(stereo()).unwrap());
    let expected = configuration.canonical_json().unwrap();
    let threads = (0..8)
        .map(|_| {
            let configuration = Arc::clone(&configuration);
            let expected = expected.clone();
            std::thread::spawn(move || {
                for _ in 0..100 {
                    assert_eq!(configuration.canonical_json().unwrap(), expected);
                    assert_eq!(configuration.config().audio_format.sample_rate, 48_000);
                }
            })
        })
        .collect::<Vec<_>>();
    for thread in threads {
        thread.join().unwrap();
    }
}

#[test]
fn every_configuration_fixture_has_expected_validity() {
    let valid = [
        include_bytes!("../../../fixtures/config/minimal-v1.json").as_slice(),
        include_bytes!("../../../fixtures/config/stereo-basic-v1.json").as_slice(),
        include_bytes!("../../../fixtures/config/surround-5-1-v1.json").as_slice(),
        include_bytes!("../../../fixtures/config/surround-7-1-v1.json").as_slice(),
        include_bytes!("../../../fixtures/config/irregular-horizontal-v1.json").as_slice(),
        include_bytes!("../../../fixtures/config/phase-3a-point-source-v1.json").as_slice(),
        include_bytes!("../../../fixtures/config/phase-3b-spread-v1.json").as_slice(),
    ];
    for fixture in valid {
        assert!(ValidatedConfiguration::from_json(fixture).is_ok());
    }
    for fixture in [
        include_bytes!("../../../fixtures/config/invalid-duplicate-speaker-v1.json").as_slice(),
        include_bytes!("../../../fixtures/config/invalid-ambiguous-device-v1.json").as_slice(),
    ] {
        assert!(ValidatedConfiguration::from_json(fixture).is_err());
    }
}

#[test]
fn production_sources_forbid_unsafe_code() {
    let sources = [
        include_str!("../src/lib.rs"),
        include_str!("../src/error.rs"),
        include_str!("../src/limits.rs"),
        include_str!("../src/migration.rs"),
        include_str!("../src/model.rs"),
        include_str!("../src/preset.rs"),
        include_str!("../src/redaction.rs"),
        include_str!("../src/validation.rs"),
    ];
    assert!(sources[0].contains("#![forbid(unsafe_code)]"));
    assert!(sources.iter().all(|source| !source.contains("unsafe {")));
}
