use std::collections::BTreeSet;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

const REGISTRY_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../config/product-capabilities.json"
));

#[derive(Debug, Deserialize)]
struct Registry {
    schema_version: u32,
    product_baseline: String,
    experience_target: String,
    claim_policy: ClaimPolicy,
    primary_hardware_lane: HardwareLane,
    capabilities: Vec<Capability>,
}

#[derive(Debug, Deserialize)]
struct ClaimPolicy {
    samsung_equivalence_claimed: bool,
    dolby_atmos_compatibility_claimed: bool,
    physical_acoustic_parity_claimed: bool,
    rule: String,
}

#[derive(Debug, Deserialize)]
struct HardwareLane {
    platform: String,
    status: String,
    fallback_reference_platform: String,
    selection_rule: String,
}

#[derive(Debug, Deserialize)]
struct Capability {
    id: String,
    status: String,
    scope: String,
    #[serde(default)]
    evidence: Vec<String>,
    #[serde(default)]
    limitations: Vec<String>,
}

fn main() -> Result<()> {
    let registry = load_registry()?;
    validate_registry(&registry)?;

    if std::env::args().skip(1).any(|arg| arg == "--json") {
        println!("{REGISTRY_JSON}");
        return Ok(());
    }

    println!("Aurora product baseline: {}", registry.product_baseline);
    println!("Experience target: {}", registry.experience_target);
    println!(
        "Primary hardware: {} [{}]",
        registry.primary_hardware_lane.platform, registry.primary_hardware_lane.status
    );
    println!(
        "Fallback/reference: {}",
        registry.primary_hardware_lane.fallback_reference_platform
    );
    println!();
    println!("{:<36} {:<42} SCOPE", "CAPABILITY", "STATUS");
    println!("{}", "-".repeat(120));
    for capability in &registry.capabilities {
        println!(
            "{:<36} {:<42} {}",
            capability.id, capability.status, capability.scope
        );
        if !capability.limitations.is_empty() {
            println!("  limitations: {}", capability.limitations.join("; "));
        }
        if !capability.evidence.is_empty() {
            println!("  evidence: {}", capability.evidence.join("; "));
        }
    }
    Ok(())
}

fn load_registry() -> Result<Registry> {
    serde_json::from_str(REGISTRY_JSON).context("parse config/product-capabilities.json")
}

fn validate_registry(registry: &Registry) -> Result<()> {
    if registry.schema_version != 1 {
        bail!("unsupported capability registry schema {}", registry.schema_version);
    }
    if registry.product_baseline != "q995-clean-baseline" {
        bail!("unexpected product baseline {}", registry.product_baseline);
    }
    if registry.experience_target != "q995-class-7.1.4" {
        bail!("unexpected experience target {}", registry.experience_target);
    }
    if registry.claim_policy.samsung_equivalence_claimed
        || registry.claim_policy.dolby_atmos_compatibility_claimed
        || registry.claim_policy.physical_acoustic_parity_claimed
    {
        bail!("unsupported product-equivalence claim enabled in capability registry");
    }
    if registry.claim_policy.rule.trim().is_empty()
        || registry.primary_hardware_lane.selection_rule.trim().is_empty()
    {
        bail!("claim and hardware-selection policies must not be empty");
    }

    let mut ids = BTreeSet::new();
    for capability in &registry.capabilities {
        if capability.id.trim().is_empty()
            || capability.status.trim().is_empty()
            || capability.scope.trim().is_empty()
        {
            bail!("capability id, status and scope must not be empty");
        }
        if !ids.insert(capability.id.as_str()) {
            bail!("duplicate capability id {}", capability.id);
        }
    }

    for required in [
        "renderer-vbap-2d",
        "renderer-vbap-3d",
        "renderer-evaluation",
        "s6-appliance-bootstrap",
        "earc-iec61937-ingest",
        "external-joc-render-path",
        "pcm-7.1.4-transport",
        "bass-management",
        "room-eq",
        "speaker-calibration",
        "output-protection",
        "wireless-rears",
        "touch-ui",
        "thermal-recovery-soak",
    ] {
        if !ids.contains(required) {
            bail!("required product capability {required} is missing");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_registry_is_valid() {
        let registry = load_registry().expect("registry must parse");
        validate_registry(&registry).expect("registry must satisfy product invariants");
    }

    #[test]
    fn no_equivalence_claim_is_enabled() {
        let registry = load_registry().expect("registry must parse");
        assert!(!registry.claim_policy.samsung_equivalence_claimed);
        assert!(!registry.claim_policy.dolby_atmos_compatibility_claimed);
        assert!(!registry.claim_policy.physical_acoustic_parity_claimed);
    }
}
