import 'package:dakit_flutter/dakit_flutter.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/legacy.dart';

import '../../core/auth/auth_controller.dart';
import '../../core/feed/artwork_feed_controller.dart';
import '../../core/runtime/runtime_provider.dart';

/// An opaque-in-the-UI day cursor. This endpoint exposes a `date` argument,
/// not paginated offsets, so walking backward is required for archival browsing.
///
/// No arbitrary page count is imposed. Stop at the founding year of DeviantArt,
/// since earlier calendar dates cannot contain DeviantArt daily selections.
String? previousDailyCursor(String current) {
  final date = DateTime.tryParse(current);
  if (date == null) {
    throw const FormatException('Invalid daily archive cursor');
  }
  final previous = DateTime.utc(date.year, date.month, date.day)
      .subtract(const Duration(days: 1));
  if (previous.year < 2000) return null;
  return '${previous.year.toString().padLeft(4, '0')}-'
      '${previous.month.toString().padLeft(2, '0')}-'
      '${previous.day.toString().padLeft(2, '0')}';
}

final dailyArchiveProvider =
    StateNotifierProvider<ArtworkFeedController, ArtworkFeedState>((ref) {
  ref.watch(
    authControllerProvider.select((auth) => (auth.status, auth.account?.id)),
  );
  final runtime = ref.watch(runtimeProvider);
  final discovery = OfficialDiscoveryRepository(runtime.transport!);

  return ArtworkFeedController((request) async {
    final now = DateTime.now();
    final today = '${now.year.toString().padLeft(4, '0')}-'
        '${now.month.toString().padLeft(2, '0')}-'
        '${now.day.toString().padLeft(2, '0')}';
    final current = request.cursor ?? today;
    final date = DateTime.tryParse(current);
    if (date == null) {
      throw const FormatException('Invalid daily archive cursor');
    }
    final artworks = await discovery.dailyDeviations(date: date);
    final next = previousDailyCursor(current);
    return Page<Artwork>(
      items: artworks,
      hasMore: next != null,
      nextCursor: next,
    );
  });
});
