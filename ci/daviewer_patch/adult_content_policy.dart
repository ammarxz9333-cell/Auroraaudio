import 'package:dakit_core/dakit_core.dart';

/// Central content policy for DAViewer's adult-only mode.
///
/// DeviantArt's [Artwork.isMature] flag is necessary but not sufficient because
/// "mature" can include non-sexual material. A DeviantArt artwork therefore
/// needs all of the following:
///   * a renderable image,
///   * mature=true,
///   * at least one adult/sexual semantic signal,
///   * no age-risk signal.
///
/// External booru adapters have their own explicit-rating gate before their
/// results ever reach the UI.
const Set<String> adultSemanticTags = <String>{
  'nsfw',
  'explicit',
  'adult',
  'nude',
  'nudity',
  'naked',
  'erotic',
  'erotica',
  'sexual',
  'sex',
  'hentai',
  'porn',
  'pornography',
  'xxx',
  'pinup',
  'pin_up',
  'fetish',
  'bdsm',
  'topless',
  'breast',
  'breasts',
  'boob',
  'boobs',
  'nipple',
  'nipples',
  'genital',
  'genitals',
  'vagina',
  'vulva',
  'penis',
  'cock',
  'blowjob',
  'oral_sex',
  'anal',
  'intercourse',
  'masturbation',
  'lingerie',
  'transgender',
  'trans_woman',
  'transwoman',
  'trans_female',
  'futanari',
};

const Set<String> ageRiskTags = <String>{
  'loli',
  'lolicon',
  'shota',
  'shotacon',
  'child',
  'children',
  'minor',
  'minors',
  'underage',
  'preteen',
  'teen',
  'teenager',
  'young',
  'young_girl',
  'young_boy',
  'young_woman',
  'young_man',
  'toddler',
  'baby',
  'infant',
  'kindergartener',
  'kindergarten',
  'elementary_school',
  'elementary_school_student',
  'middle_school',
  'middle_school_student',
  'schoolgirl',
  'schoolboy',
  'aged_down',
  'age_regression',
  'cub',
};

String normalizeAdultPolicyToken(String value) => value
    .trim()
    .toLowerCase()
    .replaceAll(RegExp(r'[^a-z0-9_\- ]+'), ' ')
    .replaceAll(RegExp(r'[\s\-]+'), '_')
    .replaceAll(RegExp(r'_+'), '_')
    .replaceAll(RegExp(r'^_|_$'), '');

Set<String> _artworkSignals(Artwork artwork) {
  final result = <String>{};
  for (final tag in artwork.tags) {
    final normalized = normalizeAdultPolicyToken(tag);
    if (normalized.isNotEmpty) result.add(normalized);
  }

  void addText(String? value) {
    if (value == null || value.trim().isEmpty) return;
    for (final raw in value.split(RegExp(r'\s+'))) {
      final normalized = normalizeAdultPolicyToken(raw);
      if (normalized.isNotEmpty) result.add(normalized);
    }
    final phrase = normalizeAdultPolicyToken(value);
    if (phrase.isNotEmpty) result.add(phrase);
  }

  addText(artwork.title);
  addText(artwork.description);
  return result;
}

bool hasAgeRiskSignal(Artwork artwork) {
  final signals = _artworkSignals(artwork);
  return signals.any(ageRiskTags.contains);
}

bool hasAdultSemanticSignal(Artwork artwork) {
  final signals = _artworkSignals(artwork);
  return signals.any(adultSemanticTags.contains);
}

bool hasDisplayableImage(Artwork artwork) => artwork.media.any(
  (asset) =>
      asset.kind == MediaKind.image &&
      asset.availability == MediaAvailability.available &&
      asset.uri != null,
);

bool isAdultOnlyArtwork(Artwork artwork) {
  if (!hasDisplayableImage(artwork)) return false;
  if (!artwork.isMature) return false;
  if (hasAgeRiskSignal(artwork)) return false;
  return hasAdultSemanticSignal(artwork);
}

List<Artwork> adultOnlyArtworks(Iterable<Artwork> artworks) =>
    List<Artwork>.unmodifiable(artworks.where(isAdultOnlyArtwork));

/// Fetches enough source pages to avoid an apparently-empty feed when the
/// first source page contains mostly non-adult material.
///
/// The cursor returned is always the cursor after the last source page we
/// inspected, so filtered-out pages are never fetched repeatedly.
Future<Page<Artwork>> fetchAdultOnlyPage(
  Future<Page<Artwork>> Function(PageRequest request) fetch,
  PageRequest request, {
  int maxSourcePages = 4,
}) async {
  var cursor = request.cursor;
  var nextCursor = request.cursor;
  var hasMore = true;
  final accepted = <Artwork>[];
  final seenCursors = <String?>{cursor};

  for (var pageIndex = 0;
      pageIndex < maxSourcePages && hasMore;
      pageIndex += 1) {
    final page = await fetch(
      PageRequest(cursor: cursor, limit: request.limit),
    );
    accepted.addAll(page.items.where(isAdultOnlyArtwork));
    hasMore = page.hasMore;
    nextCursor = page.nextCursor;

    if (accepted.length >= request.limit || !hasMore || nextCursor == null) {
      break;
    }
    if (!seenCursors.add(nextCursor)) {
      hasMore = false;
      nextCursor = null;
      break;
    }
    cursor = nextCursor;
  }

  return Page<Artwork>(
    items: List<Artwork>.unmodifiable(accepted),
    hasMore: hasMore && nextCursor != null,
    nextCursor: hasMore ? nextCursor : null,
  );
}

DAKitException adultOnlyRejectedArtwork() => const DAKitException(
  kind: DAKitFailureKind.restricted,
  code: 'app.adult_only.rejected',
  message: 'This artwork is not available in adult-only mode.',
);
