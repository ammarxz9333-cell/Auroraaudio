from pathlib import Path

# Home: persist the two official discovery feeds.
p = Path("DAViewer/lib/features/home/home_providers.dart")
s = p.read_text()
s = s.replace(
    "import '../../core/feed/artwork_feed_controller.dart';\n",
    "import '../../core/cache/artwork_page_cache.dart';\nimport '../../core/feed/artwork_feed_controller.dart';\n",
    1,
)

old = """final dailyDeviationsProvider = FutureProvider<List<Artwork>>((ref) async {
  ref.watch(
    authControllerProvider.select((auth) => (auth.status, auth.account?.id)),
  );
  final runtime = ref.watch(runtimeProvider);
  return OfficialDiscoveryRepository(runtime.transport!).dailyDeviations();
});"""
new = """final dailyDeviationsProvider = FutureProvider<List<Artwork>>((ref) async {
  final identity = ref.watch(
    authControllerProvider.select((auth) => (auth.status, auth.account?.id)),
  );
  final runtime = ref.watch(runtimeProvider);
  final page = await fetchArtworkPageCached(
    key: 'daily:${identity.$2 ?? 'anonymous'}',
    freshFor: const Duration(minutes: 10),
    fetch: () async {
      final items =
          await OfficialDiscoveryRepository(runtime.transport!).dailyDeviations();
      return Page<Artwork>(items: items, hasMore: false);
    },
  );
  ref.read(artworkStoreProvider.notifier).putAll(page.items);
  return page.items;
});"""
if old not in s:
    raise SystemExit("daily deviations anchor changed")
s = s.replace(old, new, 1)

old = """      ref.watch(
        authControllerProvider.select(
          (auth) => (auth.status, auth.account?.id),
        ),
      );
      final controller = ArtworkFeedController(
        (request) async {
          final page = await OfficialDiscoveryRepository(runtime.transport!)
              .watched(request);
"""
new = """      final identity = ref.watch(
        authControllerProvider.select(
          (auth) => (auth.status, auth.account?.id),
        ),
      );
      final controller = ArtworkFeedController(
        (request) async {
          final page = await fetchArtworkPageCached(
            key:
                'following:${identity.$2 ?? 'anonymous'}:'
                '${request.cursor ?? 'first'}:${request.limit}',
            fetch: () => OfficialDiscoveryRepository(runtime.transport!)
                .watched(request),
          );
          ref.read(artworkStoreProvider.notifier).putAll(page.items);
"""
if old not in s:
    raise SystemExit("following feed anchor changed")
s = s.replace(old, new, 1)
p.write_text(s)

# Search idle tag previews: use already-loaded artwork only (zero network calls).
p = Path("DAViewer/lib/features/search/search_providers.dart")
s = p.read_text()
start = """/// A single representative artwork for a tag, used as the tag's preview image
/// (Pixiv-style). Picks the most popular deviation of the tag so the preview
/// looks curated rather than arbitrary.
final tagPreviewProvider = FutureProvider.autoDispose.family<Artwork?, String>((
  ref,
  tag,
) async {
  final runtime = ref.watch(runtimeProvider);
  try {
    final page = await OfficialDiscoveryRepository(runtime.transport!)
        .tag(tag, const PageRequest(limit: 1), sort: BrowseSort.popular);
    return page.items.isEmpty ? null : page.items.first;
  } on Object {
    return null;
  }
});"""
replacement = """/// A representative artwork for a tag, chosen only from already-loaded local
/// artwork. The search idle screen may show many tags at once; doing one
/// official API request per preview created avoidable request bursts and was a
/// major source of 429 responses.
final tagPreviewProvider = FutureProvider.autoDispose.family<Artwork?, String>((
  ref,
  tag,
) async {
  final normalized = _normalizeTag(tag);
  final artworks = ref.watch(artworkStoreProvider).values.toList(growable: false);
  for (final artwork in artworks.reversed) {
    if (artwork.tags.any((value) => _normalizeTag(value) == normalized)) {
      return artwork;
    }
  }
  return null;
});"""
if start not in s:
    raise SystemExit("tag preview anchor changed")
s = s.replace(start, replacement, 1)
p.write_text(s)

# Tag pages: persistent cached official pages.
p = Path("DAViewer/lib/features/tag/tag_screen.dart")
s = p.read_text()
s = s.replace(
    "import '../../core/feed/artwork_feed_controller.dart';\n",
    "import '../../core/cache/artwork_page_cache.dart';\nimport '../../core/feed/artwork_feed_controller.dart';\n",
    1,
)
s = s.replace(
    "import '../../shared/widgets/artwork_feed_grid.dart';\n",
    "import '../../shared/widgets/artwork_feed_grid.dart';\nimport '../artwork/artwork_store.dart';\n",
    1,
)
old = """      final runtime = ref.watch(runtimeProvider);
      final controller = ArtworkFeedController((request) {
        return OfficialDiscoveryRepository(runtime.transport!)
            .tag(tag, request, sort: sort);
      });
      return controller;"""
new = """      final runtime = ref.watch(runtimeProvider);
      final controller = ArtworkFeedController((request) async {
        final page = await fetchArtworkPageCached(
          key:
              'tag:${tag.toLowerCase()}:${sort.name}:'
              '${request.cursor ?? 'first'}:${request.limit}',
          freshFor: const Duration(minutes: 5),
          fetch: () => OfficialDiscoveryRepository(runtime.transport!)
              .tag(tag, request, sort: sort),
        );
        ref.read(artworkStoreProvider.notifier).putAll(page.items);
        return page;
      });
      return controller;"""
if old not in s:
    raise SystemExit("tag feed anchor changed")
s = s.replace(old, new, 1)
p.write_text(s)

# Artist/folder feeds: persistent cache around official endpoints.
p = Path("DAViewer/lib/features/artist/artist_providers.dart")
s = p.read_text()
s = s.replace(
    "import '../../core/feed/artwork_feed_controller.dart';\n",
    "import '../../core/cache/artwork_page_cache.dart';\nimport '../../core/feed/artwork_feed_controller.dart';\n",
    1,
)

old = """final artistGalleryProvider = StateNotifierProvider.autoDispose
    .family<ArtworkFeedController, ArtworkFeedState, String>((ref, username) {
      final controller = ArtworkFeedController((request) {
        final runtime = ref.read(runtimeProvider);
        return dataAccessFor(runtime).gallery(username, request);
      });
      return controller;
    });"""
new = """final artistGalleryProvider = StateNotifierProvider.autoDispose
    .family<ArtworkFeedController, ArtworkFeedState, String>((ref, username) {
      final controller = ArtworkFeedController((request) async {
        final runtime = ref.read(runtimeProvider);
        final page = await fetchArtworkPageCached(
          key:
              'gallery:${username.toLowerCase()}:'
              '${request.cursor ?? 'first'}:${request.limit}',
          fetch: () => dataAccessFor(runtime).gallery(username, request),
        );
        ref.read(artworkStoreProvider.notifier).putAll(page.items);
        return page;
      });
      return controller;
    });"""
if old not in s:
    raise SystemExit("artist gallery anchor changed")
s = s.replace(old, new, 1)

old = """final artistFavouritesProvider = StateNotifierProvider.autoDispose
    .family<ArtworkFeedController, ArtworkFeedState, String>((ref, username) {
      final controller = ArtworkFeedController((request) {
        final runtime = ref.read(runtimeProvider);
        return dataAccessFor(runtime).favourites(username, request);
      });
      return controller;
    });"""
new = """final artistFavouritesProvider = StateNotifierProvider.autoDispose
    .family<ArtworkFeedController, ArtworkFeedState, String>((ref, username) {
      final controller = ArtworkFeedController((request) async {
        final runtime = ref.read(runtimeProvider);
        final page = await fetchArtworkPageCached(
          key:
              'favourites:${username.toLowerCase()}:'
              '${request.cursor ?? 'first'}:${request.limit}',
          fetch: () => dataAccessFor(runtime).favourites(username, request),
        );
        ref.read(artworkStoreProvider.notifier).putAll(page.items);
        return page;
      });
      return controller;
    });"""
if old not in s:
    raise SystemExit("artist favourites anchor changed")
s = s.replace(old, new, 1)

old = """      final runtime = ref.watch(runtimeProvider);
      final controller = ArtworkFeedController((page) {
        final repository = OfficialFolderRepository(runtime.transport!);
        return request.kind == FolderKind.collection
            ? repository.collectionContents(
                request.folderId,
                username: request.username,
                request: page,
              )
            : repository.galleryContents(
                request.folderId,
                username: request.username,
                request: page,
              );
      });
      return controller;"""
new = """      final runtime = ref.watch(runtimeProvider);
      final controller = ArtworkFeedController((pageRequest) async {
        final repository = OfficialFolderRepository(runtime.transport!);
        final page = await fetchArtworkPageCached(
          key:
              'folder:${request.kind.name}:${request.username.toLowerCase()}:'
              '${request.folderId}:${pageRequest.cursor ?? 'first'}:'
              '${pageRequest.limit}',
          fetch: () => request.kind == FolderKind.collection
              ? repository.collectionContents(
                  request.folderId,
                  username: request.username,
                  request: pageRequest,
                )
              : repository.galleryContents(
                  request.folderId,
                  username: request.username,
                  request: pageRequest,
                ),
        );
        ref.read(artworkStoreProvider.notifier).putAll(page.items);
        return page;
      });
      return controller;"""
if old not in s:
    raise SystemExit("folder contents anchor changed")
s = s.replace(old, new, 1)
p.write_text(s)
