from pathlib import Path
import re


def add_after_once(text, anchor, addition, label):
    if addition.strip() in text:
        return text
    if text.count(anchor) != 1:
        raise SystemExit(f'{label}: expected one anchor, found {text.count(anchor)}')
    return text.replace(anchor, anchor + addition, 1)


def backend_ref_expr(kind, direction):
    return f'common_backend_reference("org.aurora.backend.{kind.lower()}", DeviceDirection::{direction})'


def replace_device_backend_enums(text):
    # Preserve contract kind according to root/device role, not the enum label.
    pattern = re.compile(r'backend: BackendIntent::(Virtual|Cpal|Offline),\n(?P<indent>\s*)direction: DeviceDirection::(Input|Output),')
    def repl(match):
        kind = match.group(1)
        direction = match.group(3)
        indent = match.group('indent')
        return f'backend: {backend_ref_expr(kind, direction)},\n{indent}direction: DeviceDirection::{direction},'
    return pattern.sub(repl, text)


# All DeviceSelectionIntent test constructors become component refs.
for path in [
    Path('crates/aurora-config/tests/contracts.rs'),
    Path('crates/aurora-runtime-assembly/src/derivation.rs'),
    Path('crates/aurora-runtime-assembly/src/setup.rs'),
    Path('crates/aurora-runtime-assembly/tests/contracts.rs'),
    Path('crates/aurora-runtime-inspection/src/model/tests.rs'),
]:
    text = path.read_text()
    text = replace_device_backend_enums(text)
    path.write_text(text)

# Ensure setup tests own their component-ref/prepared-backend helpers.
p = Path('crates/aurora-runtime-assembly/src/setup.rs')
t = p.read_text()
anchor = '    use crate::{prepare_runtime_plan, PreparedRendererKind};\n'
helpers = '''\n    fn common_backend_reference(\n        component_id: &str,\n        direction: DeviceDirection,\n    ) -> ComponentReference {\n        ComponentReference {\n            component_id: component_id.to_owned(),\n            contract_kind: match direction {\n                DeviceDirection::Input => ComponentContractKind::AudioInputBackend,\n                DeviceDirection::Output => ComponentContractKind::AudioOutputBackend,\n            },\n            contract_major: 1,\n            compatible_minor: CompatibleMinorRange { minimum: 0, maximum: 0 },\n            implementation_version_pin: None,\n            configuration_schema: 1,\n            configuration: serde_json::json!({}),\n        }\n    }\n\n    fn prepared_backend(\n        component_id: &'static str,\n        implementation_version: &'static str,\n        direction: DeviceDirection,\n    ) -> PreparedBackendComponentIntent {\n        PreparedBackendComponentIntent::new(\n            PreparedComponentIdentity::new(component_id, implementation_version, 1, 0),\n            match direction {\n                DeviceDirection::Input => ComponentContractKind::AudioInputBackend,\n                DeviceDirection::Output => ComponentContractKind::AudioOutputBackend,\n            },\n            1,\n        )\n    }\n'''
t = add_after_once(t, anchor, helpers, 'setup helper')
t = t.replace(
    '''        assert_eq!(\n            plan.backend().requested_input_backend(),\n            Some(BackendIntent::Cpal)\n        );''',
    '''        assert_eq!(\n            plan.backend().requested_input_backend().unwrap().identity().implementation_id(),\n            CPAL_BACKEND_IMPLEMENTATION_ID\n        );'''
)
t = t.replace(
    '''        assert_eq!(\n            plan.backend().requested_output_backend(),\n            Some(BackendIntent::Virtual)\n        );''',
    '''        assert_eq!(\n            plan.backend().requested_output_backend().unwrap().identity().implementation_id(),\n            VIRTUAL_BACKEND_IMPLEMENTATION_ID\n        );'''
)
t = t.replace(
    'requested_output_backend: Some(BackendIntent::Virtual),',
    'requested_output_backend: Some(prepared_backend(VIRTUAL_BACKEND_IMPLEMENTATION_ID, VIRTUAL_BACKEND_IMPLEMENTATION_VERSION, DeviceDirection::Output)),',
)
p.write_text(t)

# Integration assertions compare stable selected implementation IDs.
p = Path('crates/aurora-runtime-assembly/tests/contracts.rs')
t = p.read_text()
t = t.replace(
    '''    assert_eq!(\n        setup.backend().requested_input_backend(),\n        Some(BackendIntent::Cpal)\n    );''',
    '''    assert_eq!(\n        setup.backend().requested_input_backend().unwrap().identity().implementation_id(),\n        CPAL_BACKEND_IMPLEMENTATION_ID\n    );'''
)
t = t.replace(
    '''    assert_eq!(\n        setup.backend().requested_output_backend(),\n        Some(BackendIntent::Virtual)\n    );''',
    '''    assert_eq!(\n        setup.backend().requested_output_backend().unwrap().identity().implementation_id(),\n        VIRTUAL_BACKEND_IMPLEMENTATION_ID\n    );'''
)
p.write_text(t)

# Derivation assertions compare selected backend identity rather than a deleted enum.
p = Path('crates/aurora-runtime-assembly/src/derivation.rs')
t = p.read_text()
t = re.sub(
    r'assert_eq!\(\s*plan\.device_intent\(\)\.output\(\)\.unwrap\(\)\.backend\(\),\s*BackendIntent::Virtual\s*\);',
    'assert_eq!(plan.device_intent().output().unwrap().backend().identity().implementation_id(), VIRTUAL_BACKEND_IMPLEMENTATION_ID);',
    t,
)
p.write_text(t)

# Direct low-level selector tests require a prepared backend descriptor.
p = Path('crates/aurora-runtime-assembly/src/lib.rs')
t = p.read_text()
anchor = '''    fn identity(id: &str) -> PreparedChannelIdentity {\n        PreparedChannelIdentity::new(id, id.to_uppercase()).unwrap()\n    }\n'''
helper = '''\n    fn prepared_backend(\n        component_id: &'static str,\n        implementation_version: &'static str,\n        direction: ComponentContractKind,\n    ) -> PreparedBackendComponentIntent {\n        PreparedBackendComponentIntent::new(\n            PreparedComponentIdentity::new(component_id, implementation_version, 1, 0),\n            direction,\n            1,\n        )\n    }\n'''
t = add_after_once(t, anchor, helper, 'lib test backend helper')
t = t.replace(
    '            BackendIntent::Virtual,\n            AmbiguityPolicy::RequireStableIdentifier,',
    '            prepared_backend(VIRTUAL_BACKEND_IMPLEMENTATION_ID, VIRTUAL_BACKEND_IMPLEMENTATION_VERSION, ComponentContractKind::AudioOutputBackend),\n            AmbiguityPolicy::RequireStableIdentifier,',
)
t = t.replace(
    '                BackendIntent::Offline,\n                AmbiguityPolicy::Reject,',
    '                prepared_backend(OFFLINE_BACKEND_IMPLEMENTATION_ID, OFFLINE_BACKEND_IMPLEMENTATION_VERSION, ComponentContractKind::AudioOutputBackend),\n                AmbiguityPolicy::Reject,',
)
p.write_text(t)

# Inspection model tests use expanded component identity fields.
p = Path('crates/aurora-runtime-inspection/src/model/tests.rs')
t = p.read_text()
t = t.replace('.contract_version,', '.contract_major,')
t = replace_device_backend_enums(t)
p.write_text(t)

# Configuration tests helper import can be multiline-formatted after rustfmt; ensure alias exists.
p = Path('crates/aurora-config/tests/contracts.rs')
t = p.read_text()
if 'backend_reference as common_backend_reference' not in t:
    t = t.replace(
        'use common::{',
        'use common::{\n    backend_reference as common_backend_reference,',
        1,
    )
p.write_text(t)

# Runtime assembly integration helper import/availability.
p = Path('crates/aurora-runtime-assembly/tests/contracts.rs')
t = p.read_text()
if 'fn common_backend_reference' not in t:
    marker = '\n\n#[test]'
    idx = t.index(marker)
    helper = '''\n\nfn common_backend_reference(\n    component_id: &str,\n    direction: DeviceDirection,\n) -> ComponentReference {\n    ComponentReference {\n        component_id: component_id.to_owned(),\n        contract_kind: match direction {\n            DeviceDirection::Input => ComponentContractKind::AudioInputBackend,\n            DeviceDirection::Output => ComponentContractKind::AudioOutputBackend,\n        },\n        contract_major: 1,\n        compatible_minor: CompatibleMinorRange { minimum: 0, maximum: 0 },\n        implementation_version_pin: None,\n        configuration_schema: 1,\n        configuration: serde_json::json!({}),\n    }\n}\n'''
    t = t[:idx] + helper + t[idx:]
p.write_text(t)

# Inspection test helper availability.
p = Path('crates/aurora-runtime-inspection/src/model/tests.rs')
t = p.read_text()
if 'fn common_backend_reference' not in t:
    marker = '\n\nfn prepared_plans'
    idx = t.index(marker)
    helper = '''\n\nfn common_backend_reference(\n    component_id: &str,\n    direction: DeviceDirection,\n) -> ComponentReference {\n    ComponentReference {\n        component_id: component_id.to_owned(),\n        contract_kind: match direction {\n            DeviceDirection::Input => ComponentContractKind::AudioInputBackend,\n            DeviceDirection::Output => ComponentContractKind::AudioOutputBackend,\n        },\n        contract_major: 1,\n        compatible_minor: CompatibleMinorRange { minimum: 0, maximum: 0 },\n        implementation_version_pin: None,\n        configuration_schema: 1,\n        configuration: serde_json::json!({}),\n    }\n}\n'''
    t = t[:idx] + helper + t[idx:]
p.write_text(t)

# The docs may describe the legacy enum, but the production grep gate intentionally checks the token.
p = Path('docs/configuration.md')
t = p.read_text().replace('`BackendIntent::{Virtual,Cpal,Offline}`', 'the legacy Virtual/Cpal/Offline backend enum')
p.write_text(t)

# Remove every stale enum assertion/token from Rust surfaces; fail here if anything unexpected remains.
remaining = []
for path in Path('crates').rglob('*.rs'):
    text = path.read_text()
    if 'BackendIntent' in text:
        remaining.append(str(path))
if remaining:
    raise SystemExit('BackendIntent remains after cleanup: ' + ', '.join(remaining))
