from pathlib import Path

p = Path("DAKit/packages/dakit_api/test/official_repositories_test.dart")
s = p.read_text()
old = "expect(transport.requests.single.query, <String, Object?>{'seed': 'art-1'});"
new = """expect(transport.requests.single.query, <String, Object?>{
      'seed': 'art-1',
      'mature_content': true,
    });"""
if old not in s:
    raise SystemExit("more-like-this query expectation anchor changed upstream")
p.write_text(s.replace(old, new, 1))
