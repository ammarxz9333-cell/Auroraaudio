import 'package:dakit_core/dakit_core.dart';

enum ExternalSourceKind { danbooru, gelbooru, pixiv }

final class ExternalArtwork {
  const ExternalArtwork({
    required this.source,
    required this.id,
    required this.title,
    required this.artist,
    required this.pageUri,
    required this.previewUri,
    this.originalUri,
    this.width,
    this.height,
    this.tags = const <String>[],
    this.rating,
    this.publishedAt,
    this.artistId,
  });

  final ExternalSourceKind source;
  final String id;
  final String title;
  final String artist;
  final String? artistId;
  final Uri pageUri;
  final Uri previewUri;
  final Uri? originalUri;
  final int? width;
  final int? height;
  final List<String> tags;
  final String? rating;
  final DateTime? publishedAt;

  String get key => '${source.name}:$id';

  Artwork toArtwork() => Artwork(
        id: key,
        title: title,
        author: UserProfile(
          id: '${source.name}:artist:${artistId ?? artist}',
          username: artist,
        ),
        pageUri: pageUri,
        media: <MediaAsset>[
          MediaAsset(
            id: '$key:preview',
            kind: MediaKind.image,
            role: MediaRole.preview,
            availability: MediaAvailability.available,
            uri: previewUri,
            width: width,
            height: height,
          ),
        ],
        tags: tags,
        publishedAt: publishedAt,
      );

  MediaAsset? get downloadableAsset {
    final uri = originalUri ?? previewUri;
    return MediaAsset(
      id: '$key:original',
      kind: MediaKind.image,
      role: MediaRole.original,
      availability: MediaAvailability.available,
      uri: uri,
      width: width,
      height: height,
      filename: _filename(uri),
    );
  }

  Map<String, Object?> toJson() => <String, Object?>{
        'source': source.name,
        'id': id,
        'title': title,
        'artist': artist,
        'artistId': artistId,
        'pageUri': pageUri.toString(),
        'previewUri': previewUri.toString(),
        'originalUri': originalUri?.toString(),
        'width': width,
        'height': height,
        'tags': tags,
        'rating': rating,
        'publishedAt': publishedAt?.toIso8601String(),
      };

  static ExternalArtwork? fromJson(Map<String, Object?> json) {
    try {
      final source = ExternalSourceKind.values.byName(json['source']! as String);
      final original = json['originalUri'] as String?;
      final published = json['publishedAt'] as String?;
      return ExternalArtwork(
        source: source,
        id: json['id']! as String,
        title: (json['title'] as String?) ?? 'Untitled',
        artist: (json['artist'] as String?) ?? 'unknown',
        artistId: json['artistId'] as String?,
        pageUri: Uri.parse(json['pageUri']! as String),
        previewUri: Uri.parse(json['previewUri']! as String),
        originalUri: original == null ? null : Uri.parse(original),
        width: (json['width'] as num?)?.toInt(),
        height: (json['height'] as num?)?.toInt(),
        tags: (json['tags'] as List<dynamic>? ?? const <dynamic>[])
            .map((e) => e.toString())
            .toList(growable: false),
        rating: json['rating'] as String?,
        publishedAt: published == null ? null : DateTime.tryParse(published),
      );
    } on Object {
      return null;
    }
  }

  static String _filename(Uri uri) {
    final last = uri.pathSegments.isEmpty ? '' : uri.pathSegments.last;
    if (last.trim().isNotEmpty) return last;
    return 'image.jpg';
  }
}
