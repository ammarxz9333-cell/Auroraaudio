from pathlib import Path

p = Path("DAKit/packages/dakit_api/test/official_repositories_test.dart")
s = p.read_text()

old = """    expect(transport.requests.single.query, <String, Object?>{'seed': 'art-1'});"""
new = """    expect(transport.requests.single.query, <String, Object?>{
      'seed': 'art-1',
      'mature_content': true,
    });"""

if old in s:
    p.write_text(s.replace(old, new, 1))
elif "'mature_content': true" not in s:
    raise SystemExit("more-like-this test expectation anchor changed upstream")
