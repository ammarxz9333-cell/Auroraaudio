import 'package:dio/dio.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../core/runtime/runtime_provider.dart';
import '../models/external_artwork.dart';
import '../sources/external_art_source.dart';
import '../sources/gelbooru_source.dart';
import '../sources/pixiv_bridge_source.dart';
import 'multisource_repository.dart';
import 'visual_similarity.dart';

const String pixivBridgeUrl = String.fromEnvironment('PIXIV_BRIDGE_URL');
const String pixivBridgeToken = String.fromEnvironment('PIXIV_BRIDGE_TOKEN');

final externalArtSourcesProvider = Provider<List<ExternalArtSource>>((ref) {
  final runtime = ref.watch(runtimeProvider);
  final dio = runtime.dio ?? Dio();
  return <ExternalArtSource>[
    GelbooruSource(dio),
    if (pixivBridgeUrl.trim().isNotEmpty)
      PixivBridgeSource(
        dio,
        pixivBridgeUrl.trim().replaceAll(RegExp(r'/$'), ''),
        bridgeToken: pixivBridgeToken,
      ),
  ];
});

final multiSourceRepositoryProvider = Provider<MultiSourceRepository>((ref) {
  final runtime = ref.watch(runtimeProvider);
  final dio = runtime.dio ?? Dio();
  return MultiSourceRepository(
    ref.watch(externalArtSourcesProvider),
    visualReranker: VisualSimilarityReranker(
      dio,
      pixivBridgeUrl: pixivBridgeUrl,
      pixivBridgeToken: pixivBridgeToken,
    ),
  );
});

final externalArtworkProvider = FutureProvider.autoDispose
    .family<ExternalArtwork?, (ExternalSourceKind, String)>((ref, key) {
  return ref.watch(multiSourceRepositoryProvider).getById(key.$1, key.$2);
});

final externalRelatedProvider = FutureProvider.autoDispose
    .family<List<ExternalArtwork>, (ExternalSourceKind, String, bool)>((ref, key) async {
  final repo = ref.watch(multiSourceRepositoryProvider);
  final seed = await repo.getById(key.$1, key.$2);
  if (seed == null) return const <ExternalArtwork>[];
  return repo.relatedAcross(seed, safeMode: key.$3);
});

final externalArtistSuggestionsProvider = FutureProvider.autoDispose
    .family<List<ArtistSuggestion>, (ExternalSourceKind, String, bool)>((ref, key) async {
  final repo = ref.watch(multiSourceRepositoryProvider);
  final seed = await repo.getById(key.$1, key.$2);
  if (seed == null) return const <ArtistSuggestion>[];
  return repo.suggestArtists(seed, safeMode: key.$3);
});
