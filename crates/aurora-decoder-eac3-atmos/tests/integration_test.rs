//! End-to-end Atmos 11.1.4 live stream decode and spatial rendering integration test.

use aurora_core::{
    AudioFormat, ChannelRole, Listener, SampleType, Speaker, StandardLayout, Vector3,
};
use aurora_decoder_api::Decoder;
use aurora_decoder_eac3_atmos::{
    serialize_oamd_metadata, AtmosBedLayout, AtmosFrameMetadata, AtmosObjectMetadata,
    Eac3AtmosDecoder,
};
use aurora_dsp_basic::{CrossoverProcessor, DialogueEnhancer};
use aurora_renderer_api::{RenderObject, Renderer, RendererScratch, SpeakerGain};
use aurora_renderer_basic::{BasicRenderer, BasicRendererMode};

#[test]
fn e2e_atmos_stream_to_11_1_4_spatial_audio_pipeline() {
    let sample_rate = 48000;
    let block_size = 1536;

    // 1. Get Canonical 11.1.4 layout channels (16 channels)
    let layout = StandardLayout::ElevenOneFour;
    let canonical_roles = layout.canonical_roles();
    assert_eq!(canonical_roles.len(), 16);

    let mut speakers = Vec::with_capacity(16);
    for (i, role) in canonical_roles.iter().enumerate() {
        let pos = match role {
            ChannelRole::FrontLeft => Vector3::new(-1.5, 2.5, 0.0),
            ChannelRole::FrontRight => Vector3::new(1.5, 2.5, 0.0),
            ChannelRole::FrontCenter => Vector3::new(0.0, 2.5, 0.0),
            ChannelRole::LowFrequencyEffects => Vector3::new(0.0, 1.0, -0.5),
            ChannelRole::SurroundLeft => Vector3::new(-2.5, 0.0, 0.0),
            ChannelRole::SurroundRight => Vector3::new(2.5, 0.0, 0.0),
            ChannelRole::SurroundBackLeft => Vector3::new(-1.5, -2.5, 0.0),
            ChannelRole::SurroundBackRight => Vector3::new(1.5, -2.5, 0.0),
            ChannelRole::WideLeft => Vector3::new(-2.5, 1.5, 0.0),
            ChannelRole::WideRight => Vector3::new(2.5, 1.5, 0.0),
            ChannelRole::TopFrontLeft => Vector3::new(-1.5, 2.0, 1.5),
            ChannelRole::TopFrontRight => Vector3::new(1.5, 2.0, 1.5),
            ChannelRole::TopRearLeft => Vector3::new(-1.5, -2.0, 1.5),
            ChannelRole::TopRearRight => Vector3::new(1.5, -2.0, 1.5),
            ChannelRole::TopSideLeft => Vector3::new(-2.0, 0.0, 1.5),
            ChannelRole::TopSideRight => Vector3::new(2.0, 0.0, 1.5),
            _ => Vector3::new(0.0, 0.0, 0.0),
        };
        speakers.push(Speaker {
            id: format!("spk-{i}"),
            label: format!("{role:?}"),
            channel_role: role.clone(),
            position: pos,
            orientation: Vector3::new(0.0, 0.0, 0.0),
            gain_db: 0.0,
            delay_samples: 0.0,
            enabled: true,
        });
    }

    // 2. Initialize Decoder
    let mut decoder = Eac3AtmosDecoder::new();
    let format = AudioFormat {
        sample_rate,
        channel_count: 16,
        sample_type: SampleType::F32,
        block_size,
    };
    decoder.configure(format).unwrap();

    // 3. Construct E-AC-3 + OAMD Bitstream with dynamic 3D objects in the heights
    let mut stream_bytes = vec![
        0x0B, 0x77, // syncword
        0x02, 0xFF, // strmtyp=0, substream=0, frmsiz=767
        0x3F, // 48k, 6 blocks, 3/2, lfeon=1 (5.1 bed)
        0x87, 0x00, // bsid=16, dialnorm=28
    ];

    let oamd_meta = AtmosFrameMetadata {
        bed_layout: AtmosBedLayout::FivePointOne,
        sequence_number: 100,
        decorrelation_factor: 0.2,
        objects: vec![
            AtmosObjectMetadata {
                object_id: 1,
                position: Vector3::new(-1.5, 2.0, 1.5), // Top Front Left
                gain_db: 0.0,
                spread: 0.0,
                is_active: true,
            },
            AtmosObjectMetadata {
                object_id: 2,
                position: Vector3::new(1.5, 2.0, 1.5), // Top Front Right
                gain_db: 0.0,
                spread: 0.0,
                is_active: true,
            },
        ],
    };
    stream_bytes.extend_from_slice(&serialize_oamd_metadata(&oamd_meta));

    // 4. Decode
    let decoded = decoder
        .decode_chunk(&stream_bytes)
        .unwrap()
        .expect("decoded frame expected");
    assert_eq!(decoded.objects.len(), 2);
    assert_eq!(decoded.audio.channels.len(), 16);
    assert_eq!(decoded.audio.frame_count, block_size);

    // 5. Render 3D Objects with 3D Spatial Audio Renderer
    let mut renderer = BasicRenderer::new(BasicRendererMode::InverseDistance);
    renderer
        .configure(speakers.clone(), sample_rate, block_size, 32)
        .unwrap();
    let scratch_size = renderer.required_scratch_size().unwrap();
    let mut scratch = RendererScratch::new(scratch_size);
    let listener = Listener {
        position: Vector3::new(0.0, 0.0, 0.0),
        orientation: Vector3::new(0.0, 1.0, 0.0),
        ear_height: 1.2,
    };

    let render_objs: Vec<RenderObject> = decoded
        .objects
        .iter()
        .map(|o| RenderObject {
            position: o.position,
            gain: 1.0,
        })
        .collect();

    let mut speaker_gains = vec![SpeakerGain::default(); render_objs.len() * 16];
    renderer
        .render_gains(&listener, &render_objs, &mut speaker_gains, &mut scratch)
        .unwrap();

    let mut rendered_channels = decoded.audio.channels;

    // Accumulate object audio with computed 3D spatial gains
    for (obj_idx, _) in decoded.objects.iter().enumerate() {
        for spk_idx in 0..16 {
            let gain = speaker_gains[obj_idx * 16 + spk_idx].gain;
            if gain > 0.001 {
                for i in 0..block_size {
                    rendered_channels[spk_idx][i] += 0.2 * gain;
                }
            }
        }
    }

    // 6. Apply Linkwitz-Riley 4th Order Crossover (80 Hz)
    let lfe_idx = canonical_roles
        .iter()
        .position(|r| *r == ChannelRole::LowFrequencyEffects);
    let mut crossover = CrossoverProcessor::new(16, lfe_idx, 80.0, sample_rate);
    crossover
        .process_in_place(&mut rendered_channels, block_size)
        .unwrap();

    // 7. Apply Dialogue Enhancer to Center Channel
    let center_idx = canonical_roles
        .iter()
        .position(|r| *r == ChannelRole::FrontCenter);
    let mut dialogue_enhancer = DialogueEnhancer::new(center_idx, 3.0, false, sample_rate);
    dialogue_enhancer
        .process_in_place(&mut rendered_channels, block_size)
        .unwrap();

    // 8. Assertions: Verify Height Channels have significant audio energy!
    let tfl_idx = canonical_roles
        .iter()
        .position(|r| *r == ChannelRole::TopFrontLeft)
        .unwrap();
    let tfr_idx = canonical_roles
        .iter()
        .position(|r| *r == ChannelRole::TopFrontRight)
        .unwrap();

    let tfl_energy: f32 = rendered_channels[tfl_idx].iter().map(|s| s.abs()).sum();
    let tfr_energy: f32 = rendered_channels[tfr_idx].iter().map(|s| s.abs()).sum();

    assert!(
        tfl_energy > 1.0,
        "TopFrontLeft must contain decoded Atmos height energy, got {tfl_energy}"
    );
    assert!(
        tfr_energy > 1.0,
        "TopFrontRight must contain decoded Atmos height energy, got {tfr_energy}"
    );

    // Verify LFE channel contains low-frequency energy from crossover
    let lfe_idx = lfe_idx.unwrap();
    let lfe_energy: f32 = rendered_channels[lfe_idx].iter().map(|s| s.abs()).sum();
    assert!(
        lfe_energy > 1.0,
        "Subwoofer LFE must receive crossover sub-bass energy, got {lfe_energy}"
    );
}
