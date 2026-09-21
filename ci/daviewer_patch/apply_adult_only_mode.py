from pathlib import Path

# 1) Keep the generic feed controller generic, but let production feeds opt
# into strict adult-only pagination. This preserves controller semantics/tests.
p = Path("DAViewer/lib/core/feed/artwork_feed_controller.dart")
s = p.read_text()
imp = "import '../content/adult_content_policy.dart';\n"
if imp not in s:
    anchor = "import 'package:flutter_riverpod/flutter_riverpod.dart';\n"
    if anchor not in s:
        raise SystemExit("feed controller import anchor missing")
    s = s.replace(anchor, anchor + "\n" + imp, 1)

old = """  ArtworkFeedController(this._fetch, {bool autoLoad = true, this.pageSize = 24})
    : super(const ArtworkFeedState(isLoading: true)) {"""
new = """  ArtworkFeedController(
    this._fetch, {
    bool autoLoad = true,
    this.pageSize = 24,
    this.adultOnly = false,
  }) : super(const ArtworkFeedState(isLoading: true)) {"""
if old not in s:
    raise SystemExit("feed controller constructor anchor missing")
s = s.replace(old, new, 1)

old = """  final int pageSize;
  Future<void>? _activeFirstPageFetch;"""
new = """  final int pageSize;
  final bool adultOnly;
  Future<void>? _activeFirstPageFetch;"""
if old not in s:
    raise SystemExit("feed controller fields anchor missing")
s = s.replace(old, new, 1)

old = "      final page = await _fetch(PageRequest(limit: pageSize));"
new = """      final request = PageRequest(limit: pageSize);
      final page = adultOnly
          ? await fetchAdultOnlyPage(_fetch, request)
          : await _fetch(request);"""
if old not in s:
    raise SystemExit("feed first page anchor missing")
s = s.replace(old, new, 1)

old = "      final page = await _fetch(PageRequest(cursor: cursor, limit: pageSize));"
new = """      final request = PageRequest(cursor: cursor, limit: pageSize);
      final page = adultOnly
          ? await fetchAdultOnlyPage(_fetch, request)
          : await _fetch(request);"""
if old not in s:
    raise SystemExit("feed load-more anchor missing")
s = s.replace(old, new, 1)
p.write_text(s)

# 2) Enable adult-only mode on every production ArtworkFeedController.
# Tests construct the controller directly and retain the default false.
for filename in [
    "DAViewer/lib/features/home/home_providers.dart",
    "DAViewer/lib/features/search/search_providers.dart",
    "DAViewer/lib/features/tag/tag_screen.dart",
    "DAViewer/lib/features/artist/artist_providers.dart",
    "DAViewer/lib/features/favourites/favourites_providers.dart",
]:
    p = Path(filename)
    s = p.read_text()

    # Closures that end immediately before returning the controller.
    s = s.replace(
        "      });\n      return controller;",
        "      }, adultOnly: true);\n      return controller;",
    )

    # The watched feed already has pageSize as a named argument.
    s = s.replace(
        "        pageSize: 50,\n      );\n      return controller;",
        "        pageSize: 50,\n        adultOnly: true,\n      );\n      return controller;",
    )

    p.write_text(s)

# Verify all production controller construction sites opted in.
for filename in [
    "DAViewer/lib/features/home/home_providers.dart",
    "DAViewer/lib/features/search/search_providers.dart",
    "DAViewer/lib/features/tag/tag_screen.dart",
    "DAViewer/lib/features/artist/artist_providers.dart",
    "DAViewer/lib/features/favourites/favourites_providers.dart",
]:
    s = Path(filename).read_text()
    starts = s.count("ArtworkFeedController(")
    opted = s.count("adultOnly: true")
    if starts != opted:
        raise SystemExit(
            f"adult-only opt-in mismatch in {filename}: "
            f"{starts} controllers, {opted} opted in"
        )

# 3) Direct Daily feed bypasses ArtworkFeedController.
p = Path("DAViewer/lib/features/home/home_providers.dart")
s = p.read_text()
imp = "import '../../core/content/adult_content_policy.dart';\n"
if imp not in s:
    candidates = [
        "import '../../core/cache/artwork_page_cache.dart';\n",
        "import '../../core/feed/artwork_feed_controller.dart';\n",
    ]
    for anchor in candidates:
        if anchor in s:
            s = s.replace(anchor, imp + anchor, 1)
            break
    else:
        raise SystemExit("home provider import anchor missing")

old = """  ref.read(artworkStoreProvider.notifier).putAll(page.items);
  return page.items;
});"""
new = """  final items = adultOnlyArtworks(page.items);
  ref.read(artworkStoreProvider.notifier).putAll(items);
  return items;
});"""
if old in s:
    s = s.replace(old, new, 1)
else:
    old = """  final runtime = ref.watch(runtimeProvider);
  return OfficialDiscoveryRepository(runtime.transport!).dailyDeviations();
});"""
    new = """  final runtime = ref.watch(runtimeProvider);
  final items =
      await OfficialDiscoveryRepository(runtime.transport!).dailyDeviations();
  final filtered = adultOnlyArtworks(items);
  ref.read(artworkStoreProvider.notifier).putAll(filtered);
  return filtered;
});"""
    if old not in s:
        raise SystemExit("daily provider anchor missing")
    s = s.replace(old, new, 1)
p.write_text(s)

# 4) Search tag preview must not surface a non-adult artwork that happens to be
# present in the generic in-memory store.
p = Path("DAViewer/lib/features/search/search_providers.dart")
s = p.read_text()
imp = "import '../../core/content/adult_content_policy.dart';\n"
if imp not in s:
    anchor = "import '../../core/feed/artwork_feed_controller.dart';\n"
    if anchor not in s:
        raise SystemExit("search import anchor missing")
    s = s.replace(anchor, imp + anchor, 1)

old = """  for (final artwork in artworks.reversed) {
    if (artwork.tags.any((value) => _normalizeTag(value) == normalized)) {
      return artwork;
    }
  }"""
new = """  for (final artwork in artworks.reversed) {
    if (!isAdultOnlyArtwork(artwork)) continue;
    if (artwork.tags.any((value) => _normalizeTag(value) == normalized)) {
      return artwork;
    }
  }"""
if old not in s:
    raise SystemExit("tag preview filter anchor missing")
s = s.replace(old, new, 1)
p.write_text(s)

# 5) Persistent cache stores and restores only adult-only artwork. This removes
# legacy non-adult snapshots after the mode switch.
p = Path("DAViewer/lib/core/cache/artwork_page_cache.dart")
s = p.read_text()
imp = "import '../content/adult_content_policy.dart';\n"
if imp not in s:
    anchor = "import '../diagnostics/app_logger.dart';\n"
    if anchor not in s:
        raise SystemExit("cache import anchor missing")
    s = s.replace(anchor, imp + anchor, 1)

old = """Map<String, Object?> _encodePage(Page<Artwork> page) => <String, Object?>{
  'items': page.items.map(_encodeArtwork).toList(growable: false),"""
new = """Map<String, Object?> _encodePage(Page<Artwork> page) => <String, Object?>{
  'items': page.items
      .where(isAdultOnlyArtwork)
      .map(_encodeArtwork)
      .toList(growable: false),"""
if old not in s:
    raise SystemExit("cache encode anchor missing")
s = s.replace(old, new, 1)

old = """  final items = rawItems
      .map(_decodeArtwork)
      .whereType<Artwork>()
      .toList(growable: false);"""
new = """  final items = rawItems
      .map(_decodeArtwork)
      .whereType<Artwork>()
      .where(isAdultOnlyArtwork)
      .toList(growable: false);"""
if old not in s:
    raise SystemExit("cache decode anchor missing")
s = s.replace(old, new, 1)
p.write_text(s)

# 6) Direct detail navigation is a hard gate. Related artwork and the author's
# rail are filtered in the provider path, while keeping the pure merge helper
# generic so its existing tests/semantics stay intact.
p = Path("DAViewer/lib/features/artwork/artwork_detail_providers.dart")
s = p.read_text()
imp = "import '../../core/content/adult_content_policy.dart';\n"
if imp not in s:
    anchor = "import '../../core/data/data_access.dart';\n"
    if anchor not in s:
        raise SystemExit("detail providers import anchor missing")
    s = s.replace(anchor, imp + anchor, 1)

old = """      if (cached != null) {
        // Viewing a work is an interest signal for the recommended tags.
        unawaited(InterestStore.recordTags(cached.tags));
        return cached;
      }"""
new = """      if (cached != null) {
        if (!isAdultOnlyArtwork(cached)) throw adultOnlyRejectedArtwork();
        // Viewing a work is an interest signal for the recommended tags.
        unawaited(InterestStore.recordTags(cached.tags));
        return cached;
      }"""
if old not in s:
    raise SystemExit("detail cached anchor missing")
s = s.replace(old, new, 1)

old = """      final artwork = await dataAccessFor(runtime).artworkById(uuid);
      ref.read(artworkStoreProvider.notifier).putAll(<Artwork>[artwork]);
      unawaited(InterestStore.recordTags(artwork.tags));
      return artwork;"""
new = """      final artwork = await dataAccessFor(runtime).artworkById(uuid);
      if (!isAdultOnlyArtwork(artwork)) throw adultOnlyRejectedArtwork();
      ref.read(artworkStoreProvider.notifier).putAll(<Artwork>[artwork]);
      unawaited(InterestStore.recordTags(artwork.tags));
      return artwork;"""
if old not in s:
    raise SystemExit("detail fetched anchor missing")
s = s.replace(old, new, 1)

# Filter the two related artwork inputs before the generic merge helper.
old = """        return mergeMoreLikeThisResult(
          official: result,
          webArtworks: webArtworks,
          websiteError: websiteError,
        );"""
new = """        final adultOfficial = MoreLikeThisResult(
          artworks: adultOnlyArtworks(result.artworks),
          featuredInCollections: const <CollectionWithDeviations>[],
          suggestedCollections: const <CollectionWithDeviations>[],
        );
        return mergeMoreLikeThisResult(
          official: adultOfficial,
          webArtworks: adultOnlyArtworks(webArtworks),
          websiteError: websiteError,
        );"""
if old not in s:
    raise SystemExit("more-like-this provider merge anchor missing")
s = s.replace(old, new, 1)

old = """        if (webArtworks.isNotEmpty) {
          return MoreLikeThisResult(artworks: webArtworks);
        }"""
new = """        final adultWeb = adultOnlyArtworks(webArtworks);
        if (adultWeb.isNotEmpty) {
          return MoreLikeThisResult(artworks: adultWeb);
        }"""
if old not in s:
    raise SystemExit("more-like-this fallback anchor missing")
s = s.replace(old, new, 1)

old = """          page.items.where(
            (item) => item.id != artworkId && item.media.isNotEmpty,
          ),"""
new = """          page.items.where(
            (item) => item.id != artworkId && isAdultOnlyArtwork(item),
          ),"""
if old not in s:
    raise SystemExit("more-from-artist anchor missing")
s = s.replace(old, new, 1)
p.write_text(s)

# 7) Collection recommendation rails can contain ordinary cover artwork from
# DeviantArt. Hide them entirely in adult-only mode rather than displaying
# unverified thumbnails.
p = Path("DAViewer/lib/features/artwork/artwork_detail_screen.dart")
s = p.read_text()
s = s.replace("import 'collection_sections.dart';\n", "")
s = s.replace(
    "          FeaturedInCollectionsSection(artworkId: widget.artworkId),\n",
    "",
)
s = s.replace(
    "          SuggestedCollectionsSection(artworkId: widget.artworkId),\n",
    "",
)
p.write_text(s)

# 6) Collection recommendation rails are removed in adult-only mode. Their
# collection covers/contents are not guaranteed to carry enough adult metadata
# for the central Artwork policy to prove them safe.
p = Path("DAViewer/lib/features/artwork/artwork_detail_screen.dart")
s = p.read_text()
s = s.replace("import 'collection_sections.dart';\n", "")
s = s.replace(
    "          FeaturedInCollectionsSection(artworkId: widget.artworkId),\n",
    "",
)
s = s.replace(
    "          SuggestedCollectionsSection(artworkId: widget.artworkId),\n",
    "",
)
p.write_text(s)
