import 'dart:convert';

import 'package:dio/dio.dart';

/// Image providers are independent of DeviantArt OAuth/session state.
///
/// Only publicly documented read endpoints are queried. Providers with no
/// supported public API are represented by external search links in the UI.
enum GallerySource { all, danbooru, safebooru, wallhaven, openverse, reddit, rule34 }

extension GallerySourceDetails on GallerySource {
  String get label => switch (this) {
    GallerySource.all => 'All sources',
    GallerySource.danbooru => 'Danbooru',
    GallerySource.safebooru => 'Safebooru',
    GallerySource.wallhaven => 'Wallhaven',
    GallerySource.openverse => 'Openverse',
    GallerySource.reddit => 'Reddit images',
    GallerySource.rule34 => 'Rule34 (18+)',
  };

  bool get isAdultOnly => this == GallerySource.rule34;
}

const List<GallerySource> safeGallerySources = <GallerySource>[
  GallerySource.danbooru,
  GallerySource.safebooru,
  GallerySource.wallhaven,
  GallerySource.openverse,
];

final class GalleryItem {
  const GalleryItem({
    required this.id,
    required this.source,
    required this.title,
    required this.previewUrl,
    required this.fullUrl,
    required this.pageUrl,
    this.creator,
    this.license,
  });

  final String id;
  final GallerySource source;
  final String title;
  final Uri previewUrl;
  final Uri fullUrl;
  final Uri pageUrl;
  final String? creator;
  final String? license;
}

final class GalleryPage {
  const GalleryPage({required this.items, required this.hasMore});

  final List<GalleryItem> items;
  final bool hasMore;
}

final class GalleryCatalog {
  GalleryCatalog({Dio? dio})
    : _dio =
          dio ??
          Dio(
            BaseOptions(
              connectTimeout: const Duration(seconds: 12),
              receiveTimeout: const Duration(seconds: 20),
              headers: const <String, String>{
                'Accept': 'application/json',
                'User-Agent':
                    'DAViewer-Ultra-Multisource/0.8 (+https://github.com/ammarxz9333-cell/Auroraaudio)',
              },
            ),
          );

  final Dio _dio;
  static const int pageSize = 24;

  Uri buildSearchUri(
    GallerySource source,
    String rawQuery,
    int page, {
    bool adultEnabled = false,
  }) {
    if (source == GallerySource.all) {
      throw ArgumentError('Select a concrete source');
    }
    if (page < 1) {
      throw RangeError.range(page, 1, null, 'page');
    }
    if (source.isAdultOnly && !adultEnabled) {
      throw StateError('Adult source requires explicit opt-in');
    }

    final query = rawQuery.trim().replaceAll(RegExp(r'\s+'), ' ');
    if (query.isEmpty) {
      throw ArgumentError.value(rawQuery, 'rawQuery', 'Empty search');
    }

    switch (source) {
      case GallerySource.danbooru:
        // General-rated posts only. Verified again when decoding the payload.
        return Uri.https('danbooru.donmai.us', '/posts.json', <String, String>{
          'tags': query + ' rating:g',
          'page': page.toString(),
          'limit': pageSize.toString(),
        });
      case GallerySource.safebooru:
      case GallerySource.rule34:
        final host = source == GallerySource.rule34
            ? 'api.rule34.xxx'
            : 'safebooru.org';
        return Uri.https(host, '/index.php', <String, String>{
          'page': 'dapi',
          's': 'post',
          'q': 'index',
          'json': '1',
          'pid': (page - 1).toString(),
          'limit': pageSize.toString(),
          'tags': query,
        });
      case GallerySource.wallhaven:
        return Uri.https('wallhaven.cc', '/api/v1/search', <String, String>{
          'q': query,
          'purity': '100',
          'categories': '111',
          'page': page.toString(),
        });
      case GallerySource.openverse:
        return Uri.https('api.openverse.org', '/v1/images/', <String, String>{
          'q': query,
          'page': page.toString(),
          'page_size': pageSize.toString(),
          'mature': 'false',
        });
      case GallerySource.reddit:
        throw StateError('Reddit requires approved OAuth; use RedditGallerySearch');
      case GallerySource.all:
        throw StateError('Unreachable');
    }
  }

  Future<GalleryPage> search(
    GallerySource source,
    String query,
    int page, {
    bool adultEnabled = false,
  }) async {
    final uri = buildSearchUri(
      source,
      query,
      page,
      adultEnabled: adultEnabled,
    );
    final response = await _dio.getUri<dynamic>(
      uri,
      options: Options(responseType: ResponseType.json),
    );
    return parsePayload(source, response.data, page);
  }

  /// Pure parser allows regression tests with source fixtures, without
  /// contacting external services or requiring a DeviantArt account.
  GalleryPage parsePayload(
    GallerySource source,
    dynamic payload,
    int page,
  ) {
    dynamic data = payload;
    if (data is String) {
      data = jsonDecode(data);
    }
    if (source == GallerySource.all) {
      throw ArgumentError('Select a concrete source');
    }
    final String? listKey = switch (source) {
      GallerySource.openverse => 'results',
      GallerySource.wallhaven => 'data',
      GallerySource.danbooru => null,
      GallerySource.reddit => null,
      GallerySource.safebooru || GallerySource.rule34 => 'post',
      GallerySource.all => null,
    };
    final rows = _getRows(data, key: listKey);
    final items = <GalleryItem>[];
    for (final rawRow in rows) {
      if (rawRow is! Map) continue;
      final row = Map<String, dynamic>.from(rawRow);
      final item = switch (source) {
        GallerySource.danbooru => _parseDanbooru(row),
        GallerySource.safebooru || GallerySource.rule34 =>
          _parseBooru(source, row),
        GallerySource.wallhaven => _parseWallhaven(row),
        GallerySource.openverse => _parseOpenverse(row),
        GallerySource.reddit => null,
        GallerySource.all => null,
      };
      if (item != null) items.add(item);
    }

    var hasMore = rows.length >= pageSize;
    if (source == GallerySource.openverse && data is Map) {
      hasMore = page < _asInt(data['page_count']);
    } else if (source == GallerySource.wallhaven && data is Map) {
      final meta = data['meta'];
      if (meta is Map) {
        hasMore = page < _asInt(meta['last_page']);
      }
    }
    return GalleryPage(items: items, hasMore: hasMore);
  }

  GalleryItem? _parseDanbooru(Map<String, dynamic> row) {
    // Defensive filtering: don't trust the server to apply our search tag.
    if (row['rating'] != 'g') return null;
    final id = row['id']?.toString();
    if (id == null || id.isEmpty) return null;
    final preview =
        _secureUri(row['preview_file_url']) ??
        _secureUri(row['large_file_url']) ??
        _secureUri(row['file_url']);
    final full =
        _secureUri(row['file_url']) ??
        _secureUri(row['large_file_url']) ??
        preview;
    if (preview == null || full == null) return null;
    return GalleryItem(
      id: 'danbooru:' + id,
      source: GallerySource.danbooru,
      title: _title(row['tag_string_general'] ?? row['tag_string'], id),
      previewUrl: preview,
      fullUrl: full,
      pageUrl: Uri.https('danbooru.donmai.us', '/posts/' + id),
      creator: _optionalText(row['tag_string_artist']),
    );
  }

  GalleryItem? _parseBooru(GallerySource source, Map<String, dynamic> row) {
    final id = row['id']?.toString();
    if (id == null || id.isEmpty) return null;
    final preview =
        _secureUri(row['preview_url']) ??
        _secureUri(row['sample_url']) ??
        _secureUri(row['file_url']);
    final full =
        _secureUri(row['file_url']) ??
        _secureUri(row['sample_url']) ??
        preview;
    if (preview == null || full == null) return null;

    final host = source == GallerySource.rule34
        ? 'rule34.xxx'
        : 'safebooru.org';
    return GalleryItem(
      id: source.name + ':' + id,
      source: source,
      title: _title(row['tags'], id),
      previewUrl: preview,
      fullUrl: full,
      pageUrl: Uri.https(host, '/index.php', <String, String>{
        'page': 'post',
        's': 'view',
        'id': id,
      }),
    );
  }

  GalleryItem? _parseWallhaven(Map<String, dynamic> row) {
    if (row['purity'] != 'sfw') return null;
    final id = row['id']?.toString();
    final thumbs = row['thumbs'];
    if (id == null || thumbs is! Map) return null;
    final preview =
        _secureUri(thumbs['small']) ??
        _secureUri(thumbs['large']);
    final full = _secureUri(row['path']);
    if (preview == null || full == null) return null;
    return GalleryItem(
      id: 'wallhaven:' + id,
      source: GallerySource.wallhaven,
      title: row['resolution']?.toString() ?? 'Wallpaper ' + id,
      previewUrl: preview,
      fullUrl: full,
      pageUrl: Uri.https('wallhaven.cc', '/w/' + id),
    );
  }

  GalleryItem? _parseOpenverse(Map<String, dynamic> row) {
    if (row['mature'] == true) return null;
    final id = row['id']?.toString();
    if (id == null || id.isEmpty) return null;
    final preview =
        _secureUri(row['thumbnail']) ?? _secureUri(row['url']);
    final full = _secureUri(row['url']) ?? preview;
    final page =
        _secureUri(row['foreign_landing_url']) ??
        _secureUri(row['detail_url']);
    if (preview == null || full == null || page == null) return null;
    return GalleryItem(
      id: 'openverse:' + id,
      source: GallerySource.openverse,
      title: _optionalText(row['title']) ?? 'Untitled work',
      previewUrl: preview,
      fullUrl: full,
      pageUrl: page,
      creator: _optionalText(row['creator']),
      license: _optionalText(row['license']),
    );
  }

  static List<dynamic> _getRows(dynamic data, {String? key}) {
    if (data is List) return data;
    if (data is Map && key != null) {
      final value = data[key];
      if (value is List) return value;
      if (value is Map) return <dynamic>[value];
    }
    return const <dynamic>[];
  }

  static Uri? _secureUri(dynamic raw) {
    if (raw is! String || raw.isEmpty) return null;
    final uri = Uri.tryParse(raw);
    if (uri == null || uri.scheme != 'https' || uri.host.isEmpty) {
      return null;
    }
    return uri;
  }

  static int _asInt(dynamic value) =>
      value is int ? value : int.tryParse(value?.toString() ?? '') ?? 0;

  static String? _optionalText(dynamic value) {
    final text = value?.toString().trim() ?? '';
    return text.isEmpty ? null : text;
  }

  static String _title(dynamic raw, String fallbackId) {
    final tags = _optionalText(raw);
    if (tags == null) return 'Artwork ' + fallbackId;
    return tags
        .split(RegExp(r'\s+'))
        .where((tag) => tag.isNotEmpty)
        .take(5)
        .map((tag) => tag.replaceAll('_', ' '))
        .join(' · ');
  }
}
