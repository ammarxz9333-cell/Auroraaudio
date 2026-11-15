// ignore_for_file: prefer_interpolation_to_compose_strings, prefer_initializing_formals
import 'dart:convert';

import 'package:dio/dio.dart';

import 'gallery_catalog.dart';

/// Reddit requires an approved OAuth app and explicit Data API access.
/// NEVER embed a personal bearer token or Reddit client secret in an APK.
/// The caller owns OAuth/PKCE, token refresh and secure session storage.
final class RedditGallerySearch {
  RedditGallerySearch({
    required Future<String?> Function() accessToken,
    required String userAgent,
    Dio? dio,
  }) : _accessToken = accessToken,
       _userAgent = userAgent,
       _dio = dio ??
           Dio(
             BaseOptions(
               connectTimeout: const Duration(seconds: 12),
               receiveTimeout: const Duration(seconds: 20),
             ),
           );

  final Future<String?> Function() _accessToken;
  final String _userAgent;
  final Dio _dio;

  Uri searchUri(String query, {String? after}) {
    final text = query.trim();
    if (text.isEmpty) throw ArgumentError.value(query, 'query');
    final params = <String, String>{
      'q': text,
      'sort': 'relevance',
      'type': 'link',
      'limit': '50',
      'raw_json': '1',
    };
    if (after != null && after.isNotEmpty) params['after'] = after;
    return Uri.https('oauth.reddit.com', '/search', params);
  }

  Future<RedditGalleryPage> search(String query, {String? after}) async {
    final token = await _accessToken();
    if (token == null || token.isEmpty) {
      throw StateError('Reddit OAuth access required and not configured');
    }
    if (!_userAgent.startsWith('android:') ||
        !_userAgent.contains('(by /u/')) {
      throw StateError('Configure a truthful, Reddit-compliant user agent');
    }
    final response = await _dio.getUri<dynamic>(
      searchUri(query, after: after),
      options: Options(headers: <String, String>{
        'Authorization': 'Bearer ' + token,
        'User-Agent': _userAgent,
        'Accept': 'application/json',
      }),
    );
    return parsePayload(response.data);
  }

  /// Parse one search listing. We filter *post type*, not keyword alone:
  /// exclude videos, animated GIFs, text posts, crossposts without media,
  /// unsafe/NSFW or spoiler entries and arbitrary off-site links.
  /// Only Reddit-hosted images and Reddit gallery photo entries are accepted.
  RedditGalleryPage parsePayload(dynamic raw) {
    dynamic json = raw;
    if (json is String) json = jsonDecode(json);
    if (json is! Map) return const RedditGalleryPage(items: [], after: null);
    final listing = json['data'];
    if (listing is! Map) {
      return const RedditGalleryPage(items: [], after: null);
    }
    final children = listing['children'];
    if (children is! List) {
      return const RedditGalleryPage(items: [], after: null);
    }
    final results = <GalleryItem>[];
    for (final wrapper in children) {
      if (wrapper is! Map) continue;
      final data = wrapper['data'];
      if (data is! Map) continue;
      results.addAll(_fromPost(Map<String, dynamic>.from(data)));
    }
    return RedditGalleryPage(
      items: results,
      after: listing['after'] is String ? listing['after'] as String : null,
    );
  }

  List<GalleryItem> _fromPost(Map<String, dynamic> post) {
    if (post['over_18'] == true ||
        post['spoiler'] == true ||
        post['is_video'] == true ||
        post['is_self'] == true) {
      return const <GalleryItem>[];
    }
    final id = post['id']?.toString();
    final permalink = post['permalink']?.toString();
    if (id == null || id.isEmpty || permalink == null ||
        !permalink.startsWith('/r/')) {
      return const <GalleryItem>[];
    }
    final page = Uri.https('www.reddit.com', permalink);
    final title = _asText(post['title']) ?? 'Reddit image';
    final community = _asText(post['subreddit_name_prefixed']) ??
        _asText(post['subreddit']);

    if (post['is_gallery'] == true) {
      final gallery = post['gallery_data'];
      final metadata = post['media_metadata'];
      if (gallery is! Map || metadata is! Map) {
        return const <GalleryItem>[];
      }
      final items = gallery['items'];
      if (items is! List) return const <GalleryItem>[];
      final results = <GalleryItem>[];
      for (final entry in items) {
        if (entry is! Map) continue;
        final mediaId = entry['media_id']?.toString();
        if (mediaId == null) continue;
        final rawMedia = metadata[mediaId];
        if (rawMedia is! Map) continue;
        final mime = rawMedia['m']?.toString().toLowerCase() ?? '';
        if (!mime.startsWith('image/') ||
            mime == 'image/gif' ||
            mime == 'image/webp' && rawMedia['e'] == 'AnimatedImage') {
          continue;
        }
        final source = rawMedia['s'];
        if (source is! Map) continue;
        final full = _redditMediaUri(source['u']);
        if (full == null) continue;
        final previewArray = rawMedia['p'];
        Uri? preview;
        if (previewArray is List && previewArray.isNotEmpty) {
          final last = previewArray.last;
          if (last is Map) preview = _redditMediaUri(last['u']);
        }
        preview ??= full;
        results.add(GalleryItem(
          id: 'reddit:' + id + ':' + mediaId,
          source: GallerySource.reddit,
          title: title,
          previewUrl: preview,
          fullUrl: full,
          pageUrl: page,
          creator: community,
        ));
      }
      return results;
    }

    final isNativeImage = post['post_hint'] == 'image' ||
        _isRedditImage(post['url_overridden_by_dest']);
    if (!isNativeImage) return const <GalleryItem>[];

    final full = _redditMediaUri(
      post['url_overridden_by_dest'] ?? post['url'],
    );
    if (full == null || !_isStillImage(full)) {
      return const <GalleryItem>[];
    }
    final previews = post['preview'];
    Uri? preview;
    if (previews is Map) {
      final images = previews['images'];
      if (images is List && images.isNotEmpty && images.first is Map) {
        final resolutions = images.first['resolutions'];
        if (resolutions is List && resolutions.isNotEmpty &&
            resolutions.last is Map) {
          preview = _redditMediaUri(resolutions.last['url']);
        }
      }
    }
    return <GalleryItem>[
      GalleryItem(
        id: 'reddit:' + id,
        source: GallerySource.reddit,
        title: title,
        previewUrl: preview ?? full,
        fullUrl: full,
        pageUrl: page,
        creator: community,
      ),
    ];
  }

  static String? _asText(dynamic value) {
    if (value is! String || value.trim().isEmpty) return null;
    return value.trim();
  }

  static bool _isRedditImage(dynamic raw) =>
      _redditMediaUri(raw) != null && _isStillImage(_redditMediaUri(raw)!);

  static bool _isStillImage(Uri uri) {
    final path = uri.path.toLowerCase();
    return path.endsWith('.jpg') ||
        path.endsWith('.jpeg') ||
        path.endsWith('.png') ||
        path.endsWith('.webp') ||
        path.endsWith('.avif');
  }

  static Uri? _redditMediaUri(dynamic raw) {
    if (raw is! String) return null;
    final decoded = raw.replaceAll('&amp;', '&');
    final uri = Uri.tryParse(decoded);
    if (uri == null || uri.scheme != 'https' || uri.userInfo.isNotEmpty) {
      return null;
    }
    if (uri.host != 'i.redd.it' &&
        uri.host != 'preview.redd.it' &&
        uri.host != 'external-preview.redd.it') {
      return null;
    }
    return uri;
  }
}

final class RedditGalleryPage {
  const RedditGalleryPage({required this.items, required this.after});

  final List<GalleryItem> items;
  final String? after;
  bool get hasMore => after != null && after!.isNotEmpty;
}
