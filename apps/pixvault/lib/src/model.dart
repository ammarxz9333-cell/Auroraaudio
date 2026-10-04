part of '../main.dart';

enum SourceType {
  pixiv,
  rule34vault,
  gelbooru,
  danbooru,
  yandere,
  konachan,
}

extension SourceTypeX on SourceType {
  String get label {
    switch (this) {
      case SourceType.pixiv:
        return 'Pixiv';
      case SourceType.rule34vault:
        return 'Rule34Vault';
      case SourceType.gelbooru:
        return 'Gelbooru';
      case SourceType.danbooru:
        return 'Danbooru';
      case SourceType.yandere:
        return 'yande.re';
      case SourceType.konachan:
        return 'Konachan';
    }
  }

  String get short {
    switch (this) {
      case SourceType.pixiv:
        return 'PX';
      case SourceType.rule34vault:
        return 'R34';
      case SourceType.gelbooru:
        return 'GB';
      case SourceType.danbooru:
        return 'DB';
      case SourceType.yandere:
        return 'YD';
      case SourceType.konachan:
        return 'KC';
    }
  }

  bool get isBooru =>
      this == SourceType.gelbooru ||
      this == SourceType.danbooru ||
      this == SourceType.yandere ||
      this == SourceType.konachan;

  String get homeUrl {
    switch (this) {
      case SourceType.pixiv:
        return 'https://www.pixiv.net/';
      case SourceType.rule34vault:
        return 'https://rule34vault.com/';
      case SourceType.gelbooru:
        return 'https://gelbooru.com/';
      case SourceType.danbooru:
        return 'https://danbooru.donmai.us/';
      case SourceType.yandere:
        return 'https://yande.re/';
      case SourceType.konachan:
        return 'https://konachan.com/';
    }
  }
}

class Artwork {
  final SourceType source;
  final String id;
  final String title;
  final String userName;
  final String previewUrl;
  final String mediaUrl;
  final bool isVideo;
  final List<String> tags;
  final List<String> pageUrls;
  final String sourceUrl;

  const Artwork({
    required this.source,
    required this.id,
    required this.title,
    required this.userName,
    required this.previewUrl,
    required this.mediaUrl,
    required this.isVideo,
    required this.tags,
    required this.pageUrls,
    required this.sourceUrl,
  });

  Artwork copyWith({
    String? title,
    String? userName,
    String? previewUrl,
    String? mediaUrl,
    bool? isVideo,
    List<String>? tags,
    List<String>? pageUrls,
    String? sourceUrl,
  }) {
    return Artwork(
      source: source,
      id: id,
      title: title ?? this.title,
      userName: userName ?? this.userName,
      previewUrl: previewUrl ?? this.previewUrl,
      mediaUrl: mediaUrl ?? this.mediaUrl,
      isVideo: isVideo ?? this.isVideo,
      tags: tags ?? this.tags,
      pageUrls: pageUrls ?? this.pageUrls,
      sourceUrl: sourceUrl ?? this.sourceUrl,
    );
  }

  Map<String, dynamic> toJson() => {
        'source': source.name,
        'id': id,
        'title': title,
        'userName': userName,
        'previewUrl': previewUrl,
        'mediaUrl': mediaUrl,
        'isVideo': isVideo,
        'tags': tags,
        'pageUrls': pageUrls,
        'sourceUrl': sourceUrl,
      };

  factory Artwork.fromJson(Map<String, dynamic> j) => Artwork(
        source: SourceType.values.firstWhere(
          (e) => e.name == j['source'],
          orElse: () => SourceType.rule34vault,
        ),
        id: '${j['id'] ?? ''}',
        title: '${j['title'] ?? ''}',
        userName: '${j['userName'] ?? ''}',
        previewUrl: '${j['previewUrl'] ?? ''}',
        mediaUrl: '${j['mediaUrl'] ?? ''}',
        isVideo: j['isVideo'] == true,
        tags: (j['tags'] as List? ?? []).map((e) => '$e').toList(),
        pageUrls: (j['pageUrls'] as List? ?? []).map((e) => '$e').toList(),
        sourceUrl: '${j['sourceUrl'] ?? ''}',
      );
}

class Safety {
  static const blocked = <String>{
    'loli',
    'lolicon',
    'shota',
    'shotacon',
    'underage',
    'minor',
    'minors',
    'child',
    'children',
    'kid',
    'kids',
    'preteen',
    'teen',
    'cub',
    'ロリ',
    'ショタ',
    '未成年',
  };

  static String norm(String s) =>
      s.toLowerCase().replaceAll('_', ' ').replaceAll('-', ' ').trim();

  static bool blockedQuery(String q) {
    final words = norm(q).split(RegExp(r'\s+')).where((e) => e.isNotEmpty);
    return words.any(blocked.contains);
  }

  static bool blockedArtwork(Artwork a) {
    final fields = <String>[a.title, ...a.tags];
    for (final f in fields) {
      final n = norm(f);
      final words = n.split(RegExp(r'\s+'));
      if (words.any(blocked.contains)) return true;
    }
    return false;
  }
}
