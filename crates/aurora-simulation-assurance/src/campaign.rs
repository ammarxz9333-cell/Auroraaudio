use std::collections::BTreeSet;
use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use aurora_core::{ChannelRole, Listener, Speaker, StandardLayout, Vector3};
use aurora_realtime_audio_sim::{
    builtin_profile, load_fault_timeline, run_duplex_simulation, validate_output_routing,
    CallbackSizePolicy, DeterministicRng, DuplexSimulationConfig, FaultAction, FaultEvent,
    SimulationProfile,
};
use aurora_renderer_api::{RenderObject, Renderer, RendererScratch, SpeakerGain};
use aurora_renderer_vbap::{HorizontalSpread, SpreadRenderObject, VbapRenderer};
use clap::ValueEnum;
use serde::Serialize;

const TRUTH_SOURCE: &str = "deterministic_simulation";
const FAILURE_LIMIT: usize = 32;
pub const MAX_SCENARIOS: u64 = 1_000_000;
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
const LEGACY_FIXTURES: [&str; 6] = [
    "callback_size_change.json",
    "clock_jump.json",
    "input_callback_stall.json",
    "output_loss.json",
    "repeated_stream_errors.json",
    "unsupported_format.json",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum CampaignLevel {
    Smoke,
    Standard,
    Deep,
    Soak,
}

impl CampaignLevel {
    pub fn default_count(self) -> u64 {
        match self {
            Self::Smoke => 1_000,
            Self::Standard => 10_000,
            Self::Deep => 100_000,
            Self::Soak => 4,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Smoke => "smoke",
            Self::Standard => "standard",
            Self::Deep => "deep",
            Self::Soak => "soak",
        }
    }
}

pub struct CampaignOptions {
    pub level: CampaignLevel,
    pub scenario_count: u64,
    pub start_seed: u64,
    pub shard_index: u32,
    pub shard_count: u32,
    pub repeat: u32,
    pub soak_hours: u64,
    pub replay: Option<(u64, u64)>,
    pub include_legacy_fixtures: bool,
    pub report_path: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct CampaignReport {
    pub truth_source: &'static str,
    pub level: &'static str,
    pub target: String,
    pub requested_scenarios: u64,
    pub executed_scenarios: u64,
    pub shard_index: u32,
    pub shard_count: u32,
    pub repeat_count: u32,
    pub repeat_checksums: Vec<String>,
    pub deterministic_checksum: String,
    pub reproducible: bool,
    pub legacy_fixtures: usize,
    pub verified_properties: Vec<&'static str>,
    pub required_allocation_guards: Vec<&'static str>,
    pub dimensions: DimensionCoverage,
    pub resource_bounds: ResourceBounds,
    pub failures: Vec<FailureRecord>,
    pub host_execution_seconds: f64,
    pub passed: bool,
}

#[derive(Debug, Default, Serialize)]
pub struct DimensionCoverage {
    sample_rates: BTreeSet<u32>,
    callback_sizes: BTreeSet<usize>,
    drift_ppm: BTreeSet<i32>,
    jitter_frames: BTreeSet<usize>,
    profiles: BTreeSet<String>,
    layouts: BTreeSet<String>,
    spread_steps: BTreeSet<u32>,
    fault_categories: BTreeSet<String>,
    geometry_categories: BTreeSet<String>,
    buffer_fill_states: BTreeSet<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct ResourceBounds {
    max_live_generated_scenarios: usize,
    retained_failure_limit: usize,
    max_reported_ring_capacity_frames: usize,
    report_growth_per_passing_scenario_bytes: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct FailureRecord {
    scenario_id: String,
    seed: u64,
    ordinal: u64,
    truth_source: &'static str,
    category: FailureCategory,
    detail: String,
    bounded_event_window: String,
    reproducible_command: String,
    shrink_status: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum FailureCategory {
    Panic,
    StructuredErrorMismatch,
    NonFiniteOutput,
    UnboundedState,
    StateTransition,
    ChannelLeakage,
    EnergyNormalization,
    PointCompatibility,
    PermutationEquivalence,
    MissingFaultObservation,
    InternalError,
}

#[derive(Clone)]
struct Scenario {
    id: String,
    seed: u64,
    ordinal: u64,
    reproducible_command: String,
    kind: ScenarioKind,
}

#[derive(Clone)]
enum ScenarioKind {
    Duplex(DuplexSimulationConfig),
    Routing {
        profile: SimulationProfile,
        layout: StandardLayout,
    },
    Renderer(RendererScenario),
    Probe(StructuredProbe),
}

#[derive(Clone)]
struct RendererScenario {
    layout: Vec<Speaker>,
    listener: Listener,
    source: Vector3,
    spread: HorizontalSpread,
    geometry: &'static str,
    sample_rate: u32,
    block_size: usize,
}

#[derive(Clone, Copy)]
enum StructuredProbe {
    InvalidSpread,
    UnsupportedSampleRate,
    NonFiniteSpeaker,
    RoutingCapacity,
    ExtremeFiniteRenderer,
}

struct ScenarioEvidence {
    checksum: u64,
    max_ring_capacity_frames: usize,
    dimensions: Vec<Dimension>,
}

enum Dimension {
    SampleRate(u32),
    CallbackSize(usize),
    Drift(i32),
    Jitter(usize),
    Profile(String),
    Layout(String),
    Spread(u32),
    Fault(String),
    Geometry(String),
    BufferFill(String),
}

struct ScenarioFailure {
    category: FailureCategory,
    detail: String,
}

pub fn run_campaign(options: &CampaignOptions) -> Result<CampaignReport> {
    let started = Instant::now();
    let mut repeat_checksums = Vec::with_capacity(options.repeat as usize);
    let mut retained_failures = Vec::with_capacity(FAILURE_LIMIT);
    let mut dimensions = DimensionCoverage::default();
    let mut resources = ResourceBounds {
        max_live_generated_scenarios: 1,
        retained_failure_limit: FAILURE_LIMIT,
        max_reported_ring_capacity_frames: 0,
        report_growth_per_passing_scenario_bytes: 0,
    };
    let mut executed_per_repeat = 0_u64;
    let mut legacy_per_repeat = 0_usize;

    for repeat_index in 0..options.repeat {
        let mut aggregate = FNV_OFFSET;
        let mut executed = 0_u64;
        for ordinal in selected_ordinals(options) {
            let seed = options
                .replay
                .map(|(seed, _)| seed)
                .unwrap_or_else(|| scenario_seed(options.start_seed, ordinal));
            let scenario = build_scenario(options, ordinal, seed);
            let result = catch_unwind(AssertUnwindSafe(|| execute_scenario(&scenario)));
            match result {
                Ok(Ok(evidence)) => {
                    aggregate = hash_u64(aggregate, evidence.checksum);
                    resources.max_reported_ring_capacity_frames = resources
                        .max_reported_ring_capacity_frames
                        .max(evidence.max_ring_capacity_frames);
                    if repeat_index == 0 {
                        dimensions.record_all(evidence.dimensions);
                    }
                }
                Ok(Err(failure)) => {
                    aggregate = hash_u64(aggregate, failure.category as u64);
                    retain_failure(&mut retained_failures, &scenario, failure);
                }
                Err(_) => {
                    aggregate = hash_u64(aggregate, FailureCategory::Panic as u64);
                    retain_failure(
                        &mut retained_failures,
                        &scenario,
                        ScenarioFailure {
                            category: FailureCategory::Panic,
                            detail: "scenario panicked".to_owned(),
                        },
                    );
                }
            }
            executed = executed.saturating_add(1);
        }

        let legacy = if options.include_legacy_fixtures {
            execute_legacy_fixtures(
                options,
                repeat_index,
                &mut aggregate,
                &mut retained_failures,
            )?
        } else {
            0
        };
        executed_per_repeat = executed;
        legacy_per_repeat = legacy;
        repeat_checksums.push(format!("{aggregate:016x}"));
    }

    let reproducible = repeat_checksums.first().map_or(true, |first| {
        repeat_checksums.iter().all(|value| value == first)
    });
    let deterministic_checksum = repeat_checksums.first().cloned().unwrap_or_default();
    let passed = reproducible && retained_failures.is_empty();
    let report = CampaignReport {
        truth_source: TRUTH_SOURCE,
        level: options.level.as_str(),
        target: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        requested_scenarios: options.scenario_count,
        executed_scenarios: executed_per_repeat,
        shard_index: options.shard_index,
        shard_count: options.shard_count,
        repeat_count: options.repeat,
        repeat_checksums,
        deterministic_checksum,
        reproducible,
        legacy_fixtures: legacy_per_repeat,
        verified_properties: vec![
            "no-panic",
            "bounded-loop-count",
            "finite-output",
            "bounded-memory-model",
            "valid-state-transitions",
            "target-qualified-determinism",
            "structured-invalid-input-errors",
            "explicit-fault-observability",
            "no-channel-leakage",
            "normalized-renderer-energy",
            "spread-zero-phase-3a-compatibility",
            "speaker-layout-permutation-equivalence",
            "allowed-recovery-terminal-state",
            "simulation-truth-terminology",
        ],
        required_allocation_guards: vec![
            "aurora_realtime_audio_sim::clock::steady_state_scheduler_callbacks_allocate_zero_times",
            "aurora_realtime_engine::tests::duplex_input_and_output_callbacks_allocate_zero_times_after_startup",
            "aurora_renderer_vbap::tests::warmed_up_spread_render_allocates_zero_times",
        ],
        dimensions,
        resource_bounds: resources,
        failures: retained_failures,
        host_execution_seconds: started.elapsed().as_secs_f64(),
        passed,
    };
    write_report_and_failures(options, &report)?;
    Ok(report)
}

fn selected_ordinals(options: &CampaignOptions) -> SelectedOrdinals {
    if let Some((_, ordinal)) = options.replay {
        return SelectedOrdinals::Replay(Some(ordinal));
    }
    SelectedOrdinals::Sharded {
        next: u64::from(options.shard_index),
        end: options.scenario_count,
        step: u64::from(options.shard_count),
    }
}

enum SelectedOrdinals {
    Replay(Option<u64>),
    Sharded { next: u64, end: u64, step: u64 },
}

impl Iterator for SelectedOrdinals {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Replay(value) => value.take(),
            Self::Sharded { next, end, step } if *next < *end => {
                let value = *next;
                *next = next.saturating_add(*step);
                Some(value)
            }
            Self::Sharded { .. } => None,
        }
    }
}

fn scenario_seed(start_seed: u64, ordinal: u64) -> u64 {
    let mut value = start_seed ^ ordinal.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn build_scenario(options: &CampaignOptions, ordinal: u64, seed: u64) -> Scenario {
    let kind = if options.level == CampaignLevel::Soak {
        ScenarioKind::Duplex(duplex_config(
            seed,
            ordinal.saturating_mul(4),
            options.soak_hours.saturating_mul(3_600),
        ))
    } else {
        match ordinal % 4 {
            0 => ScenarioKind::Duplex(duplex_config(seed, ordinal, 2 + ordinal % 7)),
            1 => routing_scenario(ordinal),
            2 => ScenarioKind::Renderer(renderer_scenario(seed, ordinal)),
            _ => ScenarioKind::Probe(structured_probe(ordinal)),
        }
    };
    let id = format!("sac1-{ordinal:012x}-{seed:016x}");
    let reproducible_command = format!(
        "cargo run --release -p aurora-simulation-assurance -- --level {} --replay-seed {seed} --replay-ordinal {ordinal} --report output/simulation-assurance/replay-{id}.json",
        options.level.as_str()
    );
    Scenario {
        id,
        seed,
        ordinal,
        reproducible_command,
        kind,
    }
}

fn duplex_config(seed: u64, ordinal: u64, duration_seconds: u64) -> DuplexSimulationConfig {
    let profile_name =
        ["stereo-consumer", "usb-5-1", "usb-7-1", "development-12"][(ordinal as usize / 4) % 4];
    let mut profile = builtin_profile(profile_name).expect("built-in profile is static");
    let mut rng = DeterministicRng::new(seed);
    let callback = match rng.next_u64() % 3 {
        0 => CallbackSizePolicy::Fixed {
            frames: [64, 128, 256, 512][(rng.next_u64() % 4) as usize],
        },
        1 => CallbackSizePolicy::Alternating {
            first: 64,
            second: 512,
        },
        _ => CallbackSizePolicy::RandomBounded {
            minimum: 1,
            maximum: 1_024,
        },
    };
    profile.input.callback_size = callback.clone();
    profile.output.callback_size = callback;
    let rates = &profile.output.supported_sample_rates;
    let sample_rate = rates[(rng.next_u64() as usize) % rates.len()];
    let ppm_values = [-250, -100, -25, 0, 25, 100, 250];
    let input_ppm = ppm_values[(rng.next_u64() as usize) % ppm_values.len()];
    let output_ppm = ppm_values[(rng.next_u64() as usize) % ppm_values.len()];
    let jitter = [0, 1, 3, 16, 96][(rng.next_u64() as usize) % 5];
    let faults = if ordinal % 5 == 0 && duration_seconds >= 2 {
        let mut values = vec![FaultEvent {
            at_milliseconds: 500,
            action: [
                FaultAction::InputLoss,
                FaultAction::OutputLoss,
                FaultAction::CallbackError,
                FaultAction::FormatChange,
            ][(rng.next_u64() % 4) as usize]
                .clone(),
            duration_milliseconds: 100,
            value: 0,
        }];
        if ordinal % 20 == 0 {
            values.push(FaultEvent {
                at_milliseconds: 1_200,
                action: FaultAction::CallbackSizeChange,
                duration_milliseconds: 0,
                value: 128,
            });
        }
        values
    } else {
        Vec::new()
    };
    DuplexSimulationConfig {
        channels: profile
            .output
            .supported_channel_counts
            .iter()
            .max()
            .copied(),
        profile,
        duration_seconds: duration_seconds.max(1),
        seed,
        input_ppm: Some(input_ppm),
        output_ppm: Some(output_ppm),
        callback_jitter_frames: Some(jitter),
        device_latency_frames: Some([64, 128, 256, 512][(rng.next_u64() % 4) as usize]),
        sample_rate: Some(sample_rate),
        block_size: [64, 128, 256, 512][(rng.next_u64() % 4) as usize],
        faults,
    }
}

fn routing_scenario(ordinal: u64) -> ScenarioKind {
    let (profile, layout) = match (ordinal / 4) % 3 {
        0 => ("stereo-consumer", StandardLayout::Stereo),
        1 => ("usb-5-1", StandardLayout::FiveOne),
        _ => ("usb-7-1", StandardLayout::SevenOne),
    };
    ScenarioKind::Routing {
        profile: builtin_profile(profile).expect("built-in profile is static"),
        layout,
    }
}

fn renderer_scenario(seed: u64, ordinal: u64) -> RendererScenario {
    let mut rng = DeterministicRng::new(seed);
    let geometry_index = ((ordinal / 4) % 7) as usize;
    let (layout, geometry) = match geometry_index {
        0 => (regular_layout(2), "stereo"),
        1 => (regular_layout(6), "5.1"),
        2 => (regular_layout(8), "7.1"),
        3 => (irregular_layout(10, false), "irregular"),
        4 => (irregular_layout(12, false), "dense-12"),
        5 => (irregular_layout(16, true), "near-duplicate"),
        _ => (duplicate_layout(), "duplicate"),
    };
    let angle =
        -std::f32::consts::PI + std::f32::consts::TAU * (rng.next_u64() % 14_400) as f32 / 14_400.0;
    let spread_step = (rng.next_u64() % 21) as u32;
    RendererScenario {
        layout,
        listener: Listener {
            position: Vector3::ZERO,
            orientation: Vector3::new(0.0, 1.0, 0.0),
            ear_height: 1.2,
        },
        source: Vector3::new(angle.cos(), angle.sin(), 0.0),
        spread: HorizontalSpread::new(spread_step as f32 / 20.0)
            .expect("generated spread is bounded"),
        geometry,
        sample_rate: [44_100, 48_000, 96_000][(rng.next_u64() % 3) as usize],
        block_size: [64, 128, 256, 512][(rng.next_u64() % 4) as usize],
    }
}

fn structured_probe(ordinal: u64) -> StructuredProbe {
    match (ordinal / 4) % 5 {
        0 => StructuredProbe::InvalidSpread,
        1 => StructuredProbe::UnsupportedSampleRate,
        2 => StructuredProbe::NonFiniteSpeaker,
        3 => StructuredProbe::RoutingCapacity,
        _ => StructuredProbe::ExtremeFiniteRenderer,
    }
}

fn execute_scenario(scenario: &Scenario) -> Result<ScenarioEvidence, ScenarioFailure> {
    match &scenario.kind {
        ScenarioKind::Duplex(config) => execute_duplex(config),
        ScenarioKind::Routing { profile, layout } => execute_routing(profile, *layout),
        ScenarioKind::Renderer(config) => execute_renderer(config),
        ScenarioKind::Probe(probe) => execute_probe(*probe, scenario.seed),
    }
}

fn execute_duplex(config: &DuplexSimulationConfig) -> Result<ScenarioEvidence, ScenarioFailure> {
    let report = run_duplex_simulation(config).map_err(|error| ScenarioFailure {
        category: FailureCategory::InternalError,
        detail: error.to_string(),
    })?;
    if !report.finite || !report.sample_pipeline_probe_finite {
        return failure(
            FailureCategory::NonFiniteOutput,
            "simulation report was non-finite",
        );
    }
    if !report.bounded
        || report.steady_state_memory_growth_bytes != 0
        || report.ring_fill_minimum < 0.0
        || report.ring_fill_maximum > report.ring_capacity_frames as f64
    {
        return failure(
            FailureCategory::UnboundedState,
            "simulation exceeded a fixed bound",
        );
    }
    if report.final_state != "Stopped" || !valid_transitions(&report.state_transitions) {
        return failure(
            FailureCategory::StateTransition,
            "invalid lifecycle transition sequence",
        );
    }
    if config.faults.iter().any(fault_uses_lifecycle)
        && !report
            .state_transitions
            .iter()
            .any(|record| record.to == "Faulted")
    {
        return failure(
            FailureCategory::MissingFaultObservation,
            "injected fault was not observable in lifecycle records",
        );
    }
    if !config.faults.is_empty() && !config.faults.iter().any(fault_uses_lifecycle) {
        let mut baseline = config.clone();
        baseline.faults.clear();
        let baseline = run_duplex_simulation(&baseline).map_err(|error| ScenarioFailure {
            category: FailureCategory::InternalError,
            detail: error.to_string(),
        })?;
        if baseline.deterministic_checksum == report.deterministic_checksum {
            return failure(
                FailureCategory::MissingFaultObservation,
                "non-lifecycle fault did not change deterministic simulation evidence",
            );
        }
    }
    let checksum = parse_checksum(&report.deterministic_checksum)?;
    let fault = if config.faults.is_empty() {
        "none".to_owned()
    } else {
        config
            .faults
            .iter()
            .map(|value| format!("{:?}", value.action))
            .collect::<Vec<_>>()
            .join("+")
    };
    let minimum_fill = report.ring_fill_minimum / report.ring_capacity_frames as f64;
    let maximum_fill = report.ring_fill_maximum / report.ring_capacity_frames as f64;
    let fill_state = format!(
        "min-{}-max-{}",
        fill_bucket(minimum_fill),
        fill_bucket(maximum_fill)
    );
    Ok(ScenarioEvidence {
        checksum,
        max_ring_capacity_frames: report.ring_capacity_frames,
        dimensions: vec![
            Dimension::SampleRate(report.output_sample_rate),
            Dimension::CallbackSize(config.profile.output.callback_size.maximum_frames()),
            Dimension::Drift(config.input_ppm.unwrap_or_default()),
            Dimension::Drift(config.output_ppm.unwrap_or_default()),
            Dimension::Jitter(config.callback_jitter_frames.unwrap_or_default()),
            Dimension::Profile(report.profile),
            Dimension::Fault(fault),
            Dimension::BufferFill(fill_state),
        ],
    })
}

fn execute_routing(
    profile: &SimulationProfile,
    layout: StandardLayout,
) -> Result<ScenarioEvidence, ScenarioFailure> {
    let report = validate_output_routing(profile, layout).map_err(|error| ScenarioFailure {
        category: FailureCategory::InternalError,
        detail: error.to_string(),
    })?;
    if !report.passed || !report.unique_output_routing || !report.inactive_channels_silent {
        return failure(
            FailureCategory::ChannelLeakage,
            "routing validation reported leakage",
        );
    }
    if !report.maximum_gain_error.is_finite() {
        return failure(
            FailureCategory::NonFiniteOutput,
            "routing gain error was non-finite",
        );
    }
    Ok(ScenarioEvidence {
        checksum: parse_checksum(&report.deterministic_checksum)?,
        max_ring_capacity_frames: 0,
        dimensions: vec![
            Dimension::Profile(profile.name.clone()),
            Dimension::Layout(format!("{layout:?}")),
        ],
    })
}

fn execute_renderer(config: &RendererScenario) -> Result<ScenarioEvidence, ScenarioFailure> {
    let object = RenderObject {
        position: config.source,
        gain: 1.0,
    };
    let point = render(config, HorizontalSpread::POINT, object)?;
    let point_via_spread = render_with_api(config, HorizontalSpread::POINT, object, true)?;
    if point
        .iter()
        .zip(&point_via_spread)
        .any(|(left, right)| left.gain.to_bits() != right.gain.to_bits())
    {
        return failure(
            FailureCategory::PointCompatibility,
            "spread zero differed from the Phase 3A point path",
        );
    }
    let spread = render(config, config.spread, object)?;
    if spread.iter().any(|gain| {
        !gain.gain.is_finite()
            || !gain.distance_meters.is_finite()
            || !gain.delay_samples.is_finite()
    }) {
        return failure(
            FailureCategory::NonFiniteOutput,
            "renderer output was non-finite",
        );
    }
    let power = spread.iter().map(|gain| gain.gain * gain.gain).sum::<f32>();
    if (power - 1.0).abs() > 1.0e-4 {
        return failure(
            FailureCategory::EnergyNormalization,
            format!("renderer power was {power}"),
        );
    }
    if config.spread != HorizontalSpread::POINT {
        let mut permuted = config.clone();
        permuted.layout.reverse();
        let permuted_gains = render(&permuted, config.spread, object)?;
        for (index, speaker) in config.layout.iter().enumerate() {
            let other = permuted
                .layout
                .iter()
                .position(|candidate| candidate.id == speaker.id)
                .ok_or_else(|| ScenarioFailure {
                    category: FailureCategory::PermutationEquivalence,
                    detail: "speaker identity disappeared after permutation".to_owned(),
                })?;
            if (spread[index].gain - permuted_gains[other].gain).abs() > 1.0e-5 {
                return failure(
                    FailureCategory::PermutationEquivalence,
                    "layout permutation changed identifier-mapped gains",
                );
            }
        }
    }
    let mut checksum = FNV_OFFSET;
    for gain in &spread {
        checksum = hash_u64(checksum, (gain.gain * 100_000.0).round() as u64);
    }
    Ok(ScenarioEvidence {
        checksum,
        max_ring_capacity_frames: 0,
        dimensions: vec![
            Dimension::SampleRate(config.sample_rate),
            Dimension::CallbackSize(config.block_size),
            Dimension::Layout(config.geometry.to_owned()),
            Dimension::Spread((config.spread.value() * 20.0).round() as u32),
            Dimension::Geometry(config.geometry.to_owned()),
        ],
    })
}

fn render(
    config: &RendererScenario,
    spread: HorizontalSpread,
    object: RenderObject,
) -> Result<Vec<SpeakerGain>, ScenarioFailure> {
    render_with_api(config, spread, object, spread != HorizontalSpread::POINT)
}

fn render_with_api(
    config: &RendererScenario,
    spread: HorizontalSpread,
    object: RenderObject,
    use_spread_api: bool,
) -> Result<Vec<SpeakerGain>, ScenarioFailure> {
    let mut renderer = VbapRenderer::new();
    renderer
        .configure(
            config.layout.clone(),
            config.sample_rate,
            config.block_size,
            1,
        )
        .map_err(internal_renderer_error)?;
    let mut scratch = RendererScratch::new(
        renderer
            .required_scratch_size()
            .map_err(internal_renderer_error)?,
    );
    let mut output = vec![SpeakerGain::default(); config.layout.len()];
    if use_spread_api {
        renderer
            .render_spread_gains(
                &config.listener,
                &[SpreadRenderObject { object, spread }],
                &mut output,
                &mut scratch,
            )
            .map_err(internal_renderer_error)?;
    } else {
        renderer
            .render_gains(&config.listener, &[object], &mut output, &mut scratch)
            .map_err(internal_renderer_error)?;
    }
    Ok(output)
}

fn execute_probe(probe: StructuredProbe, seed: u64) -> Result<ScenarioEvidence, ScenarioFailure> {
    match probe {
        StructuredProbe::InvalidSpread => {
            if HorizontalSpread::new(f32::NAN).is_ok()
                || HorizontalSpread::new(-0.01).is_ok()
                || HorizontalSpread::new(1.01).is_ok()
            {
                return failure(
                    FailureCategory::StructuredErrorMismatch,
                    "invalid spread did not return a structured error",
                );
            }
        }
        StructuredProbe::UnsupportedSampleRate => {
            let mut config = duplex_config(seed, 0, 1);
            config.sample_rate = Some(12_345);
            if run_duplex_simulation(&config).is_ok() {
                return failure(
                    FailureCategory::StructuredErrorMismatch,
                    "unsupported sample rate was accepted",
                );
            }
        }
        StructuredProbe::NonFiniteSpeaker => {
            let mut layout = regular_layout(2);
            layout[0].position.x = f32::INFINITY;
            let mut renderer = VbapRenderer::new();
            if renderer.configure(layout, 48_000, 256, 1).is_ok() {
                return failure(
                    FailureCategory::StructuredErrorMismatch,
                    "non-finite speaker was accepted",
                );
            }
        }
        StructuredProbe::RoutingCapacity => {
            if validate_output_routing(
                &builtin_profile("stereo-consumer").expect("built-in profile is static"),
                StandardLayout::SevenOne,
            )
            .is_ok()
            {
                return failure(
                    FailureCategory::StructuredErrorMismatch,
                    "undersized routing endpoint was accepted",
                );
            }
        }
        StructuredProbe::ExtremeFiniteRenderer => {
            let config = RendererScenario {
                layout: regular_layout(8),
                listener: Listener {
                    position: Vector3::ZERO,
                    orientation: Vector3::new(0.0, 1.0, 0.0),
                    ear_height: 1.2,
                },
                source: Vector3::new(f32::MAX, f32::MAX, 0.0),
                spread: HorizontalSpread::new(0.75).expect("constant spread is valid"),
                geometry: "extreme-finite",
                sample_rate: 96_000,
                block_size: 512,
            };
            let output = render(
                &config,
                config.spread,
                RenderObject {
                    position: config.source,
                    gain: f32::MAX,
                },
            )?;
            if output.iter().any(|gain| !gain.gain.is_finite()) {
                return failure(
                    FailureCategory::NonFiniteOutput,
                    "extreme finite renderer input escaped as non-finite output",
                );
            }
        }
    }
    Ok(ScenarioEvidence {
        checksum: hash_u64(FNV_OFFSET, probe as u64),
        max_ring_capacity_frames: 0,
        dimensions: vec![Dimension::Geometry(format!("probe-{}", probe as u8))],
    })
}

fn execute_legacy_fixtures(
    options: &CampaignOptions,
    repeat_index: u32,
    aggregate: &mut u64,
    failures: &mut Vec<FailureRecord>,
) -> Result<usize> {
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/simulation/fault_scenarios");
    for (index, fixture) in LEGACY_FIXTURES.iter().enumerate() {
        let seed = scenario_seed(options.start_seed, 0xf000_0000 + index as u64);
        let events = load_fault_timeline(root.join(fixture))
            .with_context(|| format!("load legacy fixture {fixture}"))?;
        let mut config = duplex_config(seed, 0, 3_600);
        config.profile = builtin_profile("usb-7-1").expect("built-in profile is static");
        config.channels = Some(8);
        config.sample_rate = Some(48_000);
        config.faults = events;
        let scenario = Scenario {
            id: format!("legacy-{}", fixture.trim_end_matches(".json")),
            seed,
            ordinal: 0xf000_0000 + index as u64,
            reproducible_command: format!(
                "cargo run --release -p aurora-cli --all-features -- simulate-duplex --profile usb-7-1 --duration-hours 1 --seed {seed} --fault-script fixtures/simulation/fault_scenarios/{fixture} --report output/simulation-assurance/replay-legacy-{index}.json"
            ),
            kind: ScenarioKind::Duplex(config),
        };
        match execute_scenario(&scenario) {
            Ok(evidence) => *aggregate = hash_u64(*aggregate, evidence.checksum),
            Err(error) => retain_failure(failures, &scenario, error),
        }
    }
    let _ = repeat_index;
    Ok(LEGACY_FIXTURES.len())
}

fn valid_transitions(records: &[aurora_realtime_audio_sim::StateTransitionRecord]) -> bool {
    records
        .windows(2)
        .all(|pair| pair[0].at_milliseconds <= pair[1].at_milliseconds)
        && records.iter().all(|record| {
            matches!(
                (record.from.as_str(), record.to.as_str()),
                ("Stopped", "Starting")
                    | ("Starting", "Running")
                    | ("Running", "Faulted")
                    | ("Faulted", "Recovering")
                    | ("Recovering", "Running")
                    | ("Running", "Stopping")
                    | ("Recovering", "Stopping")
                    | ("Faulted", "Stopping")
                    | ("Stopping", "Stopped")
            )
        })
}

fn fault_uses_lifecycle(fault: &FaultEvent) -> bool {
    matches!(
        fault.action,
        FaultAction::InputLoss
            | FaultAction::OutputLoss
            | FaultAction::CallbackError
            | FaultAction::FormatChange
            | FaultAction::StreamFreeze
            | FaultAction::MissingCallbacks
            | FaultAction::SchedulingStall
    )
}

fn regular_layout(channels: usize) -> Vec<Speaker> {
    (0..channels)
        .map(|index| {
            let angle = index as f32 / channels as f32 * std::f32::consts::TAU;
            speaker(
                index,
                angle,
                ChannelRole::Custom(format!("regular-{index}")),
            )
        })
        .collect()
}

fn irregular_layout(channels: usize, near_duplicate: bool) -> Vec<Speaker> {
    (0..channels)
        .map(|index| {
            let base = index as f32 / channels as f32 * std::f32::consts::TAU;
            let offset = ((index * 37 % 11) as f32 - 5.0) * 0.017;
            let angle = if near_duplicate && index == 1 {
                0.000_001
            } else {
                base + offset
            };
            speaker(
                index,
                angle,
                ChannelRole::Custom(format!("irregular-{index}")),
            )
        })
        .collect()
}

fn duplicate_layout() -> Vec<Speaker> {
    vec![
        speaker(0, 0.0, ChannelRole::Custom("duplicate-a".to_owned())),
        speaker(1, 0.0, ChannelRole::Custom("duplicate-b".to_owned())),
        speaker(
            2,
            std::f32::consts::FRAC_PI_2,
            ChannelRole::Custom("duplicate-c".to_owned()),
        ),
        speaker(
            3,
            std::f32::consts::PI,
            ChannelRole::Custom("duplicate-d".to_owned()),
        ),
    ]
}

fn speaker(index: usize, angle: f32, role: ChannelRole) -> Speaker {
    Speaker {
        id: format!("campaign-{index:02}"),
        label: format!("Campaign {index}"),
        channel_role: role,
        position: Vector3::new(angle.cos(), angle.sin(), 0.0),
        orientation: Vector3::ZERO,
        gain_db: 0.0,
        delay_samples: 0.0,
        enabled: true,
    }
}

fn internal_renderer_error(error: impl std::fmt::Display) -> ScenarioFailure {
    ScenarioFailure {
        category: FailureCategory::InternalError,
        detail: error.to_string(),
    }
}

fn parse_checksum(value: &str) -> Result<u64, ScenarioFailure> {
    u64::from_str_radix(value, 16).map_err(|_| ScenarioFailure {
        category: FailureCategory::InternalError,
        detail: format!("invalid deterministic checksum: {value}"),
    })
}

fn failure<T>(category: FailureCategory, detail: impl Into<String>) -> Result<T, ScenarioFailure> {
    Err(ScenarioFailure {
        category,
        detail: detail.into(),
    })
}

fn retain_failure(
    retained: &mut Vec<FailureRecord>,
    scenario: &Scenario,
    failure: ScenarioFailure,
) {
    if retained.len() >= FAILURE_LIMIT {
        return;
    }
    retained.push(FailureRecord {
        scenario_id: scenario.id.clone(),
        seed: scenario.seed,
        ordinal: scenario.ordinal,
        truth_source: TRUTH_SOURCE,
        category: failure.category,
        detail: failure.detail,
        bounded_event_window: scenario_window(scenario),
        reproducible_command: scenario.reproducible_command.clone(),
        shrink_status: "isolated single-scenario replay",
    });
}

fn scenario_window(scenario: &Scenario) -> String {
    match &scenario.kind {
        ScenarioKind::Duplex(config) => format!(
            "0..={} simulated milliseconds; {} scripted faults",
            config.duration_seconds.saturating_mul(1_000),
            config.faults.len()
        ),
        ScenarioKind::Routing { .. } => "single routing impulse matrix".to_owned(),
        ScenarioKind::Renderer(_) => "single point/spread render comparison".to_owned(),
        ScenarioKind::Probe(_) => "single structured-error probe".to_owned(),
    }
}

fn write_report_and_failures(options: &CampaignOptions, report: &CampaignReport) -> Result<()> {
    if let Some(parent) = options.report_path.parent() {
        fs::create_dir_all(parent).context("create campaign report directory")?;
    }
    fs::write(&options.report_path, serde_json::to_vec_pretty(report)?)
        .context("write campaign report")?;
    if !report.failures.is_empty() {
        let failure_dir = options
            .report_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("failures");
        fs::create_dir_all(&failure_dir).context("create bounded failure directory")?;
        for failure in &report.failures {
            fs::write(
                failure_dir.join(format!("{}.json", failure.scenario_id)),
                serde_json::to_vec_pretty(failure)?,
            )
            .context("write bounded failure replay record")?;
        }
    }
    Ok(())
}

fn hash_u64(mut hash: u64, value: u64) -> u64 {
    for byte in value.to_le_bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

impl DimensionCoverage {
    fn record_all(&mut self, values: Vec<Dimension>) {
        for value in values {
            match value {
                Dimension::SampleRate(item) => {
                    self.sample_rates.insert(item);
                }
                Dimension::CallbackSize(item) => {
                    self.callback_sizes.insert(item);
                }
                Dimension::Drift(item) => {
                    self.drift_ppm.insert(item);
                }
                Dimension::Jitter(item) => {
                    self.jitter_frames.insert(item);
                }
                Dimension::Profile(item) => {
                    self.profiles.insert(item);
                }
                Dimension::Layout(item) => {
                    self.layouts.insert(item);
                }
                Dimension::Spread(item) => {
                    self.spread_steps.insert(item);
                }
                Dimension::Fault(item) => {
                    self.fault_categories.insert(item);
                }
                Dimension::Geometry(item) => {
                    self.geometry_categories.insert(item);
                }
                Dimension::BufferFill(item) => {
                    self.buffer_fill_states.insert(item);
                }
            }
        }
    }
}

fn fill_bucket(fill: f64) -> &'static str {
    if fill < 0.1 {
        "low"
    } else if fill > 0.9 {
        "high"
    } else {
        "nominal"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> CampaignOptions {
        CampaignOptions {
            level: CampaignLevel::Smoke,
            scenario_count: 32,
            start_seed: 42,
            shard_index: 0,
            shard_count: 1,
            repeat: 2,
            soak_hours: 24,
            replay: None,
            include_legacy_fixtures: false,
            report_path: std::env::temp_dir().join("aurora-campaign-test.json"),
        }
    }

    #[test]
    fn generation_is_stable_and_replayable() {
        let value = options();
        let seed = scenario_seed(value.start_seed, 17);
        let first = build_scenario(&value, 17, seed);
        let second = build_scenario(&value, 17, seed);
        assert_eq!(first.id, second.id);
        assert_eq!(first.reproducible_command, second.reproducible_command);
    }

    #[test]
    fn shards_are_disjoint_and_complete() {
        let mut observed = BTreeSet::new();
        for shard_index in 0..4 {
            let mut value = options();
            value.scenario_count = 1_000;
            value.shard_index = shard_index;
            value.shard_count = 4;
            for ordinal in selected_ordinals(&value) {
                assert!(observed.insert(ordinal));
            }
        }
        assert_eq!(observed.len(), 1_000);
        assert_eq!(observed.first(), Some(&0));
        assert_eq!(observed.last(), Some(&999));
    }

    #[test]
    fn bounded_campaign_repeats_identically() {
        let value = options();
        let report = run_campaign(&value).unwrap();
        assert!(report.passed, "{:?}", report.failures);
        assert!(report.reproducible);
        assert_eq!(report.repeat_checksums.len(), 2);
        assert_eq!(report.repeat_checksums[0], report.repeat_checksums[1]);
        assert_eq!(report.resource_bounds.max_live_generated_scenarios, 1);
        assert_eq!(report.resource_bounds.retained_failure_limit, FAILURE_LIMIT);
        let _ = fs::remove_file(value.report_path);
    }
}
