from pathlib import Path

# Feed-controller regression fixtures must represent content that adult-only
# mode is expected to keep; the tests are about pagination behavior, not policy.
p = Path("DAViewer/test/artwork_feed_controller_test.dart")
s = p.read_text()
old = """    media: const <MediaAsset>[],
  );"""
new = """    media: <MediaAsset>[
      MediaAsset(
        id: 'art-$index:image',
        kind: MediaKind.image,
        role: MediaRole.preview,
        availability: MediaAvailability.available,
        uri: Uri.parse('https://example.test/art-$index.jpg'),
      ),
    ],
    isMature: true,
    tags: const <String>['explicit'],
  );"""
if old not in s:
    raise SystemExit("feed-controller fixture anchor changed")
p.write_text(s.replace(old, new, 1))

# Tag-screen scrolling fixture.
p = Path("DAViewer/test/tag_screen_test.dart")
s = p.read_text()
old = """        media: const <dakit.MediaAsset>[],
      ),"""
new = """        media: <dakit.MediaAsset>[
          dakit.MediaAsset(
            id: 'art-$index:image',
            kind: dakit.MediaKind.image,
            role: dakit.MediaRole.preview,
            availability: dakit.MediaAvailability.available,
            uri: Uri.parse('https://example.test/art-$index.jpg'),
          ),
        ],
        isMature: true,
        tags: const <String>['explicit'],
      ),"""
if old not in s:
    raise SystemExit("tag-screen fixture anchor changed")
p.write_text(s.replace(old, new, 1))

# Related-art fixtures.
p = Path("DAViewer/test/more_like_this_section_test.dart")
s = p.read_text()
old = """  media: withMedia
      ? <MediaAsset>[
          MediaAsset(
            id: '$id:preview',
            kind: MediaKind.image,
            role: MediaRole.preview,
            availability: MediaAvailability.available,
            uri: Uri.parse('https://images.example.test/$id.jpg'),
          ),
        ]
      : const <MediaAsset>[],
);"""
new = """  media: withMedia
      ? <MediaAsset>[
          MediaAsset(
            id: '$id:preview',
            kind: MediaKind.image,
            role: MediaRole.preview,
            availability: MediaAvailability.available,
            uri: Uri.parse('https://images.example.test/$id.jpg'),
          ),
        ]
      : const <MediaAsset>[],
  isMature: true,
  tags: const <String>['explicit'],
);"""
if old not in s:
    raise SystemExit("more-like-this fixture anchor changed")
p.write_text(s.replace(old, new, 1))

# Artwork-store fixtures keep the original test purpose while satisfying the
# new content contract.
p = Path("DAViewer/test/artwork_store_test.dart")
s = p.read_text()
s = s.replace("  title: 'title',", "  title: 'adult title',", 1)
s = s.replace(
    "  media: const <MediaAsset>[],",
    """  media: <MediaAsset>[
    MediaAsset(
      id: '1:image',
      kind: MediaKind.image,
      role: MediaRole.preview,
      availability: MediaAvailability.available,
      uri: Uri.parse('https://d.test/1.jpg'),
    ),
  ],""",
    1,
)
s = s.replace("  tags: const <String>['a', 'b'],", "  tags: const <String>['a', 'b', 'explicit'],", 1)
s = s.replace("expect(updated.title, 'title');", "expect(updated.title, 'adult title');", 1)
s = s.replace("expect(updated.tags, <String>['a', 'b']);", "expect(updated.tags, <String>['a', 'b', 'explicit']);", 1)
s = s.replace("expect(updated.tags, const <String>['a', 'b']);", "expect(updated.tags, const <String>['a', 'b', 'explicit']);", 1)
p.write_text(s)
