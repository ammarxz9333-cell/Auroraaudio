from pathlib import Path

# 1) Central paged feeds: every ArtworkFeedController page is adult-only.
p = Path("DAViewer/lib/core/feed/artwork_feed_controller.dart")
s = p.read_text()
imp = "import '../content/adult_content_policy.dart';\n"
if imp not in s:
    anchor = "import 'package:flutter_riverpod/flutter_riverpod.dart';\n"
    if anchor not in s:
        raise SystemExit("feed controller import anchor missing")
    s = s.replace(anchor, anchor + "\n" + imp, 1)

old = "      final page = await _fetch(PageRequest(limit: pageSize));"
new = """      final page = await fetchAdultOnlyPage(
        _fetch,
        PageRequest(limit: pageSize),
      );"""
if old not in s:
    raise SystemExit("feed first page anchor missing")
s = s.replace(old, new, 1)

old = "      final page = await _fetch(PageRequest(cursor: cursor, limit: pageSize));"
new = """      final page = await fetchAdultOnlyPage(
        _fetch,
        PageRequest(cursor: cursor, limit: pageSize),
      );"""
if old not in s:
    raise SystemExit("feed load-more anchor missing")
s = s.replace(old, new, 1)
p.write_text(s)

# 2) Store: never retain rejected artwork, even if a provider accidentally
# hands it unfiltered data.
p = Path("DAViewer/lib/features/artwork/artwork_store.dart")
s = p.read_text()
imp = "import '../../core/content/adult_content_policy.dart';\n"
if imp not in s:
    anchor = "import 'package:flutter_riverpod/flutter_riverpod.dart';\n"
    if anchor not in s:
        raise SystemExit("artwork store import anchor missing")
    s = s.replace(anchor, anchor + "\n" + imp, 1)

old = """    for (final artwork in artworks) {
      if (artwork.id.isEmpty) continue;
      final cached = next[artwork.id];
      // List endpoints are allowed to return sparse artwork objects. Never let
      // a later feed refresh erase tags that the canonical detail endpoint has
      // already hydrated.
      next[artwork.id] = mergeArtwork(cached: cached, incoming: artwork);
      if (artwork.tags.isNotEmpty) _resolvedTagIds.add(artwork.id);"""
new = """    for (final artwork in artworks) {
      if (artwork.id.isEmpty) continue;
      final cached = next[artwork.id];
      final merged = mergeArtwork(cached: cached, incoming: artwork);
      if (!isAdultOnlyArtwork(merged)) continue;
      next[artwork.id] = merged;
      if (artwork.tags.isNotEmpty) _resolvedTagIds.add(artwork.id);"""
if old not in s:
    raise SystemExit("artwork store putAll anchor missing")
s = s.replace(old, new, 1)

old = """    final normalized = List<String>.unmodifiable(tags);
    if (_sameStrings(artwork.tags, normalized)) return;
    state = <String, Artwork>{...state, id: artwork.copyWith(tags: normalized)};"""
new = """    final normalized = List<String>.unmodifiable(tags);
    final updated = artwork.copyWith(tags: normalized);
    if (!isAdultOnlyArtwork(updated)) {
      final next = Map<String, Artwork>.of(state)..remove(id);
      _resolvedTagIds.remove(id);
      state = next;
      return;
    }
    if (_sameStrings(artwork.tags, normalized)) return;
    state = <String, Artwork>{...state, id: updated};"""
if old not in s:
    raise SystemExit("artwork store setTags anchor missing")
s = s.replace(old, new, 1)

old = """Artwork mergeArtwork({Artwork? cached, required Artwork incoming}) {
  if (cached == null) return incoming;
  if (incoming.tags.isEmpty && cached.tags.isNotEmpty) {
    return incoming.copyWith(tags: cached.tags);
  }
  return incoming;
}"""
new = """Artwork mergeArtwork({Artwork? cached, required Artwork incoming}) {
  if (cached == null) return incoming;
  final sparse = incoming.tags.isEmpty && incoming.media.isEmpty;
  return incoming.copyWith(
    tags: incoming.tags.isEmpty && cached.tags.isNotEmpty
        ? cached.tags
        : incoming.tags,
    media: incoming.media.isEmpty && cached.media.isNotEmpty
        ? cached.media
        : incoming.media,
    // Sparse list payloads commonly omit mature metadata. Preserve a
    // previously-confirmed mature bit only for that sparse update shape.
    isMature: sparse && cached.isMature ? true : incoming.isMature,
  );
}"""
if old not in s:
    raise SystemExit("artwork merge anchor missing")
s = s.replace(old, new, 1)

p.write_text(s)

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

# Works for the persistent-cache patched version.
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

# 4) Persistent cache: encode only accepted adult artwork and reject stale
# legacy cache entries that no longer satisfy the policy.
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

# 5) Detail / related / artist rails: direct-link navigation and sections must
# not bypass the central policy.
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

old = """  final artworks = (webArtworks.isNotEmpty ? webArtworks : official.artworks)
      .where((artwork) => artwork.media.isNotEmpty)
      .toList(growable: false);"""
new = """  final artworks = (webArtworks.isNotEmpty ? webArtworks : official.artworks)
      .where(isAdultOnlyArtwork)
      .toList(growable: false);"""
if old not in s:
    raise SystemExit("more-like-this merge anchor missing")
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
