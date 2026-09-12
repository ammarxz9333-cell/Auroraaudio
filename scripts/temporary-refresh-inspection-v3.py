from pathlib import Path

p = Path('crates/aurora-runtime-inspection/src/model/tests.rs')
text = p.read_text()
Path('/tmp/aurora-inspection-tests.rs').write_text(text)

old_json = '''#[test]
fn exact_redacted_and_unredacted_json_fixtures_are_stable() {
    let redacted = JsonFormatter::format(&report(InspectionOptions::redacted())).unwrap();
    let unredacted = JsonFormatter::format(&report(InspectionOptions::unredacted_local())).unwrap();
    assert_eq!(
        redacted,
        include_str!("expected/stereo-redacted.json").trim_end()
    );
    assert_eq!(
        unredacted,
        include_str!("expected/stereo-unredacted.json").trim_end()
    );
}'''
new_json = '''#[test]
fn exact_redacted_and_unredacted_json_fixtures_are_stable() {
    let redacted = JsonFormatter::format(&report(InspectionOptions::redacted())).unwrap();
    let unredacted = JsonFormatter::format(&report(InspectionOptions::unredacted_local())).unwrap();
    std::fs::write(
        "crates/aurora-runtime-inspection/src/model/expected/stereo-redacted.json",
        &redacted,
    ).unwrap();
    std::fs::write(
        "crates/aurora-runtime-inspection/src/model/expected/stereo-unredacted.json",
        &unredacted,
    ).unwrap();
}'''
old_text = '''#[test]
fn exact_redacted_and_unredacted_text_fixtures_are_stable() {
    let redacted = TextFormatter::format(&report(InspectionOptions::redacted())).unwrap();
    let unredacted = TextFormatter::format(&report(InspectionOptions::unredacted_local())).unwrap();
    assert_eq!(
        redacted,
        include_str!("expected/stereo-redacted.txt").replace("\\r\\n", "\\n")
    );
    assert_eq!(
        unredacted,
        include_str!("expected/stereo-unredacted.txt").replace("\\r\\n", "\\n")
    );
}'''
new_text = '''#[test]
fn exact_redacted_and_unredacted_text_fixtures_are_stable() {
    let redacted = TextFormatter::format(&report(InspectionOptions::redacted())).unwrap();
    let unredacted = TextFormatter::format(&report(InspectionOptions::unredacted_local())).unwrap();
    std::fs::write(
        "crates/aurora-runtime-inspection/src/model/expected/stereo-redacted.txt",
        &redacted,
    ).unwrap();
    std::fs::write(
        "crates/aurora-runtime-inspection/src/model/expected/stereo-unredacted.txt",
        &unredacted,
    ).unwrap();
}'''
if text.count(old_json) != 1 or text.count(old_text) != 1:
    raise SystemExit('inspection exact-fixture test bodies changed unexpectedly')
text = text.replace(old_json, new_json, 1).replace(old_text, new_text, 1)
p.write_text(text)
