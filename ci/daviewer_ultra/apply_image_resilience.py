from pathlib import Path

root = Path("DAViewer")


def replace_once(rel: str, old: str, new: str) -> None:
    p = root / rel
    text = p.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{rel}: expected 1 match, got {count}")
    p.write_text(text.replace(old, new, 1), encoding="utf-8")


card = root / "lib/shared/widgets/artwork_card.dart"
text = card.read_text(encoding="utf-8")

anchor = """double artworkPreviewAspectRatio(BuildContext context, Artwork artwork) =>
    artworkAspectRatio(
      artwork,
      maxLandscapeAspectRatio: MediaQuery.sizeOf(context).width < 600
          ? 1.6
          : 2.0,
    );

"""
helper = """double artworkPreviewAspectRatio(BuildContext context, Artwork artwork) =>
    artworkAspectRatio(
      artwork,
      maxLandscapeAspectRatio: MediaQuery.sizeOf(context).width < 600
          ? 1.6
          : 2.0,
    );

/// Picks the safest feed thumbnail.
///
/// Official DeviantArt responses commonly list the full-size `content` image
/// before the freely renderable `preview`. Many works are not downloadable,
/// so blindly using the first image can select a gated/unavailable asset and
/// leave the grid blank even though a valid preview exists.
MediaAsset? selectArtworkThumbnail(List<MediaAsset> media) {
  final images = media
      .where((asset) => asset.kind == MediaKind.image && asset.uri != null)
      .toList(growable: false);

  for (final asset in images) {
    if (asset.role == MediaRole.preview &&
        asset.availability == MediaAvailability.available) {
      return asset;
    }
  }
  for (final asset in images) {
    if (asset.availability == MediaAvailability.available) {
      return asset;
    }
  }
  for (final asset in images) {
    if (asset.role == MediaRole.preview) return asset;
  }
  if (images.isNotEmpty) return images.first;

  return media.where((asset) => asset.uri != null).firstOrNull ??
      media.firstOrNull;
}

"""
if text.count(anchor) != 1:
    raise SystemExit(f"artwork card helper anchor mismatch: {text.count(anchor)}")
text = text.replace(anchor, helper, 1)

old_pick = """    final media = artwork.media;
    final image = media.where((m) => m.kind == MediaKind.image).firstOrNull;
    final thumbnail = image ?? media.firstOrNull;
"""
new_pick = """    final media = artwork.media;
    final thumbnail = selectArtworkThumbnail(media);
"""
if text.count(old_pick) != 1:
    raise SystemExit(f"artwork card thumbnail block mismatch: {text.count(old_pick)}")
text = text.replace(old_pick, new_pick, 1)
card.write_text(text, encoding="utf-8")

test = root / "test/artwork_card_layout_test.dart"
s = test.read_text(encoding="utf-8")

fixture_anchor = """Artwork _lockedArtwork() => Artwork(
  id: 'locked',
"""
fixture = """Artwork _officialHomeArtwork() => Artwork(
  id: 'official-home',
  title: 'Official API artwork',
  author: const UserProfile(id: 'artist-id', username: 'artist'),
  pageUri: Uri.parse('https://example.test/art/official-home'),
  media: <MediaAsset>[
    MediaAsset(
      id: 'content',
      kind: MediaKind.image,
      role: MediaRole.preview,
      availability: MediaAvailability.unavailable,
      uri: Uri.parse('https://example.test/gated-content.jpg'),
      width: 2400,
      height: 1600,
    ),
    MediaAsset(
      id: 'preview',
      kind: MediaKind.image,
      role: MediaRole.preview,
      availability: MediaAvailability.available,
      uri: Uri.parse('https://example.test/preview.jpg'),
      width: 800,
      height: 533,
    ),
  ],
);

Artwork _lockedArtwork() => Artwork(
  id: 'locked',
"""
if s.count(fixture_anchor) != 1:
    raise SystemExit(f"artwork card fixture anchor mismatch: {s.count(fixture_anchor)}")
s = s.replace(fixture_anchor, fixture, 1)

main_anchor = """void main() {
  testWidgets('mobile banner previews keep more visual height', (tester) async {
"""
test_case = """void main() {
  test('official API cards prefer an available preview over gated content', () {
    final selected = selectArtworkThumbnail(_officialHomeArtwork().media);

    expect(selected, isNotNull);
    expect(selected!.id, 'preview');
    expect(selected.uri, Uri.parse('https://example.test/preview.jpg'));
  });

  testWidgets('mobile banner previews keep more visual height', (tester) async {
"""
if s.count(main_anchor) != 1:
    raise SystemExit(f"artwork card main anchor mismatch: {s.count(main_anchor)}")
s = s.replace(main_anchor, test_case, 1)
test.write_text(s, encoding="utf-8")

print("Artwork image resilience patch applied")
