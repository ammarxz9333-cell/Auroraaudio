import 'package:dakit_flutter/dakit_flutter.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/auth/auth_controller.dart';
import '../../core/feed/artwork_feed_controller.dart';
import '../../core/runtime/runtime_provider.dart';
import '../artwork/artwork_store.dart';

/// Official OAuth fallback for Home when a Google/social system-browser login
/// cannot populate the embedded WebView's cookie jar.
///
/// The normal website-personalized feed remains preferred whenever the web
/// Cookie/CSRF session exists. This provider only keeps Home usable for a valid
/// OAuth session without changing the rest of the app.
final oauthHomeFeedProvider =
    StateNotifierProvider<ArtworkFeedController, ArtworkFeedState>((ref) {
      final runtime = ref.watch(runtimeProvider);
      ref.watch(
        authControllerProvider.select(
          (auth) => (auth.status, auth.account?.id),
        ),
      );
      return ArtworkFeedController(
        (request) async {
          final page =
              await OfficialArtworkRepository(runtime.transport!).browse(request);
          ref.read(artworkStoreProvider.notifier).putAll(page.items);
          return page;
        },
        pageSize: 50,
      );
    });
