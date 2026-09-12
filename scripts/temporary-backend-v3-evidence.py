from pathlib import Path

# Canonical schema-v3 fixture hashes measured from validated canonical JSON.
p = Path('crates/aurora-config/tests/contracts.rs')
text = p.read_text()
old = '''    let expected = [
        0xef8f5bd68119dcf1,
        0x54b72410a4da76ac,
        0xf1eea69ae35151ee,
        0x73890ea81de9a1fb,
        0x4db22da621458129,
        0x0fcf1ead7ce3767b,
        0x2f2b43d7879cbfe7,
    ];'''
new = '''    let expected = [
        0x037b10224ccc54df,
        0x984d8091d17afabe,
        0xbe6d05d004aacd20,
        0x1ef176bb12f32349,
        0xd713ea8d14d6ab2b,
        0xfae61eb5a163b755,
        0xcdeadb7503268139,
    ];'''
if old not in text:
    raise SystemExit('canonical v2 hash baseline not found after v3 transform')
p.write_text(text.replace(old, new, 1))

# Add fail-closed/custom-component proof directly beside the backend registry.
p = Path('crates/aurora-runtime-assembly/src/backend_registry.rs')
text = p.read_text()
if '#[cfg(test)]\nmod tests {' not in text:
    text += r'''

#[cfg(test)]
mod tests {
    use aurora_config::{
        CompatibleMinorRange, ComponentContractKind, ComponentReference, DeviceDirection,
        OperatingMode,
    };

    use super::*;

    const TEST_BACKEND_ID: &str = "org.aurora.backend.test-fixture";
    const TEST_BACKEND_VERSION: &str = "9.9.9";

    fn test_ref(component_id: &str, kind: ComponentContractKind) -> ComponentReference {
        ComponentReference {
            component_id: component_id.to_owned(),
            contract_kind: kind,
            contract_major: 1,
            compatible_minor: CompatibleMinorRange { minimum: 0, maximum: 0 },
            implementation_version_pin: None,
            configuration_schema: 1,
            configuration: serde_json::json!({}),
        }
    }

    fn test_registration() -> BackendComponentRegistration {
        BackendComponentRegistration::new(
            TEST_BACKEND_ID,
            TEST_BACKEND_VERSION,
            ComponentContractKind::AudioOutputBackend,
            1,
            0,
            1,
            BackendComponentCapabilities::new(
                44_100,
                96_000,
                16,
                true,
                BackendPlatformAvailability::Portable,
            ),
            resolve_empty,
        )
    }

    #[test]
    fn custom_backend_registration_needs_no_root_config_enum_change() {
        let registry = BackendComponentRegistry::builtin()
            .with_registration(test_registration())
            .unwrap();
        let prepared = registry
            .resolve(
                &test_ref(TEST_BACKEND_ID, ComponentContractKind::AudioOutputBackend),
                DeviceDirection::Output,
                48_000,
                12,
                OperatingMode::LiveIntent,
            )
            .unwrap();
        assert_eq!(prepared.identity().implementation_id(), TEST_BACKEND_ID);
        assert_eq!(prepared.identity().implementation_version(), TEST_BACKEND_VERSION);
        assert_eq!(prepared.identity().contract_major(), 1);
        assert_eq!(prepared.identity().contract_minor(), 0);
        assert_eq!(prepared.contract_kind(), ComponentContractKind::AudioOutputBackend);
        assert_eq!(prepared.configuration_schema(), 1);
    }

    #[test]
    fn backend_registry_fails_closed_on_unknown_contract_version_payload_and_capability() {
        let registry = BackendComponentRegistry::builtin()
            .with_registration(test_registration())
            .unwrap();

        let unknown = test_ref("org.aurora.backend.unknown", ComponentContractKind::AudioOutputBackend);
        assert!(matches!(
            registry.resolve(&unknown, DeviceDirection::Output, 48_000, 2, OperatingMode::LiveIntent),
            Err(RuntimePreparationError::BackendComponent { issue: BackendComponentIssue::UnknownComponentId, .. })
        ));

        let mut wrong_kind = test_ref(TEST_BACKEND_ID, ComponentContractKind::AudioInputBackend);
        assert!(matches!(
            registry.resolve(&wrong_kind, DeviceDirection::Output, 48_000, 2, OperatingMode::LiveIntent),
            Err(RuntimePreparationError::BackendComponent { issue: BackendComponentIssue::ContractKindMismatch, .. })
        ));
        wrong_kind.contract_kind = ComponentContractKind::AudioOutputBackend;

        let mut wrong_contract = wrong_kind.clone();
        wrong_contract.contract_major = 2;
        assert!(matches!(
            registry.resolve(&wrong_contract, DeviceDirection::Output, 48_000, 2, OperatingMode::LiveIntent),
            Err(RuntimePreparationError::BackendComponent { issue: BackendComponentIssue::IncompatibleContractVersion, .. })
        ));

        let mut wrong_pin = wrong_kind.clone();
        wrong_pin.implementation_version_pin = Some("0.0.1".to_owned());
        assert!(matches!(
            registry.resolve(&wrong_pin, DeviceDirection::Output, 48_000, 2, OperatingMode::LiveIntent),
            Err(RuntimePreparationError::BackendComponent { issue: BackendComponentIssue::IncompatibleImplementationVersion, .. })
        ));

        let mut wrong_schema = wrong_kind.clone();
        wrong_schema.configuration_schema = 2;
        assert!(matches!(
            registry.resolve(&wrong_schema, DeviceDirection::Output, 48_000, 2, OperatingMode::LiveIntent),
            Err(RuntimePreparationError::BackendComponent { issue: BackendComponentIssue::UnsupportedConfigurationSchema, .. })
        ));

        let mut invalid_payload = wrong_kind.clone();
        invalid_payload.configuration = serde_json::json!({"unexpected": true});
        assert!(matches!(
            registry.resolve(&invalid_payload, DeviceDirection::Output, 48_000, 2, OperatingMode::LiveIntent),
            Err(RuntimePreparationError::BackendComponent { issue: BackendComponentIssue::InvalidConfiguration, .. })
        ));

        assert!(matches!(
            registry.resolve(&wrong_kind, DeviceDirection::Output, 192_000, 2, OperatingMode::LiveIntent),
            Err(RuntimePreparationError::BackendComponent { issue: BackendComponentIssue::FormatCapabilityMismatch, .. })
        ));
    }

    #[test]
    fn offline_backend_is_rejected_from_live_activation() {
        let reference = test_ref(
            OFFLINE_BACKEND_IMPLEMENTATION_ID,
            ComponentContractKind::AudioOutputBackend,
        );
        let registry = BackendComponentRegistry::builtin();
        assert!(matches!(
            registry.resolve(&reference, DeviceDirection::Output, 48_000, 2, OperatingMode::LiveIntent),
            Err(RuntimePreparationError::BackendComponent { issue: BackendComponentIssue::RealtimeSafetyMismatch, .. })
        ));
        assert!(registry
            .resolve(&reference, DeviceDirection::Output, 48_000, 2, OperatingMode::Offline)
            .is_ok());
    }
}
'''
p.write_text(text)
