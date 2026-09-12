use aurora_config::*;
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn fixture(bytes: &[u8]) -> AuroraConfiguration {
    serde_json::from_slice(bytes).unwrap()
}

fn sixteen_speakers() -> AuroraConfiguration {
    let mut config = fixture(include_bytes!(
        "../../../fixtures/config/irregular-horizontal-v2.json"
    ));
    config.audio_format.channel_count = 16;
    config.routing.inputs.clear();
    config.routing.outputs.clear();
    config.routing.routes.clear();
    config.speaker_layout.speakers.clear();
    for index in 0..16 {
        let input = format!("input-{index:02}");
        let output = format!("output-{index:02}");
        config.routing.inputs.push(ChannelIdentity {
            id: input.clone(),
            label: input.clone(),
        });
        config.routing.outputs.push(ChannelIdentity {
            id: output.clone(),
            label: output.clone(),
        });
        config.routing.routes.push(ChannelRoute { input, output });
        config.speaker_layout.speakers.push(SpeakerConfiguration {
            id: format!("speaker-{index:02}"),
            azimuth_degrees: -180.0 + index as f32 * 22.5,
            elevation_degrees: None,
            role: format!("custom-{index:02}"),
            label: format!("Speaker {index:02}"),
            active: true,
        });
    }
    config
}

fn preset_collection(config: AuroraConfiguration) -> PresetCollection {
    PresetCollection {
        schema_version: CURRENT_SCHEMA_VERSION,
        presets: vec![Preset {
            id: "base".to_owned(),
            display_name: "Base".to_owned(),
            schema_version: CURRENT_SCHEMA_VERSION,
            preset_type: PresetType::FullConfiguration,
            payload: PresetPayload::FullConfiguration(Box::new(config)),
            description: None,
            tags: vec!["benchmark".to_owned()],
            extends: Vec::new(),
        }],
    }
}

fn benchmarks(criterion: &mut Criterion) {
    let minimal = fixture(include_bytes!("../../../fixtures/config/minimal-v2.json"));
    let sixteen = sixteen_speakers();
    let validated = ValidatedConfiguration::new(sixteen.clone()).unwrap();
    let canonical = validated.canonical_json().unwrap();
    let presets = preset_collection(sixteen);
    let migration = include_bytes!("../../../fixtures/config/migration-source-v0.json");

    criterion.bench_function("config/validate_minimal", |bencher| {
        bencher.iter(|| ValidatedConfiguration::new(black_box(minimal.clone())).unwrap())
    });
    criterion.bench_function("config/validate_16_speakers", |bencher| {
        bencher.iter(|| ValidatedConfiguration::new(black_box(sixteen_speakers())).unwrap())
    });
    criterion.bench_function("config/canonical_serialize_16_speakers", |bencher| {
        bencher.iter(|| black_box(&validated).canonical_json().unwrap())
    });
    criterion.bench_function("config/canonical_deserialize_16_speakers", |bencher| {
        bencher.iter(|| ValidatedConfiguration::from_json(black_box(canonical.as_bytes())).unwrap())
    });
    criterion.bench_function("config/materialize_preset", |bencher| {
        bencher.iter(|| materialize_preset(black_box(&presets), "base").unwrap())
    });
    criterion.bench_function("config/migrate_v0_to_v2", |bencher| {
        bencher.iter(|| migrate_v0_to_v2(black_box(migration)).unwrap())
    });
}

criterion_group!(configuration, benchmarks);
criterion_main!(configuration);
