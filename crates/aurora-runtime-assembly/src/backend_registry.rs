use aurora_config::{ComponentContractKind, ComponentReference, DeviceDirection, OperatingMode};
use serde_json::Value;

use crate::{
    BackendComponentIssue, PreparedBackendComponentIntent, PreparedComponentIdentity,
    RuntimePreparationError,
};

pub const REALTIME_BACKEND_CONTRACT_MINOR: u16 = 0;
pub const VIRTUAL_BACKEND_IMPLEMENTATION_ID: &str = "org.aurora.backend.virtual";
pub const CPAL_BACKEND_IMPLEMENTATION_ID: &str = "org.aurora.backend.cpal";
pub const OFFLINE_BACKEND_IMPLEMENTATION_ID: &str = "org.aurora.backend.offline";
pub const VIRTUAL_BACKEND_IMPLEMENTATION_VERSION: &str = "0.1.0";
pub const CPAL_BACKEND_IMPLEMENTATION_VERSION: &str = "0.1.0";
pub const OFFLINE_BACKEND_IMPLEMENTATION_VERSION: &str = "0.1.0";
const REALTIME_BACKEND_CONTRACT_VERSION: u16 = 1;

pub type BackendConfigurationResolver = fn(&Value) -> Result<(), BackendComponentIssue>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendPlatformAvailability {
    Portable,
    HostAudioBuild,
    OfflineOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackendComponentCapabilities {
    min_sample_rate: u32,
    max_sample_rate: u32,
    max_channels: usize,
    realtime_safe: bool,
    platform: BackendPlatformAvailability,
}

impl BackendComponentCapabilities {
    pub const fn new(
        min_sample_rate: u32,
        max_sample_rate: u32,
        max_channels: usize,
        realtime_safe: bool,
        platform: BackendPlatformAvailability,
    ) -> Self {
        Self {
            min_sample_rate,
            max_sample_rate,
            max_channels,
            realtime_safe,
            platform,
        }
    }
}

#[derive(Clone, Copy)]
pub struct BackendComponentRegistration {
    component_id: &'static str,
    implementation_version: &'static str,
    contract_kind: ComponentContractKind,
    contract_major: u16,
    contract_minor: u16,
    configuration_schema: u16,
    capabilities: BackendComponentCapabilities,
    resolver: BackendConfigurationResolver,
}

impl BackendComponentRegistration {
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        component_id: &'static str,
        implementation_version: &'static str,
        contract_kind: ComponentContractKind,
        contract_major: u16,
        contract_minor: u16,
        configuration_schema: u16,
        capabilities: BackendComponentCapabilities,
        resolver: BackendConfigurationResolver,
    ) -> Self {
        Self {
            component_id,
            implementation_version,
            contract_kind,
            contract_major,
            contract_minor,
            configuration_schema,
            capabilities,
            resolver,
        }
    }
    pub const fn component_id(self) -> &'static str {
        self.component_id
    }
    pub const fn implementation_version(self) -> &'static str {
        self.implementation_version
    }
    pub const fn contract_kind(self) -> ComponentContractKind {
        self.contract_kind
    }
}

#[derive(Clone)]
pub struct BackendComponentRegistry {
    registrations: Vec<BackendComponentRegistration>,
}

impl BackendComponentRegistry {
    pub fn builtin() -> Self {
        let portable = BackendComponentCapabilities::new(
            8_000,
            192_000,
            32,
            true,
            BackendPlatformAvailability::Portable,
        );
        let host = BackendComponentCapabilities::new(
            8_000,
            192_000,
            32,
            true,
            BackendPlatformAvailability::HostAudioBuild,
        );
        let offline = BackendComponentCapabilities::new(
            8_000,
            192_000,
            32,
            false,
            BackendPlatformAvailability::OfflineOnly,
        );
        let mut registrations = Vec::new();
        for kind in [
            ComponentContractKind::AudioInputBackend,
            ComponentContractKind::AudioOutputBackend,
        ] {
            registrations.push(BackendComponentRegistration::new(
                VIRTUAL_BACKEND_IMPLEMENTATION_ID,
                VIRTUAL_BACKEND_IMPLEMENTATION_VERSION,
                kind,
                REALTIME_BACKEND_CONTRACT_VERSION,
                REALTIME_BACKEND_CONTRACT_MINOR,
                1,
                portable,
                resolve_empty,
            ));
            registrations.push(BackendComponentRegistration::new(
                CPAL_BACKEND_IMPLEMENTATION_ID,
                CPAL_BACKEND_IMPLEMENTATION_VERSION,
                kind,
                REALTIME_BACKEND_CONTRACT_VERSION,
                REALTIME_BACKEND_CONTRACT_MINOR,
                1,
                host,
                resolve_empty,
            ));
            registrations.push(BackendComponentRegistration::new(
                OFFLINE_BACKEND_IMPLEMENTATION_ID,
                OFFLINE_BACKEND_IMPLEMENTATION_VERSION,
                kind,
                REALTIME_BACKEND_CONTRACT_VERSION,
                REALTIME_BACKEND_CONTRACT_MINOR,
                1,
                offline,
                resolve_empty,
            ));
        }
        Self { registrations }
    }

    pub fn with_registration(
        mut self,
        registration: BackendComponentRegistration,
    ) -> Result<Self, RuntimePreparationError> {
        if self.registrations.iter().any(|entry| {
            entry.component_id == registration.component_id
                && entry.contract_kind == registration.contract_kind
        }) {
            return Err(component_error(
                registration.component_id,
                BackendComponentIssue::DuplicateRegistration,
            ));
        }
        self.registrations.push(registration);
        Ok(self)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn resolve(
        &self,
        reference: &ComponentReference,
        direction: DeviceDirection,
        sample_rate: u32,
        channel_count: usize,
        operating_mode: OperatingMode,
    ) -> Result<PreparedBackendComponentIntent, RuntimePreparationError> {
        let expected_kind = match direction {
            DeviceDirection::Input => ComponentContractKind::AudioInputBackend,
            DeviceDirection::Output => ComponentContractKind::AudioOutputBackend,
        };
        if reference.contract_kind != expected_kind {
            return Err(component_error(
                &reference.component_id,
                BackendComponentIssue::ContractKindMismatch,
            ));
        }
        let registration = self
            .registrations
            .iter()
            .find(|entry| {
                entry.component_id == reference.component_id && entry.contract_kind == expected_kind
            })
            .copied()
            .ok_or_else(|| {
                component_error(
                    &reference.component_id,
                    BackendComponentIssue::UnknownComponentId,
                )
            })?;
        if reference.contract_major != registration.contract_major
            || reference.compatible_minor.minimum > registration.contract_minor
            || reference.compatible_minor.maximum < registration.contract_minor
        {
            return Err(component_error(
                &reference.component_id,
                BackendComponentIssue::IncompatibleContractVersion,
            ));
        }
        if reference
            .implementation_version_pin
            .as_deref()
            .is_some_and(|pin| pin != registration.implementation_version)
        {
            return Err(component_error(
                &reference.component_id,
                BackendComponentIssue::IncompatibleImplementationVersion,
            ));
        }
        if reference.configuration_schema != registration.configuration_schema {
            return Err(component_error(
                &reference.component_id,
                BackendComponentIssue::UnsupportedConfigurationSchema,
            ));
        }
        (registration.resolver)(&reference.configuration)
            .map_err(|issue| component_error(&reference.component_id, issue))?;
        let caps = registration.capabilities;
        if sample_rate < caps.min_sample_rate
            || sample_rate > caps.max_sample_rate
            || channel_count > caps.max_channels
        {
            return Err(component_error(
                &reference.component_id,
                BackendComponentIssue::FormatCapabilityMismatch,
            ));
        }
        if operating_mode == OperatingMode::LiveIntent && !caps.realtime_safe {
            return Err(component_error(
                &reference.component_id,
                BackendComponentIssue::RealtimeSafetyMismatch,
            ));
        }
        let platform_ok = match caps.platform {
            BackendPlatformAvailability::Portable => true,
            BackendPlatformAvailability::HostAudioBuild => cfg!(any(
                target_os = "linux",
                target_os = "windows",
                target_os = "macos",
                target_os = "android",
                target_os = "ios"
            )),
            BackendPlatformAvailability::OfflineOnly => operating_mode == OperatingMode::Offline,
        };
        if !platform_ok {
            return Err(component_error(
                &reference.component_id,
                BackendComponentIssue::PlatformUnavailable,
            ));
        }
        Ok(PreparedBackendComponentIntent::new(
            PreparedComponentIdentity::new(
                registration.component_id,
                registration.implementation_version,
                registration.contract_major,
                registration.contract_minor,
            ),
            registration.contract_kind,
            registration.configuration_schema,
        ))
    }
}

fn resolve_empty(payload: &Value) -> Result<(), BackendComponentIssue> {
    match payload.as_object() {
        Some(values) if values.is_empty() => Ok(()),
        _ => Err(BackendComponentIssue::InvalidConfiguration),
    }
}

fn component_error(component_id: &str, issue: BackendComponentIssue) -> RuntimePreparationError {
    RuntimePreparationError::BackendComponent {
        component_id: component_id.to_owned(),
        issue,
    }
}

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
            compatible_minor: CompatibleMinorRange {
                minimum: 0,
                maximum: 0,
            },
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
        assert_eq!(
            prepared.identity().implementation_version(),
            TEST_BACKEND_VERSION
        );
        assert_eq!(prepared.identity().contract_major(), 1);
        assert_eq!(prepared.identity().contract_minor(), 0);
        assert_eq!(
            prepared.contract_kind(),
            ComponentContractKind::AudioOutputBackend
        );
        assert_eq!(prepared.configuration_schema(), 1);
    }

    #[test]
    fn backend_registry_fails_closed_on_unknown_contract_version_payload_and_capability() {
        let registry = BackendComponentRegistry::builtin()
            .with_registration(test_registration())
            .unwrap();

        let unknown = test_ref(
            "org.aurora.backend.unknown",
            ComponentContractKind::AudioOutputBackend,
        );
        assert!(matches!(
            registry.resolve(
                &unknown,
                DeviceDirection::Output,
                48_000,
                2,
                OperatingMode::LiveIntent
            ),
            Err(RuntimePreparationError::BackendComponent {
                issue: BackendComponentIssue::UnknownComponentId,
                ..
            })
        ));

        let mut wrong_kind = test_ref(TEST_BACKEND_ID, ComponentContractKind::AudioInputBackend);
        assert!(matches!(
            registry.resolve(
                &wrong_kind,
                DeviceDirection::Output,
                48_000,
                2,
                OperatingMode::LiveIntent
            ),
            Err(RuntimePreparationError::BackendComponent {
                issue: BackendComponentIssue::ContractKindMismatch,
                ..
            })
        ));
        wrong_kind.contract_kind = ComponentContractKind::AudioOutputBackend;

        let mut wrong_contract = wrong_kind.clone();
        wrong_contract.contract_major = 2;
        assert!(matches!(
            registry.resolve(
                &wrong_contract,
                DeviceDirection::Output,
                48_000,
                2,
                OperatingMode::LiveIntent
            ),
            Err(RuntimePreparationError::BackendComponent {
                issue: BackendComponentIssue::IncompatibleContractVersion,
                ..
            })
        ));

        let mut wrong_pin = wrong_kind.clone();
        wrong_pin.implementation_version_pin = Some("0.0.1".to_owned());
        assert!(matches!(
            registry.resolve(
                &wrong_pin,
                DeviceDirection::Output,
                48_000,
                2,
                OperatingMode::LiveIntent
            ),
            Err(RuntimePreparationError::BackendComponent {
                issue: BackendComponentIssue::IncompatibleImplementationVersion,
                ..
            })
        ));

        let mut wrong_schema = wrong_kind.clone();
        wrong_schema.configuration_schema = 2;
        assert!(matches!(
            registry.resolve(
                &wrong_schema,
                DeviceDirection::Output,
                48_000,
                2,
                OperatingMode::LiveIntent
            ),
            Err(RuntimePreparationError::BackendComponent {
                issue: BackendComponentIssue::UnsupportedConfigurationSchema,
                ..
            })
        ));

        let mut invalid_payload = wrong_kind.clone();
        invalid_payload.configuration = serde_json::json!({"unexpected": true});
        assert!(matches!(
            registry.resolve(
                &invalid_payload,
                DeviceDirection::Output,
                48_000,
                2,
                OperatingMode::LiveIntent
            ),
            Err(RuntimePreparationError::BackendComponent {
                issue: BackendComponentIssue::InvalidConfiguration,
                ..
            })
        ));

        assert!(matches!(
            registry.resolve(
                &wrong_kind,
                DeviceDirection::Output,
                192_000,
                2,
                OperatingMode::LiveIntent
            ),
            Err(RuntimePreparationError::BackendComponent {
                issue: BackendComponentIssue::FormatCapabilityMismatch,
                ..
            })
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
            registry.resolve(
                &reference,
                DeviceDirection::Output,
                48_000,
                2,
                OperatingMode::LiveIntent
            ),
            Err(RuntimePreparationError::BackendComponent {
                issue: BackendComponentIssue::RealtimeSafetyMismatch,
                ..
            })
        ));
        assert!(registry
            .resolve(
                &reference,
                DeviceDirection::Output,
                48_000,
                2,
                OperatingMode::Offline
            )
            .is_ok());
    }
}
