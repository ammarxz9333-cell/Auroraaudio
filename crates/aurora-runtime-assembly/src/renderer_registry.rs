use aurora_config::{ComponentContractKind, ComponentReference};
use serde_json::Value;

use crate::{
    PreparedComponentIdentity, PreparedRendererPlan, RendererComponentIssue,
    RuntimePreparationError, BASIC_RENDERER_IMPLEMENTATION_ID,
    BASIC_RENDERER_IMPLEMENTATION_VERSION, REALTIME_RENDERER_CONTRACT_MINOR,
    REALTIME_RENDERER_CONTRACT_VERSION, VBAP_RENDERER_IMPLEMENTATION_ID,
    VBAP_RENDERER_IMPLEMENTATION_VERSION,
};

pub type RendererConfigurationResolver =
    fn(&Value, usize) -> Result<PreparedRendererPlan, RendererComponentIssue>;

pub type RendererConfigurationValidator =
    fn(&Value, usize) -> Result<(), RendererComponentIssue>;

#[derive(Clone, Copy)]
enum RendererRegistrationBehavior {
    Gain(RendererConfigurationResolver),
    ExternalObjectPcm(RendererConfigurationValidator),
}

#[derive(Clone, Copy)]
pub struct RendererComponentRegistration {
    component_id: &'static str,
    implementation_version: &'static str,
    contract_major: u16,
    contract_minor: u16,
    configuration_schema: u16,
    behavior: RendererRegistrationBehavior,
}

impl RendererComponentRegistration {
    pub const fn new(
        component_id: &'static str,
        implementation_version: &'static str,
        contract_major: u16,
        contract_minor: u16,
        configuration_schema: u16,
        resolver: RendererConfigurationResolver,
    ) -> Self {
        Self {
            component_id,
            implementation_version,
            contract_major,
            contract_minor,
            configuration_schema,
            behavior: RendererRegistrationBehavior::Gain(resolver),
        }
    }

    pub const fn external_object_pcm(
        component_id: &'static str,
        implementation_version: &'static str,
        contract_major: u16,
        contract_minor: u16,
        configuration_schema: u16,
        validator: RendererConfigurationValidator,
    ) -> Self {
        Self {
            component_id,
            implementation_version,
            contract_major,
            contract_minor,
            configuration_schema,
            behavior: RendererRegistrationBehavior::ExternalObjectPcm(validator),
        }
    }

    pub const fn component_id(self) -> &'static str {
        self.component_id
    }
    pub const fn implementation_version(self) -> &'static str {
        self.implementation_version
    }
    pub const fn contract_major(self) -> u16 {
        self.contract_major
    }
    pub const fn contract_minor(self) -> u16 {
        self.contract_minor
    }
    pub const fn configuration_schema(self) -> u16 {
        self.configuration_schema
    }
}

#[derive(Clone)]
pub struct RendererComponentRegistry {
    registrations: Vec<RendererComponentRegistration>,
}

impl RendererComponentRegistry {
    pub fn builtin() -> Self {
        Self {
            registrations: vec![
                RendererComponentRegistration::new(
                    BASIC_RENDERER_IMPLEMENTATION_ID,
                    BASIC_RENDERER_IMPLEMENTATION_VERSION,
                    REALTIME_RENDERER_CONTRACT_VERSION,
                    REALTIME_RENDERER_CONTRACT_MINOR,
                    1,
                    resolve_basic,
                ),
                RendererComponentRegistration::new(
                    VBAP_RENDERER_IMPLEMENTATION_ID,
                    VBAP_RENDERER_IMPLEMENTATION_VERSION,
                    REALTIME_RENDERER_CONTRACT_VERSION,
                    REALTIME_RENDERER_CONTRACT_MINOR,
                    1,
                    resolve_vbap,
                ),
            ],
        }
    }

    pub fn with_registration(
        mut self,
        registration: RendererComponentRegistration,
    ) -> Result<Self, RuntimePreparationError> {
        if self
            .registrations
            .iter()
            .any(|entry| entry.component_id == registration.component_id)
        {
            return Err(component_error(
                registration.component_id,
                RendererComponentIssue::DuplicateRegistration,
            ));
        }
        self.registrations.push(registration);
        Ok(self)
    }

    pub fn resolve(
        &self,
        reference: &ComponentReference,
        active_speakers: usize,
    ) -> Result<PreparedRendererPlan, RuntimePreparationError> {
        if reference.contract_kind != ComponentContractKind::Renderer {
            return Err(component_error(
                &reference.component_id,
                RendererComponentIssue::ContractKindMismatch,
            ));
        }
        let registration = self
            .registrations
            .iter()
            .find(|entry| entry.component_id == reference.component_id)
            .copied()
            .ok_or_else(|| {
                component_error(
                    &reference.component_id,
                    RendererComponentIssue::UnknownComponentId,
                )
            })?;
        if reference.contract_major != registration.contract_major
            || reference.compatible_minor.minimum > registration.contract_minor
            || reference.compatible_minor.maximum < registration.contract_minor
        {
            return Err(component_error(
                &reference.component_id,
                RendererComponentIssue::IncompatibleContractVersion,
            ));
        }
        if reference
            .implementation_version_pin
            .as_deref()
            .is_some_and(|pin| pin != registration.implementation_version)
        {
            return Err(component_error(
                &reference.component_id,
                RendererComponentIssue::IncompatibleImplementationVersion,
            ));
        }
        if reference.configuration_schema != registration.configuration_schema {
            return Err(component_error(
                &reference.component_id,
                RendererComponentIssue::UnsupportedConfigurationSchema,
            ));
        }

        let identity = PreparedComponentIdentity::new(
            registration.component_id,
            registration.implementation_version,
            registration.contract_major,
            registration.contract_minor,
        );
        match registration.behavior {
            RendererRegistrationBehavior::Gain(resolver) => {
                let plan = resolver(&reference.configuration, active_speakers)
                    .map_err(|issue| component_error(&reference.component_id, issue))?;
                Ok(plan.with_component_identity(identity))
            }
            RendererRegistrationBehavior::ExternalObjectPcm(validator) => {
                validator(&reference.configuration, active_speakers)
                    .map_err(|issue| component_error(&reference.component_id, issue))?;
                Ok(PreparedRendererPlan::external_object_pcm(identity))
            }
        }
    }
}

fn resolve_basic(
    payload: &Value,
    _active_speakers: usize,
) -> Result<PreparedRendererPlan, RendererComponentIssue> {
    match payload.as_object() {
        Some(values) if values.is_empty() => Ok(PreparedRendererPlan::basic_inverse_distance()),
        _ => Err(RendererComponentIssue::InvalidConfiguration),
    }
}

fn resolve_vbap(
    payload: &Value,
    active_speakers: usize,
) -> Result<PreparedRendererPlan, RendererComponentIssue> {
    if active_speakers < 2 {
        return Err(RendererComponentIssue::LayoutCapabilityMismatch);
    }
    let values = payload
        .as_object()
        .ok_or(RendererComponentIssue::InvalidConfiguration)?;
    match values.get("mode").and_then(Value::as_str) {
        Some("point_source") if values.len() == 1 => {
            Ok(PreparedRendererPlan::point_source_horizontal_vbap())
        }
        Some("horizontal_spread") if values.len() == 2 => {
            let spread = values
                .get("spread")
                .and_then(Value::as_f64)
                .ok_or(RendererComponentIssue::InvalidConfiguration)?;
            if !spread.is_finite() || !(0.0..=1.0).contains(&spread) {
                return Err(RendererComponentIssue::InvalidConfiguration);
            }
            PreparedRendererPlan::horizontal_spread_vbap(spread as f32)
                .map_err(|_| RendererComponentIssue::InvalidConfiguration)
        }
        _ => Err(RendererComponentIssue::InvalidConfiguration),
    }
}

fn component_error(component_id: &str, issue: RendererComponentIssue) -> RuntimePreparationError {
    RuntimePreparationError::RendererComponent {
        component_id: component_id.to_owned(),
        issue,
    }
}
