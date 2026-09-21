import 'package:dakit_flutter/dakit_flutter.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/auth/auth_controller.dart';
import '../../core/feed/artwork_feed_controller.dart';
import '../../core/runtime/runtime_provider.dart';
import '../artwork/artwork_store.dart';

/// OAuth-only fallback for the Home/For You tab.
///
/// A Google sign-in completed in the system browser cannot copy Chrome's
/// deviantart.com cookies into Android WebView. Rather than treating that as a
/// broken login, keep Home usable through the official OAuth browse/home feed.
/// This is not byte-for-byte identical to the website-only rfy/deviations feed,
/// but it preserves continuous browsing until a web Cookie/CSRF session exists.
final oauthHomeFeedProvider =
    StateNotifierProvider<ArtworkFeedController, ArtworkFeedState>((ref) {
      final runtime = ref.watch(runtimeProvider);
      ref.watch(
        authControllerProvider.select(
          (auth) => (auth.status, auth.account?.id),
        ),
      );
      return ArtworkFeedController((request) async {
        final page = await OfficialArtworkRepository(
          runtime.transport!,
        ).browse(request);
        ref.read(artworkStoreProvider.notifier).putAll(page.items);
        return page;
      }, pageSize: 50);
    });
