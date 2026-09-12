from pathlib import Path
import re


def qualify(path, symbols, prefix):
    p = Path(path)
    text = p.read_text()
    for symbol in symbols:
        # Rustfmt may move tokens across lines; qualify every unqualified test
        # reference regardless of whitespace while leaving existing paths alone.
        text = re.sub(rf'(?<![A-Za-z0-9_:]){re.escape(symbol)}', prefix + symbol, text)
    p.write_text(text)


qualify(
    'crates/aurora-runtime-assembly/tests/contracts.rs',
    ['CPAL_BACKEND_IMPLEMENTATION_ID', 'VIRTUAL_BACKEND_IMPLEMENTATION_ID'],
    'aurora_runtime_assembly::',
)

qualify(
    'crates/aurora-runtime-assembly/src/derivation.rs',
    ['VIRTUAL_BACKEND_IMPLEMENTATION_ID'],
    'crate::',
)

p = Path('crates/aurora-runtime-assembly/src/setup.rs')
text = p.read_text()
text = re.sub(
    r'(?<![A-Za-z0-9_:])PreparedComponentIdentity::new\(component_id, implementation_version, 1, 0\)',
    'crate::PreparedComponentIdentity::new(component_id, implementation_version, 1, 0)',
    text,
)
for symbol in [
    'CPAL_BACKEND_IMPLEMENTATION_ID',
    'VIRTUAL_BACKEND_IMPLEMENTATION_ID',
    'VIRTUAL_BACKEND_IMPLEMENTATION_VERSION',
]:
    text = re.sub(rf'(?<![A-Za-z0-9_:]){re.escape(symbol)}', 'crate::' + symbol, text)
p.write_text(text)
