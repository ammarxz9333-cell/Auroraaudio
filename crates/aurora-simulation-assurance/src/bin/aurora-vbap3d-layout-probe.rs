use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use aurora_core::{ChannelRole, Listener, Speaker, Vector3};
use aurora_renderer_api::{RenderObject, Renderer, RendererScratch, SpeakerGain};
use aurora_renderer_vbap::Vbap3dRenderer;
use clap::Parser;
use serde::{Deserialize, Serialize};

const POWER_TOLERANCE: f32 = 1.0e-4;
const ZERO_TOLERANCE: f32 = 1.0e-6;
const MIRROR_TOLERANCE: f32 = 2.0e-4;
const ANCHOR_MIN_GAIN: f32 = 0.999;

#[derive(Debug, Parser)]
#[command(name = "aurora-vbap3d-layout-probe")]
#[command(about = "Validate deterministic 3D VBAP layout semantics without external renderer assumptions")]
struct Cli {
    #[arg(long)]
    scene: PathBuf,
    #[arg(long)]
    output: PathBuf,
    #[arg(long, default_value_t = 48_000)]
    sample_rate: u32,
}

#[derive(Debug, Deserialize)]
struct Fixture {
    listener: Listener,
    speakers: Vec<Speaker>,
    block_size: usize,
}

#[derive(Debug, Serialize)]
struct Check {
    name: String,
    passed: bool,
    detail: String,
}

#[derive(Debug, Serialize)]
struct AnchorResult {
    speaker_id: String,
    dominant_speaker_id: String,
    anchor_gain: f32,
    spatial_power: f32,
    lfe_max_abs_gain: f32,
    passed: bool,
}

#[derive(Debug, Serialize)]
struct MirrorResult {
    label: String,
    max_abs_gain_error: f32,
    passed: bool,
}

#[derive(Debug, Serialize)]
struct DirectionResult {
    label: String,
    centroid_y: f32,
    top_energy: f32,
    spatial_power: f32,
    lfe_max_abs_gain: f32,
}

#[derive(Debug, Serialize)]
struct MultiObjectResult {
    first_dominant_speaker_id: String,
    second_dominant_speaker_id: String,
    first_spatial_power: f32,
    second_spatial_power: f32,
    lfe_max_abs_gain: f32,
    passed: bool,
}

#[derive(Debug, Serialize)]
struct Evidence {
    schema_version: u32,
    artifact: &'static str,
    scene: String,
    output_channels: usize,
    spatial_speakers: usize,
    validated_triplets: usize,
    listener_inside_hull: bool,
    anchor_results: Vec<AnchorResult>,
    mirror_results: Vec<MirrorResult>,
    direction_results: Vec<DirectionResult>,
    multi_object: MultiObjectResult,
    checks: Vec<Check>,
    passed: bool,
    truth_boundary: &'static str,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.sample_rate == 0 {
        bail!("sample rate must be greater than zero");
    }

    let bytes = fs::read(&cli.scene).with_context(|| format!("read {}", cli.scene.display()))?;
    let fixture: Fixture = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse {}", cli.scene.display()))?;
    if fixture.block_size == 0 {
        bail!("fixture block size must be greater than zero");
    }

    let mut speakers = fixture
        .speakers
        .into_iter()
        .filter(|speaker| speaker.enabled)
        .collect::<Vec<_>>();
    if speakers.is_empty() {
        bail!("fixture has no enabled speakers");
    }
    for speaker in &mut speakers {
        speaker.gain_db = 0.0;
    }

    let lfe_indices = speakers
        .iter()
        .enumerate()
        .filter_map(|(index, speaker)| {
            matches!(speaker.channel_role, ChannelRole::LowFrequencyEffects).then_some(index)
        })
        .collect::<Vec<_>>();
    if lfe_indices.len() != 1 {
        bail!("expected exactly one LFE channel, found {}", lfe_indices.len());
    }
    let spatial_indices = speakers
        .iter()
        .enumerate()
        .filter_map(|(index, speaker)| (!lfe_indices.contains(&index)).then_some(index))
        .collect::<Vec<_>>();

    let mut renderer = Vbap3dRenderer::new();
    renderer
        .configure(speakers.clone(), cli.sample_rate, fixture.block_size, 2)
        .context("configure 3D VBAP renderer")?;
    renderer
        .prepare_listener(&fixture.listener)
        .context("prepare listener-relative 3D hull")?;
    let validated_triplets = renderer.validated_triplets().len();
    let listener_inside_hull = renderer.listener_inside_hull();
    let output_channels = renderer.output_channel_count();
    let mut scratch = RendererScratch::new(renderer.required_scratch_size()?.float_count);

    let mut checks = Vec::new();
    checks.push(Check {
        name: "closed_hull".to_owned(),
        passed: listener_inside_hull && validated_triplets > 0,
        detail: format!("inside={listener_inside_hull} triplets={validated_triplets}"),
    });

    let mut anchor_results = Vec::new();
    for &speaker_index in &spatial_indices {
        renderer.reset();
        let gains = render(
            &mut renderer,
            &fixture.listener,
            &[RenderObject {
                position: speakers[speaker_index].position,
                gain: 1.0,
            }],
            &mut scratch,
            output_channels,
        )?;
        let chunk = &gains[..output_channels];
        let dominant = dominant_index(chunk);
        let spatial_power = spatial_power(chunk, &spatial_indices);
        let lfe_max = lfe_max_abs(chunk, &lfe_indices);
        let passed = all_finite_nonnegative(chunk)
            && (spatial_power - 1.0).abs() <= POWER_TOLERANCE
            && lfe_max <= ZERO_TOLERANCE
            && dominant == speaker_index
            && chunk[speaker_index].gain >= ANCHOR_MIN_GAIN;
        anchor_results.push(AnchorResult {
            speaker_id: speakers[speaker_index].id.clone(),
            dominant_speaker_id: speakers[dominant].id.clone(),
            anchor_gain: chunk[speaker_index].gain,
            spatial_power,
            lfe_max_abs_gain: lfe_max,
            passed,
        });
    }
    let anchors_passed = anchor_results.iter().all(|result| result.passed);
    checks.push(Check {
        name: "speaker_anchors".to_owned(),
        passed: anchors_passed,
        detail: format!("passed={}/{}", anchor_results.iter().filter(|r| r.passed).count(), anchor_results.len()),
    });

    let mirror_map = build_mirror_map(&speakers, &fixture.listener)?;
    let ear_z = fixture.listener.position.z + fixture.listener.ear_height;
    let mirror_sources = [
        ("front-height", Vector3::new(-0.65, 0.90, ear_z + 0.25)),
        ("rear-height", Vector3::new(-0.80, -0.70, ear_z + 0.35)),
        ("side-low", Vector3::new(-0.75, 0.15, ear_z - 0.55)),
    ];
    let mut mirror_results = Vec::new();
    for (label, left_source) in mirror_sources {
        let right_source = Vector3::new(
            2.0 * fixture.listener.position.x - left_source.x,
            left_source.y,
            left_source.z,
        );
        renderer.reset();
        let left = render(
            &mut renderer,
            &fixture.listener,
            &[RenderObject { position: left_source, gain: 1.0 }],
            &mut scratch,
            output_channels,
        )?;
        renderer.reset();
        let right = render(
            &mut renderer,
            &fixture.listener,
            &[RenderObject { position: right_source, gain: 1.0 }],
            &mut scratch,
            output_channels,
        )?;
        let mut max_error = 0.0_f32;
        for (left_index, &right_index) in mirror_map.iter().enumerate() {
            max_error = max_error.max((left[left_index].gain - right[right_index].gain).abs());
        }
        mirror_results.push(MirrorResult {
            label: label.to_owned(),
            max_abs_gain_error: max_error,
            passed: max_error <= MIRROR_TOLERANCE,
        });
    }
    let mirrors_passed = mirror_results.iter().all(|result| result.passed);
    checks.push(Check {
        name: "left_right_mirror_symmetry".to_owned(),
        passed: mirrors_passed,
        detail: format!("max_error={:.7}", mirror_results.iter().map(|r| r.max_abs_gain_error).fold(0.0_f32, f32::max)),
    });

    let direction_sources = [
        ("front", Vector3::new(0.0, 1.0, ear_z)),
        ("rear", Vector3::new(0.0, -1.0, ear_z)),
        ("high-front", Vector3::new(0.0, 0.8, ear_z + 1.0)),
        ("low-front", Vector3::new(0.0, 0.8, ear_z - 0.8)),
    ];
    let speaker_units = speaker_unit_vectors(&speakers, &fixture.listener);
    let top_indices = speakers
        .iter()
        .enumerate()
        .filter_map(|(index, speaker)| speaker.channel_role.as_str().starts_with("top-").then_some(index))
        .collect::<Vec<_>>();
    if top_indices.is_empty() {
        bail!("layout semantics probe requires at least one top speaker");
    }
    let mut direction_results = Vec::new();
    for (label, source) in direction_sources {
        renderer.reset();
        let gains = render(
            &mut renderer,
            &fixture.listener,
            &[RenderObject { position: source, gain: 1.0 }],
            &mut scratch,
            output_channels,
        )?;
        let chunk = &gains[..output_channels];
        direction_results.push(DirectionResult {
            label: label.to_owned(),
            centroid_y: energy_centroid_y(chunk, &speaker_units, &spatial_indices),
            top_energy: top_energy(chunk, &top_indices),
            spatial_power: spatial_power(chunk, &spatial_indices),
            lfe_max_abs_gain: lfe_max_abs(chunk, &lfe_indices),
        });
    }
    let front = direction_results.iter().find(|result| result.label == "front").unwrap();
    let rear = direction_results.iter().find(|result| result.label == "rear").unwrap();
    let high = direction_results.iter().find(|result| result.label == "high-front").unwrap();
    let low = direction_results.iter().find(|result| result.label == "low-front").unwrap();
    let direction_power_lfe_ok = direction_results.iter().all(|result| {
        (result.spatial_power - 1.0).abs() <= POWER_TOLERANCE
            && result.lfe_max_abs_gain <= ZERO_TOLERANCE
    });
    checks.push(Check {
        name: "front_rear_centroid".to_owned(),
        passed: front.centroid_y > 0.20 && rear.centroid_y < -0.20,
        detail: format!("front_y={:.5} rear_y={:.5}", front.centroid_y, rear.centroid_y),
    });
    checks.push(Check {
        name: "height_energy_ordering".to_owned(),
        passed: high.top_energy > low.top_energy + 0.20 && high.top_energy > 0.50,
        detail: format!("high={:.6} low={:.6}", high.top_energy, low.top_energy),
    });
    checks.push(Check {
        name: "direction_power_and_lfe".to_owned(),
        passed: direction_power_lfe_ok,
        detail: "front/rear/high/low probes retain unit spatial power and zero LFE object gain".to_owned(),
    });

    renderer.reset();
    let objects = [
        RenderObject { position: Vector3::new(-0.75, 0.85, ear_z + 0.20), gain: 1.0 },
        RenderObject { position: Vector3::new(0.70, -0.80, ear_z + 0.45), gain: 1.0 },
    ];
    let multi = render(
        &mut renderer,
        &fixture.listener,
        &objects,
        &mut scratch,
        output_channels,
    )?;
    let first = &multi[..output_channels];
    let second = &multi[output_channels..];
    let first_dominant = dominant_index(first);
    let second_dominant = dominant_index(second);
    let first_power = spatial_power(first, &spatial_indices);
    let second_power = spatial_power(second, &spatial_indices);
    let multi_lfe = lfe_max_abs(first, &lfe_indices).max(lfe_max_abs(second, &lfe_indices));
    let multi_passed = all_finite_nonnegative(first)
        && all_finite_nonnegative(second)
        && (first_power - 1.0).abs() <= POWER_TOLERANCE
        && (second_power - 1.0).abs() <= POWER_TOLERANCE
        && multi_lfe <= ZERO_TOLERANCE
        && first_dominant != second_dominant;
    let multi_object = MultiObjectResult {
        first_dominant_speaker_id: speakers[first_dominant].id.clone(),
        second_dominant_speaker_id: speakers[second_dominant].id.clone(),
        first_spatial_power: first_power,
        second_spatial_power: second_power,
        lfe_max_abs_gain: multi_lfe,
        passed: multi_passed,
    };
    checks.push(Check {
        name: "two_object_independence".to_owned(),
        passed: multi_passed,
        detail: format!(
            "dominants={} / {}",
            multi_object.first_dominant_speaker_id, multi_object.second_dominant_speaker_id
        ),
    });

    let passed = checks.iter().all(|check| check.passed);
    let evidence = Evidence {
        schema_version: 1,
        artifact: "aurora-vbap3d-layout-semantics",
        scene: cli.scene.display().to_string(),
        output_channels,
        spatial_speakers: spatial_indices.len(),
        validated_triplets,
        listener_inside_hull,
        anchor_results,
        mirror_results,
        direction_results,
        multi_object,
        checks,
        passed,
        truth_boundary: "Software-only Aurora renderer semantics for the explicit fixture: anchors, bilateral geometry, front/rear centroid, height-energy ordering, unit spatial power, LFE exclusion, and two-object buffer independence. This is not an external-renderer, standardized-layout, acoustic, hardware, or certification claim.",
    };

    if let Some(parent) = cli.output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(&cli.output, serde_json::to_vec_pretty(&evidence)?).with_context(|| format!("write {}", cli.output.display()))?;
    println!(
        "layout_probe passed={} channels={} spatial={} triplets={} anchors={} mirrors={}",
        passed,
        output_channels,
        spatial_indices.len(),
        validated_triplets,
        evidence.anchor_results.len(),
        evidence.mirror_results.len()
    );
    if !passed {
        bail!("3D VBAP layout semantics probe failed one or more gates");
    }
    Ok(())
}

fn render(
    renderer: &mut Vbap3dRenderer,
    listener: &Listener,
    objects: &[RenderObject],
    scratch: &mut RendererScratch,
    speaker_count: usize,
) -> Result<Vec<SpeakerGain>> {
    let mut output = vec![SpeakerGain::default(); speaker_count * objects.len()];
    renderer
        .render_gains(listener, objects, &mut output, scratch)
        .context("render layout semantics probe")?;
    Ok(output)
}

fn dominant_index(gains: &[SpeakerGain]) -> usize {
    gains
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| left.gain.total_cmp(&right.gain))
        .map(|(index, _)| index)
        .unwrap_or(0)
}

fn spatial_power(gains: &[SpeakerGain], spatial_indices: &[usize]) -> f32 {
    spatial_indices
        .iter()
        .map(|&index| gains[index].gain * gains[index].gain)
        .sum()
}

fn lfe_max_abs(gains: &[SpeakerGain], lfe_indices: &[usize]) -> f32 {
    lfe_indices
        .iter()
        .map(|&index| gains[index].gain.abs())
        .fold(0.0_f32, f32::max)
}

fn all_finite_nonnegative(gains: &[SpeakerGain]) -> bool {
    gains.iter().all(|gain| {
        gain.gain.is_finite()
            && gain.delay_samples.is_finite()
            && gain.distance_meters.is_finite()
            && gain.gain >= -ZERO_TOLERANCE
    })
}

fn top_energy(gains: &[SpeakerGain], top_indices: &[usize]) -> f32 {
    top_indices
        .iter()
        .map(|&index| gains[index].gain * gains[index].gain)
        .sum()
}

fn speaker_unit_vectors(speakers: &[Speaker], listener: &Listener) -> Vec<[f32; 3]> {
    let center = Vector3::new(
        listener.position.x,
        listener.position.y,
        listener.position.z + listener.ear_height,
    );
    speakers
        .iter()
        .map(|speaker| {
            let dx = speaker.position.x - center.x;
            let dy = speaker.position.y - center.y;
            let dz = speaker.position.z - center.z;
            let length = (dx * dx + dy * dy + dz * dz).sqrt();
            [dx / length, dy / length, dz / length]
        })
        .collect()
}

fn energy_centroid_y(
    gains: &[SpeakerGain],
    speaker_units: &[[f32; 3]],
    spatial_indices: &[usize],
) -> f32 {
    let power = spatial_power(gains, spatial_indices);
    if power <= 1.0e-12 {
        return 0.0;
    }
    spatial_indices
        .iter()
        .map(|&index| {
            let weight = gains[index].gain * gains[index].gain / power;
            weight * speaker_units[index][1]
        })
        .sum()
}

fn build_mirror_map(speakers: &[Speaker], listener: &Listener) -> Result<Vec<usize>> {
    let mut mapping = Vec::with_capacity(speakers.len());
    for speaker in speakers {
        let target_x = 2.0 * listener.position.x - speaker.position.x;
        let mut matches = speakers.iter().enumerate().filter(|(_, candidate)| {
            (candidate.position.x - target_x).abs() <= 1.0e-4
                && (candidate.position.y - speaker.position.y).abs() <= 1.0e-4
                && (candidate.position.z - speaker.position.z).abs() <= 1.0e-4
        });
        let Some((index, _)) = matches.next() else {
            bail!("no mirror speaker for {}", speaker.id);
        };
        if matches.next().is_some() {
            bail!("ambiguous mirror speaker for {}", speaker.id);
        }
        mapping.push(index);
    }
    Ok(mapping)
}
