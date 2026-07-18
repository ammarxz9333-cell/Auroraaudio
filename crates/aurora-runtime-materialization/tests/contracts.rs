use aurora_runtime_materialization::{
    DeferredMaterializationRequirement, DeferredMaterializationRequirementKind,
    MaterializationCapabilityRequirement, MaterializationCapabilityRequirementKind,
    MaterializationDependency, MaterializationDependencyIssue, MaterializationError,
    MaterializationFactSemantics, MaterializationLimit, MaterializationStage,
    RuntimeResourceDescriptor, RuntimeResourceKind, MATERIALIZATION_SCHEMA_VERSION,
    MAX_CAPABILITY_REQUIREMENTS, MAX_DEFERRED_REQUIREMENTS, MAX_MATERIALIZATION_DEPENDENCIES,
    MAX_MATERIALIZATION_RESOURCES, MAX_MATERIALIZATION_STAGES, MAX_STRING_BYTES,
    MAX_TOTAL_STRING_BYTES,
};

#[test]
fn schema_one_constants_are_exact() {
    assert_eq!(MATERIALIZATION_SCHEMA_VERSION, 1);
    assert_eq!(MAX_MATERIALIZATION_RESOURCES, 32);
    assert_eq!(MAX_MATERIALIZATION_STAGES, 6);
    assert_eq!(MAX_MATERIALIZATION_DEPENDENCIES, 64);
    assert_eq!(MAX_CAPABILITY_REQUIREMENTS, 64);
    assert_eq!(MAX_DEFERRED_REQUIREMENTS, 32);
    assert_eq!(MAX_STRING_BYTES, 256);
    assert_eq!(MAX_TOTAL_STRING_BYTES, 32_768);
}

#[test]
fn every_public_enum_variant_is_stable_and_distinct() {
    assert_all_distinct(&[
        RuntimeResourceKind::DeviceIntent,
        RuntimeResourceKind::RequestedAudioFormat,
        RuntimeResourceKind::Topology,
        RuntimeResourceKind::Routing,
        RuntimeResourceKind::Renderer,
        RuntimeResourceKind::Dsp,
        RuntimeResourceKind::Backend,
        RuntimeResourceKind::SharedStorage,
    ]);
    assert_all_distinct(&[
        MaterializationStage::DeviceIntentRequirements,
        MaterializationStage::RequestedFormatRequirements,
        MaterializationStage::RendererRequirements,
        MaterializationStage::DspRequirements,
        MaterializationStage::BackendRequirements,
        MaterializationStage::MaterializationPlanComplete,
    ]);
    assert_all_distinct(&[
        MaterializationCapabilityRequirementKind::RequestedAudioFormat,
        MaterializationCapabilityRequirementKind::CanonicalTopology,
        MaterializationCapabilityRequirementKind::RendererFamily,
        MaterializationCapabilityRequirementKind::DspSchemaState,
        MaterializationCapabilityRequirementKind::BackendFamily,
        MaterializationCapabilityRequirementKind::PreallocatedStorage,
    ]);
    assert_all_distinct(&[
        DeferredMaterializationRequirementKind::MaximumObjects,
        DeferredMaterializationRequirementKind::RendererScratch,
        DeferredMaterializationRequirementKind::RendererHistory,
        DeferredMaterializationRequirementKind::ImplementationTemporary,
        DeferredMaterializationRequirementKind::DelayProcessorStorage,
        DeferredMaterializationRequirementKind::AsrcStorage,
        DeferredMaterializationRequirementKind::BackendRingStorage,
    ]);
    assert_all_distinct(&[
        MaterializationFactSemantics::Requested,
        MaterializationFactSemantics::Prepared,
        MaterializationFactSemantics::MaterializationPlanned,
        MaterializationFactSemantics::Deferred,
        MaterializationFactSemantics::ConstructedUnavailable,
        MaterializationFactSemantics::ActiveUnavailable,
    ]);
    assert_all_distinct(&[
        MaterializationLimit::Resources,
        MaterializationLimit::Stages,
        MaterializationLimit::Dependencies,
        MaterializationLimit::CapabilityRequirements,
        MaterializationLimit::DeferredRequirements,
        MaterializationLimit::StringBytes,
        MaterializationLimit::TotalStringBytes,
    ]);
    assert_all_distinct(&[
        MaterializationDependencyIssue::SelfDependency,
        MaterializationDependencyIssue::ResourceIndexOutOfRange,
        MaterializationDependencyIssue::DependencyIndexOutOfRange,
    ]);
}

#[test]
fn valid_descriptor_is_immutable_and_readable() {
    let descriptor = RuntimeResourceDescriptor::new(
        "renderer-requirement",
        RuntimeResourceKind::Renderer,
        MaterializationStage::RendererRequirements,
        MaterializationFactSemantics::MaterializationPlanned,
    )
    .unwrap();
    assert_eq!(descriptor.identifier(), "renderer-requirement");
    assert_eq!(descriptor.kind(), RuntimeResourceKind::Renderer);
    assert_eq!(
        descriptor.stage(),
        MaterializationStage::RendererRequirements
    );
    assert_eq!(
        descriptor.semantics(),
        MaterializationFactSemantics::MaterializationPlanned
    );
}

#[test]
fn maximum_string_is_accepted_and_oversized_string_is_rejected() {
    let maximum = "x".repeat(MAX_STRING_BYTES);
    let descriptor = RuntimeResourceDescriptor::new(
        maximum.clone(),
        RuntimeResourceKind::SharedStorage,
        MaterializationStage::BackendRequirements,
        MaterializationFactSemantics::Deferred,
    )
    .unwrap();
    assert_eq!(descriptor.identifier(), maximum);

    assert_eq!(
        RuntimeResourceDescriptor::new(
            "x".repeat(MAX_STRING_BYTES + 1),
            RuntimeResourceKind::SharedStorage,
            MaterializationStage::BackendRequirements,
            MaterializationFactSemantics::Deferred,
        ),
        Err(MaterializationError::InvalidIdentifierLength {
            actual: MAX_STRING_BYTES + 1,
            minimum: 1,
            maximum: MAX_STRING_BYTES,
        })
    );
}

#[test]
fn unavailable_runtime_semantics_cannot_be_descriptor_evidence() {
    for semantics in [
        MaterializationFactSemantics::ConstructedUnavailable,
        MaterializationFactSemantics::ActiveUnavailable,
    ] {
        assert_eq!(
            RuntimeResourceDescriptor::new(
                "unavailable",
                RuntimeResourceKind::Backend,
                MaterializationStage::BackendRequirements,
                semantics,
            ),
            Err(MaterializationError::UnsupportedSemanticCategory { semantics })
        );
    }
}

#[test]
fn capability_and_deferred_markers_have_fixed_honest_semantics() {
    let capability = MaterializationCapabilityRequirement::new(
        "format-requirement",
        MaterializationCapabilityRequirementKind::RequestedAudioFormat,
    )
    .unwrap();
    assert_eq!(capability.identifier(), "format-requirement");
    assert_eq!(
        capability.kind(),
        MaterializationCapabilityRequirementKind::RequestedAudioFormat
    );
    assert_eq!(
        capability.semantics(),
        MaterializationFactSemantics::MaterializationPlanned
    );

    let deferred = DeferredMaterializationRequirement::new(
        "renderer-scratch",
        DeferredMaterializationRequirementKind::RendererScratch,
    )
    .unwrap();
    assert_eq!(deferred.identifier(), "renderer-scratch");
    assert_eq!(
        deferred.kind(),
        DeferredMaterializationRequirementKind::RendererScratch
    );
    assert_eq!(deferred.semantics(), MaterializationFactSemantics::Deferred);
}

#[test]
fn dependency_constructor_checks_only_local_relationships() {
    let dependency = MaterializationDependency::new(1, 0).unwrap();
    assert_eq!(dependency.resource_index(), 1);
    assert_eq!(dependency.depends_on_index(), 0);

    assert_eq!(
        MaterializationDependency::new(1, 1),
        Err(MaterializationError::InvalidDependencyRelationship {
            issue: MaterializationDependencyIssue::SelfDependency,
        })
    );
    assert_eq!(
        MaterializationDependency::new(MAX_MATERIALIZATION_RESOURCES, 0),
        Err(MaterializationError::InvalidDependencyRelationship {
            issue: MaterializationDependencyIssue::ResourceIndexOutOfRange,
        })
    );
    assert_eq!(
        MaterializationDependency::new(0, MAX_MATERIALIZATION_RESOURCES),
        Err(MaterializationError::InvalidDependencyRelationship {
            issue: MaterializationDependencyIssue::DependencyIndexOutOfRange,
        })
    );
}

#[test]
fn structured_errors_have_deterministic_display() {
    let cases = [
        (
            MaterializationError::LimitExceeded {
                limit: MaterializationLimit::Resources,
                actual: 33,
                maximum: 32,
            },
            "materialization limit Resources exceeded: 33 > 32",
        ),
        (
            MaterializationError::InvalidIdentifierLength {
                actual: 0,
                minimum: 1,
                maximum: 256,
            },
            "materialization identifier length 0 is outside 1..=256 bytes",
        ),
        (
            MaterializationError::CumulativeStringAccountingOverflow,
            "materialization string accounting overflowed",
        ),
        (
            MaterializationError::InvalidDependencyRelationship {
                issue: MaterializationDependencyIssue::SelfDependency,
            },
            "invalid materialization dependency: SelfDependency",
        ),
        (
            MaterializationError::UnsupportedSemanticCategory {
                semantics: MaterializationFactSemantics::ActiveUnavailable,
            },
            "materialization descriptor cannot represent ActiveUnavailable as evidence",
        ),
    ];
    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
        let cloned = error;
        assert_eq!(cloned, error);
    }
}

#[test]
fn malformed_local_values_return_errors_without_panicking() {
    let result = std::panic::catch_unwind(|| {
        (
            RuntimeResourceDescriptor::new(
                "",
                RuntimeResourceKind::DeviceIntent,
                MaterializationStage::DeviceIntentRequirements,
                MaterializationFactSemantics::Requested,
            ),
            MaterializationDependency::new(usize::MAX, usize::MAX),
        )
    });

    let (descriptor, dependency) = result.expect("contract validation must not panic");
    assert!(matches!(
        descriptor,
        Err(MaterializationError::InvalidIdentifierLength { actual: 0, .. })
    ));
    assert!(matches!(
        dependency,
        Err(MaterializationError::InvalidDependencyRelationship {
            issue: MaterializationDependencyIssue::ResourceIndexOutOfRange,
        })
    ));
}

fn assert_all_distinct<T: Eq + core::fmt::Debug>(values: &[T]) {
    for (index, value) in values.iter().enumerate() {
        assert!(
            !values[..index].contains(value),
            "duplicate variant: {value:?}"
        );
    }
}
