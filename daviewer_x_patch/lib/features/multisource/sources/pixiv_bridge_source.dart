import 'package:dio/dio.dart';

import '../models/external_artwork.dart';
import 'external_art_source.dart';

/// Thin client for the optional Pixiv bridge in `bridge/`.
/// Keeping Pixiv auth out of the APK avoids shipping a refresh token in the app.
final class PixivBridgeSource implements ExternalArtSource {
  PixivBridgeSource(this._dio, this.baseUrl, {this.bridgeToken = ''});

  final Dio _dio;
  final String baseUrl;
  final String bridgeToken;

  Options get _options => Options(
        headers: bridgeToken.isEmpty
            ? null
            : <String, String>{'X-Bridge-Token': bridgeToken},
      );

  @override
  ExternalSourceKind get kind => ExternalSourceKind.pixiv;

  @override
  String get label => 'Pixiv';

  @override
  Future<ExternalPage> search(
    String query, {
    int page = 1,
    int limit = 30,
    bool safeMode = true,
  }) async {
    final response = await _dio.get<Map<String, dynamic>>(
      '$baseUrl/v1/search',
      options: _options,
      queryParameters: <String, Object?>{
        'q': query,
        'page': page,
        'limit': limit,
        'safe': safeMode,
      },
    );
    final items = _items(response.data?['items']);
    return ExternalPage(
      items: items,
      hasMore: response.data?['has_more'] == true,
    );
  }

  @override
  Future<ExternalArtwork?> getById(String id) async {
    final response = await _dio.get<Map<String, dynamic>>(
      '$baseUrl/v1/illust/$id',
      options: _options,
    );
    return _map(response.data);
  }

  @override
  Future<List<ExternalArtwork>> related(
    ExternalArtwork seed, {
    int limit = 24,
    bool safeMode = true,
  }) async {
    final response = await _dio.get<Map<String, dynamic>>(
      '$baseUrl/v1/illust/${seed.id}/related',
      queryParameters: <String, Object?>{'limit': limit, 'safe': safeMode},
      options: _options,
    );
    return _items(response.data?['items']);
  }

  @override
  Future<List<ExternalArtwork>> artistWorks(
    String artist, {
    int limit = 30,
    bool safeMode = true,
  }) async {
    final response = await _dio.get<Map<String, dynamic>>(
      '$baseUrl/v1/user/$artist/illusts',
      queryParameters: <String, Object?>{'limit': limit, 'safe': safeMode},
      options: _options,
    );
    return _items(response.data?['items']);
  }

  List<ExternalArtwork> _items(Object? raw) => (raw is List ? raw : const [])
      .whereType<Map<String, dynamic>>()
      .map(_map)
      .whereType<ExternalArtwork>()
      .toList(growable: false);

  ExternalArtwork? _map(Map<String, dynamic>? json) {
    if (json == null) return null;
    final id = json['id']?.toString();
    final preview = Uri.tryParse(json['preview_url']?.toString() ?? '');
    final page = Uri.tryParse(json['page_url']?.toString() ?? '');
    if (id == null || preview == null || page == null) return null;
    final originalText = json['original_url']?.toString();
    return ExternalArtwork(
      source: kind,
      id: id,
      title: json['title']?.toString() ?? 'Pixiv #$id',
      artist: json['artist_name']?.toString() ?? 'unknown',
      artistId: json['artist_id']?.toString(),
      pageUri: page,
      previewUri: preview,
      originalUri: originalText == null ? null : Uri.tryParse(originalText),
      width: (json['width'] as num?)?.toInt(),
      height: (json['height'] as num?)?.toInt(),
      tags: (json['tags'] as List<dynamic>? ?? const <dynamic>[])
          .map((e) => e.toString())
          .toList(growable: false),
      rating: json['rating']?.toString(),
      publishedAt: DateTime.tryParse(json['published_at']?.toString() ?? ''),
    );
  }
}
