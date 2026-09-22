from pathlib import Path

# Patch DAKit web init so DAViewer can consume the website-mapped Artwork
# (including mature/premium access state) without bypassing provider access rules.
dakit_path = Path("DAKit/packages/dakit_web/lib/src/deviation_init.dart")
dakit = dakit_path.read_text()

if "import 'web_deviation_mapper.dart';" not in dakit:
    dakit = dakit.replace(
        "import 'web_session_options.dart';\n",
        "import 'web_session_options.dart';\nimport 'web_deviation_mapper.dart';\n",
    )

ctor_old = """    this.updatedAt,
  });"""
ctor_new = """    this.updatedAt,
    this.artwork,
  });"""
if ctor_old not in dakit:
    raise SystemExit("DeviationInit constructor anchor changed upstream")
dakit = dakit.replace(ctor_old, ctor_new, 1)

field_old = """  final DateTime? updatedAt;
}"""
field_new = """  final DateTime? updatedAt;

  /// Website-mapped artwork with provider access state preserved.
  final Artwork? artwork;
}"""
if field_old not in dakit:
    raise SystemExit("DeviationInit field anchor changed upstream")
dakit = dakit.replace(field_old, field_new, 1)

return_old = """      uuid: uuid,
      description: description,"""
return_new = """      uuid: uuid,
      artwork: WebDeviationMapper.mapDeviation(
        Map<Object?, Object?>.from(deviation),
      ),
      description: description,"""
if return_old not in dakit:
    raise SystemExit("DeviationInit return anchor changed upstream")
dakit = dakit.replace(return_old, return_new, 1)

# Backport website view-gate handling from newer DAKit so paid/tier media
# is never marked viewable unless DeviantArt says the logged-in session has access.
mapper_path = Path("DAKit/packages/dakit_web/lib/src/web_deviation_mapper.dart")
mapper = mapper_path.read_text()
if "final viewGate = _viewGateAvailability(json);" not in mapper:
    mapper = mapper.replace(
        "    final isDownloadable = json['isDownloadable'] == true;\n",
        "    final isDownloadable = json['isDownloadable'] == true;\n"
        "    final viewGate = _viewGateAvailability(json);\n",
        1,
    )
    mapper = mapper.replace(
        "      media: _mediaAssets(media, id, json),\n",
        "      media: _mediaAssets(media, id, json, viewGate),\n",
        1,
    )
    mapper = mapper.replace(
        """      downloadAvailability: isDownloadable
          ? MediaAvailability.available
          : MediaAvailability.unavailable,""",
        """      downloadAvailability: viewGate != MediaAvailability.available
          ? viewGate
          : isDownloadable
          ? MediaAvailability.available
          : MediaAvailability.unavailable,""",
        1,
    )
    mapper = mapper.replace(
        """  static List<MediaAsset> _mediaAssets(
    Map<Object?, Object?> media,
    String id,
    Map<Object?, Object?> json,
  ) {""",
        """  static List<MediaAsset> _mediaAssets(
    Map<Object?, Object?> media,
    String id,
    Map<Object?, Object?> json,
    MediaAvailability viewGate,
  ) {""",
        1,
    )
    # Do not treat the poster as unlocked when the provider reports a paid/tier lock.
    mapper = mapper.replace(
        "          availability: MediaAvailability.available,\n          uri: posterUri,",
        "          availability: viewGate,\n          uri: posterUri,",
        1,
    )
    # Video previews.
    mapper = mapper.replace(
        "            availability: MediaAvailability.available,\n            uri: Uri.tryParse(withWixToken(url, type, tokens)),",
        "            availability: viewGate,\n            uri: Uri.tryParse(withWixToken(url, type, tokens)),",
        1,
    )
    # Video original.
    mapper = mapper.replace(
        """            availability: isDownloadable
                ? MediaAvailability.available
                : MediaAvailability.unavailable,""",
        """            availability: viewGate != MediaAvailability.available
                ? viewGate
                : isDownloadable
                ? MediaAvailability.available
                : MediaAvailability.unavailable,""",
        1,
    )
    # GIF/display previews; there are two occurrences of the old availability.
    mapper = mapper.replace(
        "            availability: MediaAvailability.available,\n            uri: fullUri,",
        "            availability: viewGate,\n            uri: fullUri,",
        1,
    )
    mapper = mapper.replace(
        "              availability: MediaAvailability.available,\n              uri: displayUri,",
        "              availability: viewGate,\n              uri: displayUri,",
        1,
    )
    # Image original.
    mapper = mapper.replace(
        """          availability: isDownloadable
              ? MediaAvailability.available
              : MediaAvailability.unavailable,""",
        """          availability: viewGate != MediaAvailability.available
              ? viewGate
              : isDownloadable
              ? MediaAvailability.available
              : MediaAvailability.unavailable,""",
        1,
    )
    insert_anchor = "  static String _filenameFromUri(Uri uri, String pretty, String filetype) {"
    gate_fn = """  static MediaAvailability _viewGateAvailability(
    Map<Object?, Object?> json,
  ) {
    if (json['isBlocked'] == true || json['isDeleted'] == true) {
      return MediaAvailability.restricted;
    }
    final premium = json['premiumFolderData'] ?? json['premium_folder_data'];
    if (premium is Map && premium['hasAccess'] == false) {
      return MediaAvailability.purchaseRequired;
    }
    final tier = json['tierAccess'] ?? json['tier_access'];
    if (tier == 'locked' || tier == 'locked-subscribed') {
      return MediaAvailability.purchaseRequired;
    }
    return MediaAvailability.available;
  }

"""
    if insert_anchor not in mapper:
        raise SystemExit("WebDeviationMapper insertion anchor changed upstream")
    mapper = mapper.replace(insert_anchor, gate_fn + insert_anchor, 1)
mapper_path.write_text(mapper)
dakit_path.write_text(dakit)

# Add focused access-state tests. These prove that website media can be used
# for mature content when available, while explicit paid/tier locks stay locked.
test_path = Path("DAKit/packages/dakit_web/test/mature_entitlement_mapping_test.dart")
test_path.write_text(r"""import 'package:dakit_core/dakit_core.dart';
import 'package:dakit_web/dakit_web.dart';
import 'package:test/test.dart';

Map<String, Object?> payload({
  bool? premiumHasAccess,
  String? tierAccess,
}) {
  return <String, Object?>{
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
}

void main() {
  test('mature work with premium entitlement maps view media as available', () {
    final init = DeviationInitFetcher.parseInit(
      payload(premiumHasAccess: true),
    );
    final artwork = init.artwork;
    expect(artwork, isNotNull);
    expect(artwork!.isMature, isTrue);
    expect(
      artwork.media.where(
        (asset) =>
            asset.availability == MediaAvailability.purchaseRequired,
      ),
      isEmpty,
    );
    expect(
      artwork.media.any(
        (asset) =>
            asset.uri != null &&
            asset.availability == MediaAvailability.available,
      ),
      isTrue,
    );
  });

  test('premium work without entitlement remains purchase-required', () {
    final init = DeviationInitFetcher.parseInit(
      payload(premiumHasAccess: false),
    );
    final artwork = init.artwork!;
    expect(
      artwork.media.any(
        (asset) =>
            asset.availability == MediaAvailability.purchaseRequired,
      ),
      isTrue,
    );
  });

  test('locked subscription tier remains purchase-required', () {
    final init = DeviationInitFetcher.parseInit(
      payload(tierAccess: 'locked'),
    );
    final artwork = init.artwork!;
    expect(
      artwork.media.any(
        (asset) =>
            asset.availability == MediaAvailability.purchaseRequired,
      ),
      isTrue,
    );
  });
}
""")

# Patch DAViewer detail resolution:
# - derive a numeric deviation id from the page URL even for OAuth UUID items;
# - hydrate mature/locked items from the logged-in website session;
# - only replace media when the website mapper does NOT report a view gate;
# - fall back to website artwork if the official API fails for an accessible
#   mature work.
viewer_path = Path("DAViewer/lib/features/artwork/artwork_detail_providers.dart")
viewer = viewer_path.read_text()

old_deviation = """final deviationInitProvider = FutureProvider.autoDispose
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
      final webSession = ref.watch(webSessionProvider);
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
new_deviation = """final deviationInitProvider = FutureProvider.autoDispose
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
      final webSession = ref.watch(webSessionProvider);
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
if old_deviation not in viewer:
    raise SystemExit("deviationInitProvider anchor changed upstream")
viewer = viewer.replace(old_deviation, new_deviation, 1)

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
                debugPrint(
                  '[artwork] website session hydrated $artworkId '
                  'with ${websiteMedia.length} accessible media assets',
                );
              }
            }
          } on Object catch (error) {
            // Website hydration is a compatibility fallback. Keep the cached
            // official result when the session is stale or the website fails.
            debugPrint(
              '[artwork] website mature/entitlement fallback failed '
              'for $artworkId: $error',
            );
          }
        }

        // Viewing a work is an interest signal for the recommended tags.
        unawaited(InterestStore.recordTags(artwork.tags));
        return artwork;
      }

      final runtime = ref.watch(runtimeProvider);
      DeviationInit? webInit;
      if (isNumericDeviationId(artworkId)) {
        try {
          webInit = await ref.watch(deviationInitProvider(artworkId).future);
        } on Object catch (error) {
          debugPrint('[artwork] prefetch web init failed: $error');
        }
      }

      // Numeric website ids (pasted links) must be resolved to the OAuth UUID
      // first — the official deviation/{id} endpoint rejects numeric ids.
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
          debugPrint(
            '[artwork] official detail failed; using entitled website '
            'fallback for $artworkId: $officialError',
          );
          return fallback;
        }
        rethrow;
      }
    });

"""
if old_detail not in viewer:
    raise SystemExit("artworkDetailProvider anchor changed upstream")
viewer = viewer.replace(old_detail, new_detail, 1)
viewer_path.write_text(viewer)
