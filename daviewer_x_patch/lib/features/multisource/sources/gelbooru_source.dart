import 'package:dio/dio.dart';

import '../models/external_artwork.dart';
import 'external_art_source.dart';

final class GelbooruSource implements ExternalArtSource {
  GelbooruSource(this._dio);

  final Dio _dio;

  @override
  ExternalSourceKind get kind => ExternalSourceKind.gelbooru;

  @override
  String get label => 'Gelbooru';

  static const _base = 'https://gelbooru.com';

  @override
  Future<ExternalPage> search(
    String query, {
    int page = 1,
    int limit = 30,
    bool safeMode = true,
  }) async {
    final tags = <String>[
      if (query.trim().isNotEmpty) query.trim(),
      if (safeMode) 'rating:general',
    ].join(' ');
    final response = await _dio.get<dynamic>(
      '$_base/index.php',
      queryParameters: <String, Object?>{
        'page': 'dapi',
        's': 'post',
        'q': 'index',
        'json': '1',
        'limit': limit.clamp(1, 100),
        'pid': (page - 1).clamp(0, 1000000),
        'tags': tags,
      },
      options: Options(headers: const <String, String>{
        'User-Agent': 'DAViewer-X/0.1 (multi-source client)',
      }),
    );
    final posts = _extractPosts(response.data);
    final items = posts.map(_mapPost).whereType<ExternalArtwork>().toList();
    return ExternalPage(items: items, hasMore: posts.length >= limit);
  }

  @override
  Future<ExternalArtwork?> getById(String id) async {
    final response = await _dio.get<dynamic>(
      '$_base/index.php',
      queryParameters: <String, Object?>{
        'page': 'dapi',
        's': 'post',
        'q': 'index',
        'json': '1',
        'id': id,
      },
      options: Options(headers: const <String, String>{
        'User-Agent': 'DAViewer-X/0.1 (multi-source client)',
      }),
    );
    final posts = _extractPosts(response.data);
    return posts.isEmpty ? null : _mapPost(posts.first);
  }

  @override
  Future<List<ExternalArtwork>> related(
    ExternalArtwork seed, {
    int limit = 24,
    bool safeMode = true,
  }) async {
    final query = seed.tags.take(3).join(' ');
    final page = await search(query, limit: limit + 1, safeMode: safeMode);
    return page.items.where((item) => item.key != seed.key).take(limit).toList();
  }

  @override
  Future<List<ExternalArtwork>> artistWorks(
    String artist, {
    int limit = 30,
    bool safeMode = true,
  }) async {
    final page = await search(artist, limit: limit, safeMode: safeMode);
    return page.items;
  }

  static List<Map<String, dynamic>> _extractPosts(dynamic data) {
    if (data is List) return data.whereType<Map<String, dynamic>>().toList();
    if (data is Map<String, dynamic>) {
      final post = data['post'];
      if (post is List) return post.whereType<Map<String, dynamic>>().toList();
      if (post is Map<String, dynamic>) return <Map<String, dynamic>>[post];
    }
    return const <Map<String, dynamic>>[];
  }

  ExternalArtwork? _mapPost(Map<String, dynamic> post) {
    final id = post['id']?.toString();
    if (id == null) return null;
    final preview = _uri(post['sample_url']) ??
        _uri(post['file_url']) ??
        _uri(post['preview_url']);
    if (preview == null) return null;
    final original = _uri(post['file_url']) ?? _uri(post['sample_url']);
    final tags = _split(post['tags']);
    final artist = (post['owner']?.toString().trim().isNotEmpty ?? false)
        ? post['owner'].toString()
        : 'unknown';
    return ExternalArtwork(
      source: kind,
      id: id,
      title: 'Gelbooru #$id',
      artist: artist,
      pageUri: Uri.parse('$_base/index.php?page=post&s=view&id=$id'),
      previewUri: preview,
      originalUri: original,
      width: (post['width'] as num?)?.toInt(),
      height: (post['height'] as num?)?.toInt(),
      tags: tags,
      rating: post['rating']?.toString(),
      publishedAt: _date(post['created_at']),
    );
  }

  static DateTime? _date(Object? value) {
    final text = value?.toString() ?? '';
    return DateTime.tryParse(text);
  }

  static Uri? _uri(Object? value) {
    final text = value?.toString().trim() ?? '';
    return text.isEmpty ? null : Uri.tryParse(text);
  }

  static List<String> _split(Object? value) =>
      (value?.toString().trim().isEmpty ?? true)
          ? const <String>[]
          : value.toString().trim().split(RegExp(r'\s+'));
}
