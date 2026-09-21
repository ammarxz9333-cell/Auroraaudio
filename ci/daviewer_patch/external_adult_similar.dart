import 'dart:convert';

import 'package:cached_network_image/cached_network_image.dart';
import 'package:dakit_core/dakit_core.dart';
import 'package:dio/dio.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:url_launcher/url_launcher.dart';

import '../../core/l10n/app_strings.dart';
import '../../core/runtime/runtime_provider.dart';
import 'artwork_detail_providers.dart';

enum ExternalAdultSource { rule34, gelbooru }

final class ExternalAdultPost {
  const ExternalAdultPost({
    required this.source,
    required this.id,
    required this.pageUri,
    required this.previewUri,
    required this.fileUri,
    required this.tags,
    required this.rating,
    this.width,
    this.height,
    this.similarity = 0,
  });

  final ExternalAdultSource source;
  final String id;
  final Uri pageUri;
  final Uri previewUri;
  final Uri fileUri;
  final Set<String> tags;
  final String rating;
  final int? width;
  final int? height;
  final double similarity;

  ExternalAdultPost copyWith({double? similarity}) => ExternalAdultPost(
    source: source,
    id: id,
    pageUri: pageUri,
    previewUri: previewUri,
    fileUri: fileUri,
    tags: tags,
    rating: rating,
    width: width,
    height: height,
    similarity: similarity ?? this.similarity,
  );

  String get sourceLabel => switch (source) {
    ExternalAdultSource.rule34 => 'Rule34',
    ExternalAdultSource.gelbooru => 'Gelbooru',
  };
}

const Set<String> _blockedAgeTags = <String>{
  'loli',
  'lolicon',
  'shota',
  'shotacon',
  'child',
  'children',
  'minor',
  'minors',
  'underage',
  'preteen',
  'teen',
  'teenager',
  'young',
  'young_girl',
  'young_boy',
  'toddler',
  'baby',
  'infant',
  'kindergartener',
  'kindergarten',
  'elementary_school',
  'elementary_school_student',
  'middle_school',
  'middle_school_student',
  'schoolgirl',
  'schoolboy',
  'aged_down',
  'age_regression',
  'cub',
};

const Set<String> _uninformativeSeedTags = <String>{
  'art',
  'artist',
  'digitalart',
  'digital_art',
  'drawing',
  'illustration',
  'nsfw',
  'mature',
  'adult',
  'explicit',
  'deviantart',
};

String normalizeExternalTag(String value) => value
    .trim()
    .toLowerCase()
    .replaceAll(RegExp(r'[^a-z0-9_\- ]+'), '')
    .replaceAll(RegExp(r'[\s\-]+'), '_')
    .replaceAll(RegExp(r'_+'), '_');

List<String> buildExternalAdultSearchTags(Iterable<String> seedTags) {
  final seen = <String>{};
  final result = <String>[];
  for (final raw in seedTags) {
    final tag = normalizeExternalTag(raw);
    if (tag.length < 3 ||
        _uninformativeSeedTags.contains(tag) ||
        _blockedAgeTags.contains(tag) ||
        !seen.add(tag)) {
      continue;
    }
    result.add(tag);
    if (result.length == 3) break;
  }
  return result;
}

bool isStrictAdultExternalPost(ExternalAdultPost post) {
  final rating = post.rating.trim().toLowerCase();
  if (rating != 'e' && rating != 'explicit') return false;
  for (final tag in post.tags) {
    if (_blockedAgeTags.contains(normalizeExternalTag(tag))) return false;
  }
  return true;
}

double externalSimilarityScore({
  required Set<String> seedTags,
  required ExternalAdultPost candidate,
  double? seedAspectRatio,
}) {
  final normalizedSeed = seedTags.map(normalizeExternalTag).where((e) => e.isNotEmpty).toSet();
  final normalizedCandidate =
      candidate.tags.map(normalizeExternalTag).where((e) => e.isNotEmpty).toSet();
  final intersection = normalizedSeed.intersection(normalizedCandidate).length;
  final union = normalizedSeed.union(normalizedCandidate).length;
  final tagScore = union == 0 ? 0.0 : intersection / union;

  var aspectScore = 0.5;
  final width = candidate.width;
  final height = candidate.height;
  if (seedAspectRatio != null &&
      seedAspectRatio > 0 &&
      width != null &&
      height != null &&
      width > 0 &&
      height > 0) {
    final candidateAspect = width / height;
    final high = seedAspectRatio > candidateAspect
        ? seedAspectRatio
        : candidateAspect;
    final low = seedAspectRatio < candidateAspect
        ? seedAspectRatio
        : candidateAspect;
    aspectScore = (low / high).clamp(0.0, 1.0);
  }

  // Tag semantics dominate. Aspect ratio is a low-weight visual/composition
  // signal and helps break ties without downloading every candidate image.
  return (tagScore * 0.85 + aspectScore * 0.15).clamp(0.0, 1.0);
}

final externalAdultSimilarProvider = FutureProvider.autoDispose
    .family<List<ExternalAdultPost>, String>((ref, artworkId) async {
      final artwork = await ref.watch(artworkDetailProvider(artworkId).future);
      final tags = await ref.watch(artworkTagsProvider(artworkId).future);
      final queryTags = buildExternalAdultSearchTags(tags);
      if (queryTags.isEmpty) return const <ExternalAdultPost>[];

      final dio = ref.read(runtimeProvider).dio;
      if (dio == null) return const <ExternalAdultPost>[];
      final client = ExternalAdultArtClient(dio);

      final batches = await Future.wait<List<ExternalAdultPost>>(
        <Future<List<ExternalAdultPost>>>[
          client.search(ExternalAdultSource.rule34, queryTags),
          client.search(ExternalAdultSource.gelbooru, queryTags),
        ],
      );

      double? seedAspect;
      MediaAsset? seedMedia;
      for (final media in artwork.media) {
        if (media.kind != MediaKind.image || media.width == null || media.height == null) {
          continue;
        }
        if (seedMedia == null || (media.width ?? 0) > (seedMedia.width ?? 0)) {
          seedMedia = media;
        }
      }
      if (seedMedia?.width case final int width when width > 0) {
        final height = seedMedia?.height ?? 0;
        if (height > 0) seedAspect = width / height;
      }

      final seedSet = tags.map(normalizeExternalTag).toSet();
      final unique = <String, ExternalAdultPost>{};
      for (final post in batches.expand((items) => items)) {
        if (!isStrictAdultExternalPost(post)) continue;
        final key = '${post.source.name}:${post.id}';
        unique[key] = post.copyWith(
          similarity: externalSimilarityScore(
            seedTags: seedSet,
            candidate: post,
            seedAspectRatio: seedAspect,
          ),
        );
      }

      final ranked = unique.values.toList(growable: false)
        ..sort((a, b) => b.similarity.compareTo(a.similarity));
      return List<ExternalAdultPost>.unmodifiable(ranked.take(24));
    });

final class ExternalAdultArtClient {
  const ExternalAdultArtClient(this._dio);

  final Dio _dio;

  Future<List<ExternalAdultPost>> search(
    ExternalAdultSource source,
    List<String> seedTags,
  ) async {
    final endpoint = switch (source) {
      ExternalAdultSource.rule34 => Uri.https('api.rule34.xxx', '/index.php'),
      ExternalAdultSource.gelbooru => Uri.https('gelbooru.com', '/index.php'),
    };
    final tags = <String>['rating:explicit', ...seedTags.take(2)].join(' ');
    try {
      final response = await _dio.getUri<Object?>(
        endpoint,
        queryParameters: <String, Object?>{
          'page': 'dapi',
          's': 'post',
          'q': 'index',
          'json': 1,
          'limit': 40,
          'pid': 0,
          'tags': tags,
        },
        options: Options(
          headers: const <String, String>{
            'Accept': 'application/json',
            'User-Agent': 'DAViewer/0.4.2 external-similar',
          },
          responseType: ResponseType.json,
          sendTimeout: const Duration(seconds: 12),
          receiveTimeout: const Duration(seconds: 18),
          validateStatus: (status) => status != null && status >= 200 && status < 500,
        ),
      );
      if ((response.statusCode ?? 500) >= 400) return const <ExternalAdultPost>[];
      return parseExternalAdultPosts(source, response.data);
    } on Object {
      return const <ExternalAdultPost>[];
    }
  }
}

List<ExternalAdultPost> parseExternalAdultPosts(
  ExternalAdultSource source,
  Object? payload,
) {
  Object? decoded = payload;
  if (decoded is String) {
    try {
      decoded = jsonDecode(decoded);
    } on Object {
      return const <ExternalAdultPost>[];
    }
  }

  final List<Object?> rows;
  if (decoded is List) {
    rows = decoded.cast<Object?>();
  } else if (decoded is Map) {
    final post = decoded['post'];
    rows = post is List ? post.cast<Object?>() : const <Object?>[];
  } else {
    return const <ExternalAdultPost>[];
  }

  final result = <ExternalAdultPost>[];
  for (final raw in rows) {
    if (raw is! Map) continue;
    final row = <String, Object?>{
      for (final entry in raw.entries) entry.key.toString(): entry.value,
    };
    final id = _string(row['id']);
    final preview = _uri(row['preview_url']) ?? _uri(row['sample_url']);
    final file = _uri(row['file_url']) ?? _uri(row['sample_url']) ?? preview;
    if (id == null || preview == null || file == null) continue;

    final tags = (_string(row['tags']) ?? '')
        .split(RegExp(r'\s+'))
        .map(normalizeExternalTag)
        .where((tag) => tag.isNotEmpty)
        .toSet();

    final pageUri = switch (source) {
      ExternalAdultSource.rule34 => Uri.https(
        'rule34.xxx',
        '/index.php',
        <String, String>{'page': 'post', 's': 'view', 'id': id},
      ),
      ExternalAdultSource.gelbooru => Uri.https(
        'gelbooru.com',
        '/index.php',
        <String, String>{'page': 'post', 's': 'view', 'id': id},
      ),
    };

    final candidate = ExternalAdultPost(
      source: source,
      id: id,
      pageUri: pageUri,
      previewUri: _absoluteUri(source, preview),
      fileUri: _absoluteUri(source, file),
      tags: tags,
      rating: _string(row['rating']) ?? '',
      width: _int(row['width']),
      height: _int(row['height']),
    );
    if (isStrictAdultExternalPost(candidate)) result.add(candidate);
  }
  return List<ExternalAdultPost>.unmodifiable(result);
}

Uri _absoluteUri(ExternalAdultSource source, Uri uri) {
  if (uri.hasScheme) return uri;
  final origin = switch (source) {
    ExternalAdultSource.rule34 => Uri.parse('https://rule34.xxx/'),
    ExternalAdultSource.gelbooru => Uri.parse('https://gelbooru.com/'),
  };
  return origin.resolveUri(uri);
}

String? _string(Object? value) {
  if (value == null) return null;
  final text = value.toString().trim();
  return text.isEmpty ? null : text;
}

int? _int(Object? value) => switch (value) {
  int v => v,
  String v => int.tryParse(v),
  _ => null,
};

Uri? _uri(Object? value) {
  final text = _string(value);
  return text == null ? null : Uri.tryParse(text);
}

final class ExternalAdultSimilarSection extends ConsumerWidget {
  const ExternalAdultSimilarSection({required this.artworkId, super.key});

  final String artworkId;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final s = strings(ref.watch(appLanguageProvider));
    final result = ref.watch(externalAdultSimilarProvider(artworkId));

    return result.when(
      loading: () => const SizedBox.shrink(),
      error: (error, stack) => const SizedBox.shrink(),
      data: (items) {
        if (items.isEmpty) return const SizedBox.shrink();
        return Padding(
          padding: const EdgeInsets.only(top: 12, bottom: 4),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: <Widget>[
              Row(
                children: <Widget>[
                  Expanded(
                    child: Text(
                      s.externalAdultSimilar,
                      style: Theme.of(context).textTheme.titleMedium,
                    ),
                  ),
                  const Icon(Icons.eighteen_up_rating_outlined, size: 18),
                ],
              ),
              const SizedBox(height: 4),
              Text(
                s.externalAdultSimilarHint,
                style: Theme.of(context).textTheme.bodySmall,
              ),
              const SizedBox(height: 10),
              SizedBox(
                height: 188,
                child: ListView.separated(
                  scrollDirection: Axis.horizontal,
                  itemCount: items.length,
                  separatorBuilder: (_, _) => const SizedBox(width: 10),
                  itemBuilder: (context, index) {
                    final item = items[index];
                    return _ExternalAdultCard(item: item);
                  },
                ),
              ),
            ],
          ),
        );
      },
    );
  }
}

final class _ExternalAdultCard extends StatelessWidget {
  const _ExternalAdultCard({required this.item});

  final ExternalAdultPost item;

  @override
  Widget build(BuildContext context) {
    final percent = (item.similarity * 100).round().clamp(0, 100);
    return SizedBox(
      width: 132,
      child: InkWell(
        borderRadius: BorderRadius.circular(10),
        onTap: () => launchUrl(
          item.pageUri,
          mode: LaunchMode.externalApplication,
        ),
        child: Card(
          margin: EdgeInsets.zero,
          clipBehavior: Clip.antiAlias,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: <Widget>[
              Expanded(
                child: CachedNetworkImage(
                  imageUrl: item.previewUri.toString(),
                  width: double.infinity,
                  fit: BoxFit.cover,
                  memCacheWidth: 320,
                  placeholder: (context, url) => const ColoredBox(
                    color: Color(0x1A808080),
                    child: Center(child: CircularProgressIndicator(strokeWidth: 2)),
                  ),
                  errorWidget: (context, url, error) => const ColoredBox(
                    color: Color(0x1A808080),
                    child: Center(child: Icon(Icons.broken_image_outlined)),
                  ),
                ),
              ),
              Padding(
                padding: const EdgeInsets.fromLTRB(8, 6, 8, 7),
                child: Row(
                  children: <Widget>[
                    Expanded(
                      child: Text(
                        item.sourceLabel,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: Theme.of(context).textTheme.labelSmall,
                      ),
                    ),
                    Text(
                      '$percent%',
                      style: Theme.of(context).textTheme.labelSmall,
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
