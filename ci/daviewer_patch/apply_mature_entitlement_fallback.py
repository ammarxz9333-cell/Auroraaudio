from pathlib import Path

# Targets:
#   DAViewer 0.5.0+212 @ 0a6cdd47c96d7cbb27bcd7a1a39d9fb560c93100
#   DAKit main       @ f226cef261a5fa5e4030d84ceb3f55aac1c86afa
#
# Keep current auth/session code untouched. Add only website hydration for
# mature/multi-image/entitled premium media.

dakit_path = Path("DAKit/packages/dakit_web/lib/src/deviation_init.dart")
dakit = dakit_path.read_text()

if "import 'web_deviation_mapper.dart';" not in dakit:
    anchor = "import 'web_session_options.dart';\n"
    if anchor not in dakit:
        raise SystemExit("DAKit import anchor changed")
    dakit = dakit.replace(
        anchor,
        anchor + "import 'web_deviation_mapper.dart';\n",
        1,
    )

ctor_old = """    this.publishedAt,
    this.updatedAt,
  });"""
ctor_new = """    this.publishedAt,
    this.updatedAt,
    this.artwork,
  });"""
if ctor_old not in dakit:
    raise SystemExit("DeviationInit constructor anchor changed")
dakit = dakit.replace(ctor_old, ctor_new, 1)

field_old = """  final DateTime? updatedAt;
}"""
field_new = """  final DateTime? updatedAt;

  /// Website-mapped artwork. Access gates from DeviantArt are preserved by
  /// WebDeviationMapper (purchaseRequired/restricted/loginRequired).
  final Artwork? artwork;
}"""
if field_old not in dakit:
    raise SystemExit("DeviationInit field anchor changed")
dakit = dakit.replace(field_old, field_new, 1)

return_old = """    return DeviationInit(
      uuid: uuid,
      description: description,"""
return_new = """    return DeviationInit(
      uuid: uuid,
      artwork: WebDeviationMapper.mapDeviation(
        Map<Object?, Object?>.from(deviation),
      ),
      description: description,"""
if return_old not in dakit:
    raise SystemExit("DeviationInit return anchor changed")
dakit = dakit.replace(return_old, return_new, 1)
dakit_path.write_text(dakit)

# Focused entitlement tests for the website mapper as consumed through init.
test_path = Path("DAKit/packages/dakit_web/test/mature_entitlement_mapping_test.dart")
test_path.write_text(r"""import 'package:dakit_core/dakit_core.dart';
import 'package:dakit_web/dakit_web.dart';
import 'package:test/test.dart';

Map<String, Object?> payload({
  bool? premiumHasAccess,
  String? tierAccess,
}) =>
    <String, Object?>{
      'deviation': <String, Object?>{
        'deviationId': '123456',
        'title': 'Mature test',
        'url': 'https://www.deviantart.com/test/art/mature-test-123456',
        'isMature': true,
        'isDownloadable': false,
        'filetype': 'jpg',
        'author': <String, Object?>{
          'userId': 'u1',
          'username': 'test',
        },
        'media': <String, Object?>{
          'baseUri': 'https://images.example.test/test.jpg',
          'prettyName': 'test',
          'token': <Object?>[],
          'types': <Object?>[
            <String, Object?>{
              't': 'fullview',
              'c': '/v1/fill/w_1200,h_800,q_70,strp/<prettyName>-pre.jpg',
              'w': 1200,
              'h': 800,
            },
          ],
        },
        if (premiumHasAccess != null)
          'premiumFolderData': <String, Object?>{
            'hasAccess': premiumHasAccess,
          },
        if (tierAccess != null) 'tierAccess': tierAccess,
        'extended': <String, Object?>{
          'deviationUuid': '11111111-2222-3333-4444-555555555555',
          'additionalMedia': <Object?>[],
        },
      },
    };

void main() {
  test('mature website artwork is exposed when no view gate exists', () {
    final artwork = DeviationInitFetcher.parseInit(payload()).artwork!;
    expect(artwork.isMature, isTrue);
    expect(
      artwork.media.any(
        (asset) =>
            asset.uri != null &&
            asset.availability == MediaAvailability.available,
      ),
      isTrue,
    );
    expect(
      artwork.downloadAvailability,
      isNot(MediaAvailability.purchaseRequired),
    );
  });

  test('purchased premium gallery is not marked purchase-required', () {
    final artwork = DeviationInitFetcher.parseInit(
      payload(premiumHasAccess: true),
    ).artwork!;
    expect(
      artwork.downloadAvailability,
      isNot(MediaAvailability.purchaseRequired),
    );
  });

  test('unowned premium gallery remains purchase-required', () {
    final artwork = DeviationInitFetcher.parseInit(
      payload(premiumHasAccess: false),
    ).artwork!;
    expect(
      artwork.downloadAvailability,
      MediaAvailability.purchaseRequired,
    );
  });

  test('locked subscription tier remains purchase-required', () {
    final artwork = DeviationInitFetcher.parseInit(
      payload(tierAccess: 'locked'),
    ).artwork!;
    expect(
      artwork.downloadAvailability,
      MediaAvailability.purchaseRequired,
    );
  });
}
""")

viewer_path = Path("DAViewer/lib/features/artwork/artwork_detail_providers.dart")
viewer = viewer_path.read_text()

old_init = """final deviationInitProvider = FutureProvider.autoDispose
    .family<DeviationInit?, String>((ref, artworkId) async {
      if (!isNumericDeviationId(artworkId)) return null;
      var csrf = ref.watch(
        webSessionControllerProvider.select((web) => web.csrf),
      );
      if (csrf.isEmpty) {
        await ref.read(webSessionRefresherProvider).refresh();
        csrf = ref.read(webSessionControllerProvider).csrf;
      }
      if (csrf.isEmpty) {
        throw StateError('Public browser session is unavailable');
      }
      final webSession = ref.read(webSessionProvider);
      final cookieHeader = await webSession.cookieHeader();
      final cached = ref.read(artworkStoreProvider)[artworkId];
      final username =
          cached?.author.username ?? ref.read(linkUsernameProvider) ?? '';
      final runtime = ref.watch(runtimeProvider);
      return DeviationInitFetcher(runtime.dio!).fetch(
        deviationId: artworkId,
        username: username,
        cookieHeader: cookieHeader,
        csrfToken: csrf,
      );
    });

"""
new_init = """final deviationInitProvider = FutureProvider.autoDispose
    .family<DeviationInit?, String>((ref, artworkId) async {
      final cached = ref.read(artworkStoreProvider)[artworkId];
      final numericId = isNumericDeviationId(artworkId)
          ? artworkId
          : RegExp(r'-(\\d+)/?$')
                .firstMatch(cached?.pageUri.path ?? '')
                ?.group(1);
      if (numericId == null) return null;

      var csrf = ref.watch(
        webSessionControllerProvider.select((web) => web.csrf),
      );
      if (csrf.isEmpty) {
        await ref.read(webSessionRefresherProvider).refresh();
        csrf = ref.read(webSessionControllerProvider).csrf;
      }
      if (csrf.isEmpty) {
        throw StateError('Public browser session is unavailable');
      }
      final webSession = ref.read(webSessionProvider);
      final cookieHeader = await webSession.cookieHeader();
      final username =
          cached?.author.username ?? ref.read(linkUsernameProvider) ?? '';
      final runtime = ref.watch(runtimeProvider);
      return DeviationInitFetcher(runtime.dio!).fetch(
        deviationId: numericId,
        username: username,
        cookieHeader: cookieHeader,
        csrfToken: csrf,
      );
    });

"""
if old_init not in viewer:
    raise SystemExit("DAViewer deviationInitProvider anchor changed")
viewer = viewer.replace(old_init, new_init, 1)

old_detail = """final artworkDetailProvider = FutureProvider.autoDispose
    .family<Artwork, String>((ref, artworkId) async {
      final cached = ref.read(artworkStoreProvider)[artworkId];
      if (cached != null) {
        // Viewing a work is an interest signal for the recommended tags.
        unawaited(InterestStore.recordTags(cached.tags));
        return cached;
      }
      final runtime = ref.watch(runtimeProvider);
      // Numeric website ids (pasted links) must be resolved to the OAuth UUID
      // first — the official deviation/{id} endpoint rejects numeric ids with
      // "api endpoint not found". Feed items skip this because they are cached.
      final uuid = await ref.watch(artworkUuidProvider(artworkId).future);
      final artwork = await dataAccessFor(runtime).artworkById(uuid);
      ref.read(artworkStoreProvider.notifier).putAll(<Artwork>[artwork]);
      unawaited(InterestStore.recordTags(artwork.tags));
      return artwork;
    });

"""
new_detail = """bool _isExplicitViewGate(MediaAvailability availability) =>
    availability == MediaAvailability.purchaseRequired ||
    availability == MediaAvailability.restricted ||
    availability == MediaAvailability.loginRequired;

bool _websiteArtworkIsViewable(Artwork artwork) {
  if (artwork.media.isEmpty) return false;
  if (_isExplicitViewGate(artwork.downloadAvailability)) return false;
  return !artwork.media.any(
    (asset) => _isExplicitViewGate(asset.availability),
  );
}

List<MediaAsset> _mergedWebsiteMedia(
  Artwork websiteArtwork,
  DeviationInit init,
) {
  final byId = <String, MediaAsset>{};
  for (final asset in websiteArtwork.media) {
    byId[asset.id] = asset;
  }
  for (final asset in init.additionalMedia) {
    byId[asset.id] = asset;
  }
  return List<MediaAsset>.unmodifiable(byId.values);
}

final artworkDetailProvider = FutureProvider.autoDispose
    .family<Artwork, String>((ref, artworkId) async {
      final cached = ref.read(artworkStoreProvider)[artworkId];
      if (cached != null) {
        var artwork = cached;
        final shouldTryWebsite =
            cached.isMature ||
            cached.isMultiMedia ||
            cached.media.any(
              (asset) =>
                  asset.availability != MediaAvailability.available,
            );

        if (shouldTryWebsite) {
          try {
            final init = await ref.watch(
              deviationInitProvider(artworkId).future,
            );
            final websiteArtwork = init?.artwork;
            if (init != null &&
                websiteArtwork != null &&
                _websiteArtworkIsViewable(websiteArtwork)) {
              final websiteMedia = _mergedWebsiteMedia(
                websiteArtwork,
                init,
              );
              if (websiteMedia.isNotEmpty) {
                artwork = cached.copyWith(
                  media: websiteMedia,
                  isMature: cached.isMature || websiteArtwork.isMature,
                  isMultiMedia:
                      websiteArtwork.isMultiMedia ||
                      init.additionalMedia.isNotEmpty,
                  isDownloadable:
                      cached.isDownloadable ||
                      websiteArtwork.isDownloadable,
                  downloadAvailability:
                      websiteArtwork.downloadAvailability ==
                              MediaAvailability.available
                          ? MediaAvailability.available
                          : cached.downloadAvailability,
                );
                ref
                    .read(artworkStoreProvider.notifier)
                    .putAll(<Artwork>[artwork]);
              }
            }
          } on Object catch (error) {
            debugPrint(
              '[artwork] website mature/entitlement fallback failed '
              'for $artworkId: $error',
            );
          }
        }

        unawaited(InterestStore.recordTags(artwork.tags));
        return artwork;
      }

      final runtime = ref.watch(runtimeProvider);
      DeviationInit? webInit;
      if (isNumericDeviationId(artworkId)) {
        try {
          webInit = await ref.watch(deviationInitProvider(artworkId).future);
        } on Object catch (error) {
          debugPrint('[artwork] prefetch website init failed: $error');
        }
      }

      final uuid = webInit?.uuid ??
          await ref.watch(artworkUuidProvider(artworkId).future);
      try {
        final artwork = await dataAccessFor(runtime).artworkById(uuid);
        ref.read(artworkStoreProvider.notifier).putAll(<Artwork>[artwork]);
        unawaited(InterestStore.recordTags(artwork.tags));
        return artwork;
      } on Object catch (officialError) {
        final websiteArtwork = webInit?.artwork;
        if (webInit != null &&
            websiteArtwork != null &&
            _websiteArtworkIsViewable(websiteArtwork)) {
          final fallback = websiteArtwork.copyWith(
            media: _mergedWebsiteMedia(websiteArtwork, webInit),
            isMultiMedia:
                websiteArtwork.isMultiMedia ||
                webInit.additionalMedia.isNotEmpty,
          );
          ref.read(artworkStoreProvider.notifier).putAll(<Artwork>[fallback]);
          unawaited(InterestStore.recordTags(fallback.tags));
          return fallback;
        }
        rethrow;
      }
    });

"""
if old_detail not in viewer:
    raise SystemExit("DAViewer artworkDetailProvider anchor changed")
viewer = viewer.replace(old_detail, new_detail, 1)
viewer_path.write_text(viewer)
