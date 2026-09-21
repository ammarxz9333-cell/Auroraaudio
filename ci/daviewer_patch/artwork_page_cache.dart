import 'dart:convert';
import 'dart:io';

import 'package:dakit_core/dakit_core.dart';
import 'package:path_provider/path_provider.dart';

import '../diagnostics/app_logger.dart';

/// Persistent, account-scoped cache for artwork pages.
///
/// This is deliberately small and JSON-only. It reduces repeated DeviantArt
/// requests across tab switches and app restarts, and lets feeds continue from
/// the last successful snapshot when the provider is temporarily throttling.
final class ArtworkPageCacheStore {
  const ArtworkPageCacheStore._();

  static const Duration defaultFreshFor = Duration(minutes: 2);
  static const Duration defaultStaleFor = Duration(days: 7);

  static Future<_CachedArtworkPage?> load(
    String key, {
    Directory? directory,
  }) async {
    try {
      final file = await _file(key, directory);
      if (!await file.exists()) return null;
      final decoded = jsonDecode(await file.readAsString());
      if (decoded is! Map<Object?, Object?>) return null;
      final savedAtRaw = decoded['savedAt'];
      final savedAt = savedAtRaw is String ? DateTime.tryParse(savedAtRaw) : null;
      final pageRaw = decoded['page'];
      if (savedAt == null || pageRaw is! Map<Object?, Object?>) return null;
      final page = _decodePage(pageRaw);
      if (page == null) return null;
      return _CachedArtworkPage(page: page, savedAt: savedAt.toUtc());
    } on Object catch (error, stack) {
      AppLogger.instance.warning(
        'feed-cache',
        'failed to read cached artwork page',
        error,
        stack,
      );
      return null;
    }
  }

  static Future<void> save(
    String key,
    Page<Artwork> page, {
    Directory? directory,
  }) async {
    try {
      final file = await _file(key, directory);
      await file.parent.create(recursive: true);
      final temporary = File('${file.path}.tmp');
      await temporary.writeAsString(
        jsonEncode(<String, Object?>{
          'savedAt': DateTime.now().toUtc().toIso8601String(),
          'page': _encodePage(page),
        }),
      );
      await temporary.rename(file.path);
    } on Object catch (error, stack) {
      AppLogger.instance.warning(
        'feed-cache',
        'failed to save artwork page',
        error,
        stack,
      );
    }
  }

  static Future<void> clear({Directory? directory}) async {
    try {
      final dir = directory ?? await getApplicationSupportDirectory();
      final cacheDir = Directory(
        '${dir.path}${Platform.pathSeparator}artwork_page_cache',
      );
      if (await cacheDir.exists()) {
        await cacheDir.delete(recursive: true);
      }
    } on Object catch (error, stack) {
      AppLogger.instance.warning(
        'feed-cache',
        'failed to clear artwork page cache',
        error,
        stack,
      );
    }
  }

  static Future<File> _file(String key, Directory? directory) async {
    final dir = directory ?? await getApplicationSupportDirectory();
    final cacheDir = Directory(
      '${dir.path}${Platform.pathSeparator}artwork_page_cache',
    );
    final hash = _fnv1a64(key);
    return File(
      '${cacheDir.path}${Platform.pathSeparator}${hash.toRadixString(16)}.json',
    );
  }

  static int _fnv1a64(String value) {
    var hash = 0xcbf29ce484222325;
    const prime = 0x100000001b3;
    const mask = 0x7fffffffffffffff;
    for (final byte in utf8.encode(value)) {
      hash ^= byte;
      hash = (hash * prime) & mask;
    }
    return hash;
  }
}

final class _CachedArtworkPage {
  const _CachedArtworkPage({required this.page, required this.savedAt});

  final Page<Artwork> page;
  final DateTime savedAt;

  Duration age(DateTime now) {
    final value = now.toUtc().difference(savedAt);
    return value.isNegative ? Duration.zero : value;
  }
}

/// Fetches a page with a short persistent-cache fast path and stale fallback.
///
/// The live request is still attempted once the short freshness window expires.
/// Only transient provider/network failures are allowed to fall back to a stale
/// snapshot; authentication/authorization failures remain visible.
Future<Page<Artwork>> fetchArtworkPageCached({
  required String key,
  required Future<Page<Artwork>> Function() fetch,
  Duration freshFor = ArtworkPageCacheStore.defaultFreshFor,
  Duration staleFor = ArtworkPageCacheStore.defaultStaleFor,
}) async {
  final cached = await ArtworkPageCacheStore.load(key);
  final now = DateTime.now().toUtc();
  if (cached != null && cached.age(now) <= freshFor) {
    return cached.page;
  }

  try {
    final page = await fetch();
    await ArtworkPageCacheStore.save(key, page);
    return page;
  } on Object catch (error) {
    if (cached != null &&
        cached.age(now) <= staleFor &&
        _transientProviderFailure(error)) {
      return cached.page;
    }
    rethrow;
  }
}

bool _transientProviderFailure(Object error) {
  if (error is! DAKitException) return true;
  return switch (error.kind) {
    DAKitFailureKind.rateLimit ||
    DAKitFailureKind.network ||
    DAKitFailureKind.upstream => true,
    _ => false,
  };
}

Map<String, Object?> _encodePage(Page<Artwork> page) => <String, Object?>{
  'items': page.items.map(_encodeArtwork).toList(growable: false),
  'hasMore': page.hasMore,
  if (page.nextCursor != null) 'nextCursor': page.nextCursor,
};

Page<Artwork>? _decodePage(Map<Object?, Object?> value) {
  final rawItems = value['items'];
  if (rawItems is! List) return null;
  final items = rawItems
      .map(_decodeArtwork)
      .whereType<Artwork>()
      .toList(growable: false);
  final hasMore = value['hasMore'] == true;
  final nextCursor = value['nextCursor'] is String
      ? value['nextCursor'] as String
      : null;
  if (hasMore && (nextCursor == null || nextCursor.isEmpty)) return null;
  return Page<Artwork>(
    items: items,
    hasMore: hasMore,
    nextCursor: nextCursor,
  );
}

Map<String, Object?> _encodeArtwork(Artwork artwork) => <String, Object?>{
  'id': artwork.id,
  'title': artwork.title,
  'author': _encodeUser(artwork.author),
  'pageUri': artwork.pageUri.toString(),
  'media': artwork.media.map(_encodeMedia).toList(growable: false),
  if (artwork.description != null) 'description': artwork.description,
  if (artwork.publishedAt != null)
    'publishedAt': artwork.publishedAt!.toUtc().toIso8601String(),
  'isMature': artwork.isMature,
  'isDownloadable': artwork.isDownloadable,
  'isFavourited': artwork.isFavourited,
  'isMultiMedia': artwork.isMultiMedia,
  'downloadAvailability': artwork.downloadAvailability.name,
  if (artwork.textContent != null)
    'textContent': <String, Object?>{
      'excerpt': artwork.textContent!.excerpt,
      if (artwork.textContent!.format != null)
        'format': artwork.textContent!.format,
      if (artwork.textContent!.markup != null)
        'markup': artwork.textContent!.markup,
      if (artwork.textContent!.features != null)
        'features': artwork.textContent!.features,
    },
  'tags': artwork.tags,
};

Artwork? _decodeArtwork(Object? raw) {
  if (raw is! Map<Object?, Object?>) return null;
  final id = raw['id'];
  final title = raw['title'];
  final author = _decodeUser(raw['author']);
  final pageUri = raw['pageUri'] is String
      ? Uri.tryParse(raw['pageUri'] as String)
      : null;
  final rawMedia = raw['media'];
  if (id is! String ||
      id.isEmpty ||
      title is! String ||
      author == null ||
      pageUri == null ||
      rawMedia is! List) {
    return null;
  }

  final media = rawMedia.map(_decodeMedia).whereType<MediaAsset>().toList();
  final publishedAt = raw['publishedAt'] is String
      ? DateTime.tryParse(raw['publishedAt'] as String)?.toUtc()
      : null;
  final availability = _enumByName(
    MediaAvailability.values,
    raw['downloadAvailability'],
  );
  final textRaw = raw['textContent'];
  final textContent = textRaw is Map<Object?, Object?> &&
          textRaw['excerpt'] is String
      ? ArtworkTextContent(
          excerpt: textRaw['excerpt'] as String,
          format: textRaw['format'] as String?,
          markup: textRaw['markup'] as String?,
          features: textRaw['features'] as String?,
        )
      : null;

  return Artwork(
    id: id,
    title: title,
    author: author,
    pageUri: pageUri,
    media: media,
    description: raw['description'] as String?,
    publishedAt: publishedAt,
    isMature: raw['isMature'] == true,
    isDownloadable: raw['isDownloadable'] == true,
    isFavourited: raw['isFavourited'] == true,
    isMultiMedia: raw['isMultiMedia'] == true,
    downloadAvailability: availability,
    textContent: textContent,
    tags: (raw['tags'] is List)
        ? (raw['tags'] as List).whereType<String>().toList(growable: false)
        : const <String>[],
  );
}

Map<String, Object?> _encodeUser(UserProfile user) => <String, Object?>{
  'id': user.id,
  'username': user.username,
  if (user.displayName != null) 'displayName': user.displayName,
  if (user.avatarUri != null) 'avatarUri': user.avatarUri.toString(),
  if (user.profileUri != null) 'profileUri': user.profileUri.toString(),
};

UserProfile? _decodeUser(Object? raw) {
  if (raw is! Map<Object?, Object?>) return null;
  final id = raw['id'];
  final username = raw['username'];
  if (id is! String || username is! String) return null;
  return UserProfile(
    id: id,
    username: username,
    displayName: raw['displayName'] as String?,
    avatarUri: raw['avatarUri'] is String
        ? Uri.tryParse(raw['avatarUri'] as String)
        : null,
    profileUri: raw['profileUri'] is String
        ? Uri.tryParse(raw['profileUri'] as String)
        : null,
  );
}

Map<String, Object?> _encodeMedia(MediaAsset media) => <String, Object?>{
  'id': media.id,
  'kind': media.kind.name,
  'role': media.role.name,
  'availability': media.availability.name,
  if (media.uri != null) 'uri': media.uri.toString(),
  if (media.mimeType != null) 'mimeType': media.mimeType,
  if (media.filename != null) 'filename': media.filename,
  if (media.byteLength != null) 'byteLength': media.byteLength,
  if (media.width != null) 'width': media.width,
  if (media.height != null) 'height': media.height,
  if (media.duration != null)
    'durationMs': media.duration!.inMilliseconds,
  if (media.availabilityReason != null)
    'availabilityReason': media.availabilityReason,
};

MediaAsset? _decodeMedia(Object? raw) {
  if (raw is! Map<Object?, Object?>) return null;
  final id = raw['id'];
  final kind = _enumByName(MediaKind.values, raw['kind']);
  final role = _enumByName(MediaRole.values, raw['role']);
  final availability = _enumByName(
    MediaAvailability.values,
    raw['availability'],
  );
  if (id is! String || kind == null || role == null || availability == null) {
    return null;
  }
  return MediaAsset(
    id: id,
    kind: kind,
    role: role,
    availability: availability,
    uri: raw['uri'] is String ? Uri.tryParse(raw['uri'] as String) : null,
    mimeType: raw['mimeType'] as String?,
    filename: raw['filename'] as String?,
    byteLength: raw['byteLength'] as int?,
    width: raw['width'] as int?,
    height: raw['height'] as int?,
    duration: raw['durationMs'] is int
        ? Duration(milliseconds: raw['durationMs'] as int)
        : null,
    availabilityReason: raw['availabilityReason'] as String?,
  );
}

T? _enumByName<T extends Enum>(List<T> values, Object? raw) {
  if (raw is! String) return null;
  for (final value in values) {
    if (value.name == raw) return value;
  }
  return null;
}
