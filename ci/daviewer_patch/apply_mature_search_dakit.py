from pathlib import Path

repo = Path("DAKit/packages/dakit_api/lib/src/repositories/official_repositories.dart")
s = repo.read_text()

old = """      query: <String, Object?>{
        'offset': offset,
        'limit': request.limit,
        ...query == null
            ? const <String, Object?>{}
            : <String, Object?>{'q': query},
      },
"""
new = """      query: <String, Object?>{
        'offset': offset,
        'limit': request.limit,
        'mature_content': true,
        ...query == null
            ? const <String, Object?>{}
            : <String, Object?>{'q': query},
      },
"""
if old not in s:
    raise SystemExit("OfficialArtworkRepository._page query changed upstream")
repo.write_text(s.replace(old, new, 1))

test = Path("DAKit/packages/dakit_api/test/official_repositories_test.dart")
s = test.read_text()
s = s.replace(
"""    expect(transport.requests[0].query, <String, Object?>{
      'offset': 0,
      'limit': 20,
    });
    expect(transport.requests[1].query, <String, Object?>{
      'offset': 24,
      'limit': 12,
      'q': 'landscape',
    });
""",
"""    expect(transport.requests[0].query, <String, Object?>{
      'offset': 0,
      'limit': 20,
      'mature_content': true,
    });
    expect(transport.requests[1].query, <String, Object?>{
      'offset': 24,
      'limit': 12,
      'mature_content': true,
      'q': 'landscape',
    });
""",
1,
)
test.write_text(s)
