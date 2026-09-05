//! Capability-registry presentation shared by CLI and documentation checks.

use std::fmt::Write as _;

use anyhow::{Context, Result};
use aurora_core::{
    canonical_capability_registry, ArtifactVerification, CapabilityKind, CapabilityRegistry,
    ImplementationStatus, SupportStatus,
};

/// Stable README marker before the generated capability table.
pub const README_CAPABILITIES_BEGIN: &str = "<!-- AURORA_CAPABILITIES_BEGIN -->";
/// Stable README marker after the generated capability table.
pub const README_CAPABILITIES_END: &str = "<!-- AURORA_CAPABILITIES_END -->";

/// Capability output formats owned by the CLI presentation layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityOutputFormat {
    /// Human-readable detailed text.
    Text,
    /// Stable machine-readable JSON registry.
    Json,
    /// README-compatible generated Markdown table.
    Markdown,
}

/// Renders the canonical validated registry in the selected presentation format.
pub fn render_capabilities(format: CapabilityOutputFormat) -> Result<String> {
    let registry = canonical_capability_registry();
    registry
        .validate()
        .context("validate capability registry")?;
    match format {
        CapabilityOutputFormat::Text => Ok(render_text(&registry)),
        CapabilityOutputFormat::Json => {
            serde_json::to_string_pretty(&registry).context("serialize capability registry")
        }
        CapabilityOutputFormat::Markdown => Ok(render_markdown(&registry)),
    }
}

/// Returns README content with the generated capability section replaced.
pub fn replace_readme_capability_section(readme: &str) -> Result<String> {
    let begin = readme
        .find(README_CAPABILITIES_BEGIN)
        .context("README capability begin marker is missing")?;
    let after_begin = begin + README_CAPABILITIES_BEGIN.len();
    let relative_end = readme[after_begin..]
        .find(README_CAPABILITIES_END)
        .context("README capability end marker is missing")?;
    let end = after_begin + relative_end;
    let table = render_capabilities(CapabilityOutputFormat::Markdown)?;

    let mut output = String::with_capacity(readme.len() + table.len());
    output.push_str(&readme[..after_begin]);
    output.push_str("\n\n");
    output.push_str(&table);
    output.push_str("\n\n");
    output.push_str(&readme[end..]);
    Ok(output)
}

/// Verifies that the README generated section exactly matches the canonical registry.
pub fn verify_readme_capability_section(readme: &str) -> Result<()> {
    let expected = replace_readme_capability_section(readme)?;
    anyhow::ensure!(
        expected == readme,
        "README capability table is stale or claims capability state not present in the registry"
    );
    Ok(())
}

fn render_text(registry: &CapabilityRegistry) -> String {
    let mut output = String::new();
    writeln!(
        output,
        "Aurora capability registry v{}",
        registry.schema_version
    )
    .unwrap();
    writeln!(
        output,
        "{:<20} {:<19} {:<12} {:<12} {:<16} {:<11}",
        "ID", "IMPLEMENTATION", "OFFLINE", "REALTIME", "VERIFICATION", "PRODUCTION"
    )
    .unwrap();
    for entry in &registry.capabilities {
        writeln!(
            output,
            "{:<20} {:<19} {:<12} {:<12} {:<16} {:<11}",
            entry.id,
            implementation_label(entry.implementation_status),
            support_label(entry.offline_support),
            support_label(entry.realtime_support),
            verification_label(entry.artifact_verification),
            yes_no(entry.production_ready),
        )
        .unwrap();
        writeln!(output, "  name: {}", entry.name).unwrap();
        writeln!(
            output,
            "  kind: {} | tested layouts: {} | adapter placeholder: {}",
            kind_label(entry.kind),
            list_or_none(&entry.tested_layouts),
            yes_no(entry.adapter_placeholder)
        )
        .unwrap();
        writeln!(
            output,
            "  limitations: {}",
            list_or_none(&entry.cue_limitations)
        )
        .unwrap();
        writeln!(output, "  evidence: {}", list_or_none(&entry.evidence)).unwrap();
    }
    output
}

fn render_markdown(registry: &CapabilityRegistry) -> String {
    let mut output = String::new();
    output.push_str("| Capability | Implementation | Offline | Realtime | Verification | Tested layouts | Production-ready |\n");
    output.push_str("| --- | --- | --- | --- | --- | --- | --- |\n");
    for entry in &registry.capabilities {
        writeln!(
            output,
            "| `{}` — {} | {} | {} | {} | {} | {} | {} |",
            entry.id,
            escape_markdown(&entry.name),
            implementation_label(entry.implementation_status),
            support_label(entry.offline_support),
            support_label(entry.realtime_support),
            verification_label(entry.artifact_verification),
            escape_markdown(&list_or_none(&entry.tested_layouts)),
            yes_no(entry.production_ready),
        )
        .unwrap();
    }
    while output.ends_with('\n') {
        output.pop();
    }
    output
}

fn implementation_label(status: ImplementationStatus) -> &'static str {
    match status {
        ImplementationStatus::Experimental => "experimental",
        ImplementationStatus::Functional => "functional",
        ImplementationStatus::AdapterPlaceholder => "adapter-placeholder",
        ImplementationStatus::InactiveResearch => "inactive-research",
        ImplementationStatus::NotImplemented => "not-implemented",
    }
}

fn support_label(status: SupportStatus) -> &'static str {
    match status {
        SupportStatus::Unsupported => "unsupported",
        SupportStatus::Experimental => "experimental",
        SupportStatus::Functional => "functional",
    }
}

fn verification_label(status: ArtifactVerification) -> &'static str {
    match status {
        ArtifactVerification::None => "none",
        ArtifactVerification::SoftwareTested => "software-tested",
        ArtifactVerification::CiArtifact => "ci-artifact",
        ArtifactVerification::PhysicalAcceptance => "physical-acceptance",
    }
}

fn kind_label(kind: CapabilityKind) -> &'static str {
    match kind {
        CapabilityKind::Renderer => "renderer",
        CapabilityKind::Decoder => "decoder",
        CapabilityKind::Dsp => "dsp",
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn list_or_none(values: &[String]) -> String {
    if values.is_empty() {
        "none".to_owned()
    } else {
        values.join("; ")
    }
}

fn escape_markdown(value: &str) -> String {
    value.replace('|', "\\|")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_output_is_versioned_and_contains_conservative_entries() {
        let output = render_capabilities(CapabilityOutputFormat::Json).unwrap();
        assert!(output.contains("\"schema_version\": 1"));
        assert!(output.contains("\"geometric-binaural\""));
        assert!(output.contains("\"adapter_placeholder\""));
        assert!(output.contains("\"loudspeaker-3d\""));
    }

    #[test]
    fn generated_markdown_never_marks_current_entries_production_ready() {
        let output = render_capabilities(CapabilityOutputFormat::Markdown).unwrap();
        assert!(output.contains("| `iamf` — IAMF decoder adapter | adapter-placeholder |"));
        assert!(output.contains("| `loudspeaker-3d` — 3D loudspeaker renderer | not-implemented |"));
        assert!(!output.contains("| yes |"));
    }

    #[test]
    fn readme_verification_detects_manual_claim_changes() {
        let source = format!(
            "before\n{}\n\nstale table\n\n{}\nafter",
            README_CAPABILITIES_BEGIN, README_CAPABILITIES_END
        );
        assert!(verify_readme_capability_section(&source).is_err());
        let generated = replace_readme_capability_section(&source).unwrap();
        verify_readme_capability_section(&generated).unwrap();
    }
}
