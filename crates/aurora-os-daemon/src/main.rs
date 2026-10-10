use std::{
    env,
    fs,
    io::Write,
    path::{Path, PathBuf},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use aurora_modular_speaker_fabric::{
    DeploymentIntent, DeploymentMode, FabricPlan, FabricPlanner, FabricPolicy, ModuleClockSource,
    ModuleRoute, SpeakerModuleState,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
struct AuroraOsConfig {
    control: ControlConfig,
}

#[derive(Debug, Clone, Deserialize)]
struct ControlConfig {
    module_dir: PathBuf,
    intent_path: PathBuf,
    plan_path: PathBuf,
    health_path: PathBuf,
    #[serde(default = "default_poll_ms")]
    poll_millis: u64,
    #[serde(default)]
    policy: PolicyConfig,
}

#[derive(Debug, Clone, Copy, Deserialize)]
struct PolicyConfig {
    #[serde(default = "default_cinema_skew")]
    maximum_cinema_skew_micros: u32,
    #[serde(default = "default_stereo_skew")]
    maximum_stereo_skew_micros: u32,
    #[serde(default = "default_multiroom_skew")]
    maximum_multiroom_skew_micros: u32,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            maximum_cinema_skew_micros: default_cinema_skew(),
            maximum_stereo_skew_micros: default_stereo_skew(),
            maximum_multiroom_skew_micros: default_multiroom_skew(),
        }
    }
}

fn default_poll_ms() -> u64 {
    250
}
fn default_cinema_skew() -> u32 {
    1_000
}
fn default_stereo_skew() -> u32 {
    1_000
}
fn default_multiroom_skew() -> u32 {
    10_000
}

#[derive(Debug, Serialize)]
struct PlanView {
    mode: String,
    assignments: Vec<AssignmentView>,
}

#[derive(Debug, Serialize)]
struct AssignmentView {
    module_id: String,
    route: String,
    clock_source: String,
    roles: Vec<String>,
}

#[derive(Debug, Serialize)]
struct HealthView {
    state: String,
    updated_unix_ms: u128,
    module_count: usize,
    mode: Option<String>,
    assignment_count: usize,
    error: Option<String>,
}

fn mode_name(mode: DeploymentMode) -> &'static str {
    match mode {
        DeploymentMode::Cinema => "cinema",
        DeploymentMode::Stereo => "stereo",
        DeploymentMode::Standalone => "standalone",
        DeploymentMode::Multiroom => "multiroom",
    }
}

fn route_name(route: ModuleRoute) -> &'static str {
    match route {
        ModuleRoute::DockBus => "dock-bus",
        ModuleRoute::WiredNetwork => "wired-network",
        ModuleRoute::WirelessNetwork => "wireless-network",
        ModuleRoute::Muted => "muted",
    }
}

fn clock_name(clock: ModuleClockSource) -> &'static str {
    match clock {
        ModuleClockSource::DockRecovered => "dock-recovered",
        ModuleClockSource::Ptp => "ptp",
        ModuleClockSource::AdaptivePeer => "adaptive-peer",
        ModuleClockSource::Unlocked => "unlocked",
    }
}

fn plan_view(plan: &FabricPlan) -> PlanView {
    PlanView {
        mode: mode_name(plan.mode).to_owned(),
        assignments: plan
            .assignments
            .iter()
            .map(|assignment| AssignmentView {
                module_id: assignment.module_id.clone(),
                route: route_name(assignment.route).to_owned(),
                clock_source: clock_name(assignment.clock_source).to_owned(),
                roles: assignment
                    .roles
                    .iter()
                    .map(|role| format!("{role:?}"))
                    .collect(),
            })
            .collect(),
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn atomic_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path
        .parent()
        .context("output path has no parent directory")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("create {}", parent.display()))?;
    let tmp = path.with_extension("tmp");
    let mut file = fs::File::create(&tmp)
        .with_context(|| format!("create {}", tmp.display()))?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    fs::rename(&tmp, path)
        .with_context(|| format!("replace {}", path.display()))?;
    Ok(())
}

fn load_modules(dir: &Path) -> Result<Vec<SpeakerModuleState>> {
    fs::create_dir_all(dir)
        .with_context(|| format!("create module directory {}", dir.display()))?;
    let mut paths = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|v| v.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    paths.sort();

    let mut modules = Vec::with_capacity(paths.len());
    for path in paths {
        let bytes = fs::read(&path)
            .with_context(|| format!("read module state {}", path.display()))?;
        let module = serde_json::from_slice::<SpeakerModuleState>(&bytes)
            .with_context(|| format!("parse module state {}", path.display()))?;
        modules.push(module);
    }
    Ok(modules)
}

fn load_intent(path: &Path) -> Result<Option<DeploymentIntent>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(
            serde_json::from_slice(&bytes)
                .with_context(|| format!("parse deployment intent {}", path.display()))?,
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
    }
}

fn policy(config: PolicyConfig) -> FabricPolicy {
    FabricPolicy {
        maximum_cinema_skew_micros: config.maximum_cinema_skew_micros,
        maximum_stereo_skew_micros: config.maximum_stereo_skew_micros,
        maximum_multiroom_skew_micros: config.maximum_multiroom_skew_micros,
    }
}

fn reconcile_once(config: &ControlConfig) -> Result<HealthView> {
    let modules = load_modules(&config.module_dir)?;
    let intent = load_intent(&config.intent_path)?;

    let Some(intent) = intent else {
        let _ = fs::remove_file(&config.plan_path);
        return Ok(HealthView {
            state: "waiting-intent".to_owned(),
            updated_unix_ms: now_ms(),
            module_count: modules.len(),
            mode: None,
            assignment_count: 0,
            error: None,
        });
    };

    let planner = FabricPlanner::new(policy(config.policy));
    match planner.plan(&modules, &intent) {
        Ok(plan) => {
            let view = plan_view(&plan);
            atomic_json(&config.plan_path, &view)?;
            Ok(HealthView {
                state: "ready".to_owned(),
                updated_unix_ms: now_ms(),
                module_count: modules.len(),
                mode: Some(view.mode.clone()),
                assignment_count: view.assignments.len(),
                error: None,
            })
        }
        Err(error) => {
            let _ = fs::remove_file(&config.plan_path);
            Ok(HealthView {
                state: "fail-closed".to_owned(),
                updated_unix_ms: now_ms(),
                module_count: modules.len(),
                mode: Some(mode_name(intent.mode).to_owned()),
                assignment_count: 0,
                error: Some(error.to_string()),
            })
        }
    }
}

fn run_once(config: &ControlConfig) -> Result<()> {
    let health = match reconcile_once(config) {
        Ok(health) => health,
        Err(error) => HealthView {
            state: "daemon-error".to_owned(),
            updated_unix_ms: now_ms(),
            module_count: 0,
            mode: None,
            assignment_count: 0,
            error: Some(format!("{error:#}")),
        },
    };
    atomic_json(&config.health_path, &health)?;
    if health.state == "daemon-error" {
        anyhow::bail!(
            "{}",
            health
                .error
                .unwrap_or_else(|| "unknown daemon error".to_owned())
        );
    }
    Ok(())
}

fn usage() {
    eprintln!("usage: aurora-os-daemon [--config PATH] [--once]");
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let mut config_path = PathBuf::from("/etc/aurora/aurora-os.json");
    let mut once = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => {
                config_path = PathBuf::from(args.next().context("--config requires a path")?);
            }
            "--once" => once = true,
            "-h" | "--help" => {
                usage();
                return Ok(());
            }
            other => anyhow::bail!("unknown argument: {other}"),
        }
    }

    let config_bytes = fs::read(&config_path)
        .with_context(|| format!("read {}", config_path.display()))?;
    let config = serde_json::from_slice::<AuroraOsConfig>(&config_bytes)
        .with_context(|| format!("parse {}", config_path.display()))?;

    if once {
        return run_once(&config.control);
    }

    loop {
        if let Err(error) = run_once(&config.control) {
            eprintln!("aurora-os-daemon: {error:#}");
        }
        thread::sleep(Duration::from_millis(config.control.poll_millis.max(50)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::ChannelRole;
    use aurora_modular_speaker_fabric::{
        ModuleAttachment, ModuleSyncStatus, SpeakerModuleCapabilities,
    };

    fn detached(module_id: &str) -> SpeakerModuleState {
        SpeakerModuleState {
            module_id: module_id.to_owned(),
            capabilities: SpeakerModuleCapabilities {
                output_lanes: 2,
                dock_pcm: true,
                wired_network_audio: false,
                wireless_network_audio: true,
                battery_powered: true,
            },
            attachment: ModuleAttachment::Detached,
            sync: ModuleSyncStatus {
                locked: true,
                scheduled_playout: true,
                estimated_skew_micros: Some(250),
                clock_source: ModuleClockSource::Ptp,
            },
            online: true,
        }
    }

    #[test]
    fn detachable_pair_is_admitted_and_serialized() {
        let planner = FabricPlanner::with_defaults();
        let intent = DeploymentIntent {
            mode: DeploymentMode::Cinema,
            targets: vec![
                aurora_modular_speaker_fabric::ModuleTarget {
                    module_id: "left-pod".to_owned(),
                    roles: vec![ChannelRole::SurroundLeft, ChannelRole::TopRearLeft],
                },
                aurora_modular_speaker_fabric::ModuleTarget {
                    module_id: "right-pod".to_owned(),
                    roles: vec![ChannelRole::SurroundRight, ChannelRole::TopRearRight],
                },
            ],
        };
        let plan = planner
            .plan(&[detached("left-pod"), detached("right-pod")], &intent)
            .unwrap();
        let view = plan_view(&plan);
        assert_eq!(view.mode, "cinema");
        assert_eq!(view.assignments.len(), 2);
        assert_eq!(view.assignments[0].route, "wireless-network");
    }

    #[test]
    fn excessive_skew_fails_closed() {
        let mut left = detached("left-pod");
        left.sync.estimated_skew_micros = Some(5_000);
        let planner = FabricPlanner::with_defaults();
        let intent = DeploymentIntent {
            mode: DeploymentMode::Cinema,
            targets: vec![aurora_modular_speaker_fabric::ModuleTarget {
                module_id: "left-pod".to_owned(),
                roles: vec![ChannelRole::SurroundLeft],
            }],
        };
        assert!(planner.plan(&[left], &intent).is_err());
    }
}
