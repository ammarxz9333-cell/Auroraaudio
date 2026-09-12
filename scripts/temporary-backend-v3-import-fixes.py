from pathlib import Path

# Test-only symbol qualification after the schema-v3 transformation.  Keep
# production modules free of extra imports just to support test assertions.

p = Path('crates/aurora-runtime-assembly/tests/contracts.rs')
t = p.read_text()
t = t.replace('        CPAL_BACKEND_IMPLEMENTATION_ID\n', '        aurora_runtime_assembly::CPAL_BACKEND_IMPLEMENTATION_ID\n')
t = t.replace('        VIRTUAL_BACKEND_IMPLEMENTATION_ID\n', '        aurora_runtime_assembly::VIRTUAL_BACKEND_IMPLEMENTATION_ID\n')
p.write_text(t)

p = Path('crates/aurora-runtime-assembly/src/derivation.rs')
t = p.read_text()
t = t.replace('            VIRTUAL_BACKEND_IMPLEMENTATION_ID\n', '            crate::VIRTUAL_BACKEND_IMPLEMENTATION_ID\n')
p.write_text(t)

p = Path('crates/aurora-runtime-assembly/src/setup.rs')
t = p.read_text()
t = t.replace('            PreparedComponentIdentity::new(component_id, implementation_version, 1, 0),', '            crate::PreparedComponentIdentity::new(component_id, implementation_version, 1, 0),')
t = t.replace('            CPAL_BACKEND_IMPLEMENTATION_ID\n', '            crate::CPAL_BACKEND_IMPLEMENTATION_ID\n')
t = t.replace('            VIRTUAL_BACKEND_IMPLEMENTATION_ID\n', '            crate::VIRTUAL_BACKEND_IMPLEMENTATION_ID\n')
t = t.replace('                VIRTUAL_BACKEND_IMPLEMENTATION_ID,\n                VIRTUAL_BACKEND_IMPLEMENTATION_VERSION,', '                crate::VIRTUAL_BACKEND_IMPLEMENTATION_ID,\n                crate::VIRTUAL_BACKEND_IMPLEMENTATION_VERSION,')
p.write_text(t)
