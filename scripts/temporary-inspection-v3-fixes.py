from pathlib import Path

p = Path('crates/aurora-runtime-inspection/src/model/tests.rs')
text = p.read_text()
old = '    assert_eq!(report.inspection_schema_version(), 2);'
new = '    assert_eq!(report.inspection_schema_version(), 3);'
if text.count(old) != 1:
    raise SystemExit(f'expected one stale inspection schema assertion, found {text.count(old)}')
text = text.replace(old, new, 1)

# Strengthen the identity proof now that the projection carries exact selected
# implementation and contract versions rather than a single ambiguous number.
renderer_anchor = '''    assert_eq!(
        report
            .runtime()
            .prepared_components
            .renderer
            .contract_major,
        1
    );'''
renderer_extra = renderer_anchor + '''
    assert_eq!(
        report
            .runtime()
            .prepared_components
            .renderer
            .implementation_version,
        "0.1.0"
    );
    assert_eq!(
        report
            .runtime()
            .prepared_components
            .renderer
            .contract_minor,
        0
    );'''
if text.count(renderer_anchor) == 1:
    text = text.replace(renderer_anchor, renderer_extra, 1)

delay_anchor = '''    assert_eq!(
        report
            .runtime()
            .prepared_components
            .realtime_delay
            .contract_major,
        1
    );'''
delay_extra = delay_anchor + '''
    assert_eq!(
        report
            .runtime()
            .prepared_components
            .realtime_delay
            .implementation_version,
        "0.1.0"
    );
    assert_eq!(
        report
            .runtime()
            .prepared_components
            .realtime_delay
            .contract_minor,
        0
    );'''
if text.count(delay_anchor) == 1:
    text = text.replace(delay_anchor, delay_extra, 1)

p.write_text(text)
