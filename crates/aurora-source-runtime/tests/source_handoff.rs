use aurora_core::{
    AudioBlock, AudioFormat, AudioObject, ChannelRole, Listener, SampleType, Speaker, Vector3,
};
use aurora_decoder_api::{DecodedFrame, Decoder, DecoderError, DecoderInfo};
use aurora_plugin_api::source_manager::{
    PlayableMediaRef, SourceAuthorization, SourceCapabilities, SourceKind, SourceManager,
    SourceMetadata, SourceRegistration, SOURCE_MANAGER_SCHEMA_VERSION,
};
use aurora_plugin_api::PluginPermission;
use aurora_renderer_basic::{BasicRenderer, BasicRendererMode};
use aurora_source_runtime::{MediaLoadError, MediaLoader, SourceMediaRuntime, SourceRuntimeError};

struct FixtureLoader {
    fail_path: Option<String>,
}

impl MediaLoader for FixtureLoader {
    fn load_chunk(&mut self, media: &PlayableMediaRef) -> Result<Vec<u8>, MediaLoadError> {
        match media {
            PlayableMediaRef::LocalFile { path } => {
                if self.fail_path.as_deref() == Some(path.as_str()) {
                    Err(MediaLoadError::LoadFailed(
                        "fixture load failure".to_owned(),
                    ))
                } else {
                    Ok(path.as_bytes().to_vec())
                }
            }
            _ => Err(MediaLoadError::UnsupportedReference(
                "fixture loader only accepts local files",
            )),
        }
    }
}

#[derive(Default)]
struct FixtureDecoder {
    configured: Option<AudioFormat>,
}

impl Decoder for FixtureDecoder {
    fn info(&self) -> DecoderInfo {
        DecoderInfo {
            name: "source-runtime-fixture",
            production_ready: false,
            maturity: "test",
        }
    }

    fn configure(&mut self, output_format: AudioFormat) -> Result<(), DecoderError> {
        self.configured = Some(output_format);
        Ok(())
    }

    fn decode_chunk(&mut self, input: &[u8]) -> Result<Option<DecodedFrame>, DecoderError> {
        let format = self
            .configured
            .ok_or(DecoderError::UnsupportedInput("decoder not configured"))?;
        if input.is_empty() {
            return Err(DecoderError::UnsupportedInput("empty fixture input"));
        }
        let frame_count = 4;
        let channels = (0..format.channel_count)
            .map(|channel| {
                (0..frame_count)
                    .map(|frame| (channel + frame + 1) as f32 / 10.0)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        Ok(Some(DecodedFrame {
            audio: AudioBlock {
                channels,
                frame_count,
                presentation_time_seconds: 0.0,
                discontinuity: true,
            },
            objects: vec![AudioObject {
                id: "object-1".to_owned(),
                position: Vector3::new(-1.0, 1.0, 1.2),
                velocity: Vector3::ZERO,
                gain_db: 0.0,
                spread: 0.0,
                start_time_seconds: Some(0.0),
                end_time_seconds: None,
            }],
        }))
    }

    fn reset(&mut self) {
        self.configured = None;
    }
}

fn authorization() -> SourceAuthorization {
    SourceAuthorization {
        principal_id: "org.aurora.local".to_owned(),
        permissions: vec![
            PluginPermission::PlaybackControl,
            PluginPermission::LocalMediaRead,
        ],
        builtin_trusted: false,
    }
}

fn registration(id: &str, priority: u16) -> SourceRegistration {
    SourceRegistration {
        schema_version: SOURCE_MANAGER_SCHEMA_VERSION,
        source_id: id.to_owned(),
        provider_id: "org.aurora.local".to_owned(),
        kind: SourceKind::LocalMedia,
        priority,
        authorization: authorization(),
    }
}

fn source_caps() -> SourceCapabilities {
    SourceCapabilities {
        can_seek: true,
        can_next: true,
        can_previous: true,
        can_pause: true,
        codec_hint: Some("fixture".to_owned()),
        channel_layout_hint: Some("stereo".to_owned()),
        sample_rate_hz: Some(48_000),
    }
}

fn audio_format() -> AudioFormat {
    AudioFormat {
        sample_rate: 48_000,
        channel_count: 2,
        sample_type: SampleType::F32,
        block_size: 256,
    }
}

fn listener() -> Listener {
    Listener {
        position: Vector3::ZERO,
        orientation: Vector3::new(0.0, 1.0, 0.0),
        ear_height: 1.2,
    }
}

fn speaker(id: &str, role: ChannelRole, x: f32) -> Speaker {
    Speaker {
        id: id.to_owned(),
        label: id.to_owned(),
        channel_role: role,
        position: Vector3::new(x, 1.0, 1.2),
        orientation: Vector3::new(0.0, -1.0, 0.0),
        gain_db: 0.0,
        delay_samples: 0.0,
        enabled: true,
    }
}

fn stereo_layout() -> Vec<Speaker> {
    vec![
        speaker("fl", ChannelRole::FrontLeft, -1.0),
        speaker("fr", ChannelRole::FrontRight, 1.0),
    ]
}

fn runtime(
    fail_path: Option<&str>,
) -> SourceMediaRuntime<FixtureLoader, FixtureDecoder, BasicRenderer> {
    SourceMediaRuntime::new(
        FixtureLoader {
            fail_path: fail_path.map(str::to_owned),
        },
        FixtureDecoder::default(),
        BasicRenderer::new(BasicRendererMode::NearestSpeaker),
    )
}

#[test]
fn local_source_flows_through_manager_decoder_and_real_renderer() {
    let mut manager = SourceManager::new();
    let session = manager.register(registration("local.main", 10)).unwrap();
    manager
        .prepare(
            &session,
            PlayableMediaRef::LocalFile {
                path: "/music/test.flac".to_owned(),
            },
            SourceMetadata::default(),
            source_caps(),
        )
        .unwrap();

    let mut runtime = runtime(None);
    let prepared = runtime
        .prepare_source_chunk(
            &manager,
            &session,
            audio_format(),
            &listener(),
            &stereo_layout(),
        )
        .unwrap();
    assert_eq!(prepared.audio().frame_count, 4);
    assert_eq!(prepared.objects().len(), 1);
    assert_eq!(prepared.speaker_gains().len(), 2);
    assert!(prepared.speaker_gains().iter().any(|gain| gain.gain > 0.0));

    assert!(matches!(
        runtime.commit_active(&manager, prepared.clone()),
        Err(SourceRuntimeError::NoActiveSource)
    ));
    manager.activate(&session).unwrap();
    let active = runtime.commit_active(&manager, prepared).unwrap();
    assert_eq!(active.session, session);
    assert_eq!(active.audio.frame_count, 4);
    assert_eq!(active.speaker_gains.len(), 2);
}

#[test]
fn media_preflight_failure_does_not_replace_active_source() {
    let mut manager = SourceManager::new();
    let first = manager.register(registration("local.first", 10)).unwrap();
    manager
        .prepare(
            &first,
            PlayableMediaRef::LocalFile {
                path: "/music/first.flac".to_owned(),
            },
            SourceMetadata::default(),
            source_caps(),
        )
        .unwrap();
    manager.activate(&first).unwrap();

    let second = manager.register(registration("local.second", 20)).unwrap();
    manager
        .prepare(
            &second,
            PlayableMediaRef::LocalFile {
                path: "/music/broken.flac".to_owned(),
            },
            SourceMetadata::default(),
            source_caps(),
        )
        .unwrap();

    let mut runtime = runtime(Some("/music/broken.flac"));
    assert!(matches!(
        runtime.prepare_source_chunk(
            &manager,
            &second,
            audio_format(),
            &listener(),
            &stereo_layout(),
        ),
        Err(SourceRuntimeError::MediaLoad(MediaLoadError::LoadFailed(_)))
    ));
    manager
        .fail_prepare(&second, "media preflight failed")
        .unwrap();
    assert_eq!(manager.active_session(), Some(&first));
}

#[test]
fn prepared_chunk_cannot_commit_for_an_inactive_session() {
    let mut manager = SourceManager::new();
    let first = manager.register(registration("local.first", 10)).unwrap();
    manager
        .prepare(
            &first,
            PlayableMediaRef::LocalFile {
                path: "/music/first.flac".to_owned(),
            },
            SourceMetadata::default(),
            source_caps(),
        )
        .unwrap();
    let mut runtime = runtime(None);
    let first_chunk = runtime
        .prepare_source_chunk(
            &manager,
            &first,
            audio_format(),
            &listener(),
            &stereo_layout(),
        )
        .unwrap();

    let second = manager.register(registration("local.second", 20)).unwrap();
    manager
        .prepare(
            &second,
            PlayableMediaRef::LocalFile {
                path: "/music/second.flac".to_owned(),
            },
            SourceMetadata::default(),
            source_caps(),
        )
        .unwrap();
    manager.activate(&second).unwrap();

    assert_eq!(
        runtime.commit_active(&manager, first_chunk),
        Err(SourceRuntimeError::SessionNotActive {
            source_id: "local.first".to_owned(),
            generation: 1,
        })
    );
}

#[test]
fn channel_only_decode_skips_object_renderer_without_hidden_fallback() {
    struct ChannelOnlyDecoder;

    impl Decoder for ChannelOnlyDecoder {
        fn info(&self) -> DecoderInfo {
            DecoderInfo {
                name: "channel-only",
                production_ready: false,
                maturity: "test",
            }
        }

        fn configure(&mut self, _: AudioFormat) -> Result<(), DecoderError> {
            Ok(())
        }

        fn decode_chunk(&mut self, _: &[u8]) -> Result<Option<DecodedFrame>, DecoderError> {
            Ok(Some(DecodedFrame {
                audio: AudioBlock {
                    channels: vec![vec![0.25; 4], vec![0.5; 4]],
                    frame_count: 4,
                    presentation_time_seconds: 0.0,
                    discontinuity: false,
                },
                objects: Vec::new(),
            }))
        }

        fn reset(&mut self) {}
    }

    let mut manager = SourceManager::new();
    let session = manager.register(registration("local.stereo", 10)).unwrap();
    manager
        .prepare(
            &session,
            PlayableMediaRef::LocalFile {
                path: "/music/stereo.flac".to_owned(),
            },
            SourceMetadata::default(),
            source_caps(),
        )
        .unwrap();

    let mut runtime = SourceMediaRuntime::new(
        FixtureLoader { fail_path: None },
        ChannelOnlyDecoder,
        BasicRenderer::new(BasicRendererMode::NearestSpeaker),
    );
    let prepared = runtime
        .prepare_source_chunk(&manager, &session, audio_format(), &listener(), &[])
        .unwrap();
    assert!(prepared.speaker_gains().is_empty());
}
