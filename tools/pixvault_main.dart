import 'dart:convert';
import 'dart:io';

import 'package:cached_network_image/cached_network_image.dart';
import 'package:flutter/material.dart';
import 'package:webview_flutter/webview_flutter.dart';
import 'package:http/http.dart' as http;
import 'package:path_provider/path_provider.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:url_launcher/url_launcher.dart';
import 'package:video_player/video_player.dart';

void main() async {
  WidgetsFlutterBinding.ensureInitialized();
  runApp(const PixVaultApp());
}

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

class PixVaultApp extends StatefulWidget {
  const PixVaultApp({super.key});

  @override
  State<PixVaultApp> createState() => _PixVaultAppState();
}

class _PixVaultAppState extends State<PixVaultApp> {
  bool? accepted;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    final p = await SharedPreferences.getInstance();
    if (mounted) setState(() => accepted = p.getBool('ageAccepted') ?? false);
  }

  @override
  Widget build(BuildContext context) {
    final scheme = ColorScheme.fromSeed(
      seedColor: const Color(0xff00c8a7),
      brightness: Brightness.dark,
    );
    return MaterialApp(
      debugShowCheckedModeBanner: false,
      title: 'PixVault',
      theme: ThemeData(
        colorScheme: scheme,
        brightness: Brightness.dark,
        scaffoldBackgroundColor: const Color(0xff0c0f12),
        cardColor: const Color(0xff14191e),
        useMaterial3: true,
      ),
      home: accepted == null
          ? const Scaffold(body: Center(child: CircularProgressIndicator()))
          : accepted!
              ? const HomeShell()
              : AgeGate(onAccepted: () => setState(() => accepted = true)),
    );
  }
}

class AgeGate extends StatelessWidget {
  final VoidCallback onAccepted;
  const AgeGate({super.key, required this.onAccepted});

  Future<void> _accept() async {
    final p = await SharedPreferences.getInstance();
    await p.setBool('ageAccepted', true);
    onAccepted();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: SafeArea(
        child: Center(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 520),
            child: Padding(
              padding: const EdgeInsets.all(28),
              child: Column(
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  const Icon(Icons.visibility_rounded, size: 74),
                  const SizedBox(height: 20),
                  Text('PixVault', style: Theme.of(context).textTheme.headlineLarge),
                  const SizedBox(height: 14),
                  const Text(
                    'This viewer is for adults only. Verified sources in this build are Rule34Vault/XYZ, yande.re and Pixiv. Searches and tags that explicitly indicate minors are blocked.',
                    textAlign: TextAlign.center,
                  ),
                  const SizedBox(height: 28),
                  FilledButton.icon(
                    onPressed: _accept,
                    icon: const Icon(Icons.check_circle_outline),
                    label: const Text('I confirm I am 18 or older'),
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class PixivRepo {
  static const ua =
      'Mozilla/5.0 (Linux; Android 16) AppleWebKit/537.36 Chrome/140 Mobile Safari/537.36';

  Future<String> _cookies() async {
    final list = await WebViewCookieManager().getCookies(
      domain: Uri.parse('https://www.pixiv.net/'),
    );
    return list.map((c) => '${c.name}=${c.value}').join('; ');
  }

  Future<bool> hasLogin() async {
    final c = await _cookies();
    return c.contains('PHPSESSID=');
  }

  Future<Map<String, String>> headers({String? referer}) async {
    final c = await _cookies();
    return {
      'User-Agent': ua,
      'Referer': referer ?? 'https://www.pixiv.net/',
      'Accept': 'application/json,text/plain,*/*',
      'Accept-Language': 'en-US,en;q=0.9',
      if (c.isNotEmpty) 'Cookie': c,
    };
  }

  Future<dynamic> _get(Uri uri, {String? referer}) async {
    final r = await http.get(uri, headers: await headers(referer: referer));
    if (r.statusCode != 200) {
      throw Exception('Pixiv HTTP ${r.statusCode}');
    }
    final j = jsonDecode(utf8.decode(r.bodyBytes));
    if (j is Map && j['error'] == true) {
      throw Exception('${j['message'] ?? 'Pixiv request failed'}');
    }
    return j;
  }

  List<String> _tags(dynamic raw) {
    if (raw is! List) return const [];
    return raw.map((e) {
      if (e is Map) return '${e['tag'] ?? e['name'] ?? ''}';
      return '$e';
    }).where((e) => e.isNotEmpty).toList();
  }

  Future<List<Artwork>> list(String query, int page) async {
    if (!await hasLogin()) {
      throw Exception('PIXIV_LOGIN_REQUIRED');
    }
    if (Safety.blockedQuery(query)) throw Exception('BLOCKED_QUERY');

    if (query.trim().isEmpty) {
      final uri = Uri.parse(
        'https://www.pixiv.net/ranking.php?format=json&mode=daily_r18&content=illust&p=$page',
      );
      final j = await _get(uri);
      final rows = (j is Map ? j['contents'] : null) as List? ?? const [];
      return rows.map((e) {
        final m = Map<String, dynamic>.from(e as Map);
        final id = '${m['illust_id'] ?? m['id'] ?? ''}';
        final tags = _tags(m['tags']);
        return Artwork(
          source: SourceType.pixiv,
          id: id,
          title: '${m['title'] ?? 'Pixiv artwork'}',
          userName: '${m['user_name'] ?? ''}',
          previewUrl: '${m['url'] ?? ''}',
          mediaUrl: '',
          isVideo: false,
          tags: tags,
          pageUrls: const [],
          sourceUrl: 'https://www.pixiv.net/artworks/$id',
        );
      }).where((a) => a.id.isNotEmpty && !Safety.blockedArtwork(a)).toList();
    }

    final q = query.trim();
    final enc = Uri.encodeComponent(q);
    final uri = Uri.parse(
      'https://www.pixiv.net/ajax/search/artworks/$enc'
      '?word=$enc&order=date_d&mode=r18&p=$page&s_mode=s_tag&type=all&lang=en',
    );
    final j = await _get(uri);
    final body = j is Map ? j['body'] : null;
    final block = body is Map ? body['illustManga'] : null;
    final rows = block is Map ? block['data'] : null;
    final list = rows is List ? rows : const [];
    return list.map((e) {
      final m = Map<String, dynamic>.from(e as Map);
      final id = '${m['id'] ?? ''}';
      final tags = _tags(m['tags']);
      return Artwork(
        source: SourceType.pixiv,
        id: id,
        title: '${m['title'] ?? 'Pixiv artwork'}',
        userName: '${m['userName'] ?? ''}',
        previewUrl: '${m['url'] ?? ''}',
        mediaUrl: '',
        isVideo: m['illustType'] == 2,
        tags: tags,
        pageUrls: const [],
        sourceUrl: 'https://www.pixiv.net/artworks/$id',
      );
    }).where((a) => a.id.isNotEmpty && !Safety.blockedArtwork(a)).toList();
  }

  Future<Artwork> details(Artwork a) async {
    final info = await _get(
      Uri.parse('https://www.pixiv.net/ajax/illust/${a.id}'),
      referer: a.sourceUrl,
    );
    final pages = await _get(
      Uri.parse('https://www.pixiv.net/ajax/illust/${a.id}/pages'),
      referer: a.sourceUrl,
    );

    final body = info is Map && info['body'] is Map
        ? Map<String, dynamic>.from(info['body'] as Map)
        : <String, dynamic>{};
    final pageBody = pages is Map && pages['body'] is List
        ? pages['body'] as List
        : const [];

    final urls = <String>[];
    for (final p in pageBody) {
      if (p is! Map) continue;
      final u = p['urls'];
      if (u is Map) {
        final value = u['original'] ?? u['regular'] ?? u['small'];
        if (value != null) urls.add('$value');
      }
    }

    final tagBlock = body['tags'];
    final tags = <String>[];
    if (tagBlock is Map && tagBlock['tags'] is List) {
      for (final t in tagBlock['tags'] as List) {
        if (t is Map && t['tag'] != null) tags.add('${t['tag']}');
      }
    }

    final enriched = a.copyWith(
      title: '${body['illustTitle'] ?? body['title'] ?? a.title}',
      userName: '${body['userName'] ?? a.userName}',
      tags: tags.isEmpty ? a.tags : tags,
      pageUrls: urls,
      mediaUrl: urls.isNotEmpty ? urls.first : a.mediaUrl,
    );
    if (Safety.blockedArtwork(enriched)) throw Exception('BLOCKED_CONTENT');
    return enriched;
  }

  Future<List<Artwork>> recommendations(Artwork a) async {
    final j = await _get(
      Uri.parse(
        'https://www.pixiv.net/ajax/illust/${a.id}/recommend/init?limit=40',
      ),
      referer: a.sourceUrl,
    );
    final body = j is Map ? j['body'] : null;
    final rows = body is Map && body['illusts'] is List
        ? body['illusts'] as List
        : const [];
    final out = <Artwork>[];
    for (final raw in rows) {
      if (raw is! Map) continue;
      final m = Map<String, dynamic>.from(raw);
      final id = '${m['id'] ?? ''}';
      if (id.isEmpty || id == a.id) continue;
      final tags = _tags(m['tags']);
      final rawUrls = m['urls'];
      final urls = rawUrls is Map
          ? Map<String, dynamic>.from(rawUrls)
          : <String, dynamic>{};
      final preview =
          '${m['url'] ?? urls['regular'] ?? urls['small'] ?? ''}';
      final item = Artwork(
        source: SourceType.pixiv,
        id: id,
        title: '${m['title'] ?? 'Pixiv artwork'}',
        userName: '${m['userName'] ?? ''}',
        previewUrl: preview,
        mediaUrl: '${urls['original'] ?? urls['regular'] ?? ''}',
        isVideo: m['illustType'] == 2,
        tags: tags,
        pageUrls: const [],
        sourceUrl: 'https://www.pixiv.net/artworks/$id',
      );
      if (!Safety.blockedArtwork(item)) out.add(item);
    }
    return out;
  }
}

class R34Repo {
  static const root = 'https://rule34.xyz';
  static const cdn = 'https://rule34xyz.b-cdn.net';
  static const ua =
      'Mozilla/5.0 (Linux; Android 16) AppleWebKit/537.36 Chrome/140 Mobile Safari/537.36';

  List<String> parseTags(dynamic raw) {
    if (raw is! List) return const [];
    return raw.map((e) {
      if (e is Map) return '${e['value'] ?? e['name'] ?? ''}';
      return '$e';
    }).where((e) => e.isNotEmpty).toList();
  }

  String _canonicalTag(String tag) {
    final value = tag.trim().toLowerCase();
    const aliases = <String, String>{
      'futa': 'futanari',
      'trans': 'transgender',
    };
    return aliases[value] ?? tag.trim();
  }

  List<String> _queryTags(String query) => query
      .split(RegExp(r'[,|]+'))
      .map((e) => _canonicalTag(e))
      .where((e) => e.isNotEmpty)
      .toList();

  Future<Map<String, dynamic>> _detailMap(String id) async {
    final r = await http.get(
      Uri.parse('$root/api/v2/post/$id'),
      headers: {
        'User-Agent': ua,
        'Accept': 'application/json',
        'Referer': '$root/',
      },
    );
    if (r.statusCode != 200) {
      throw Exception('Rule34Vault detail HTTP ${r.statusCode}');
    }
    return Map<String, dynamic>.from(
      jsonDecode(utf8.decode(r.bodyBytes)) as Map,
    );
  }

  String _fileUrl(Map<String, dynamic> m) {
    final id = int.tryParse('${m['id'] ?? ''}') ?? 0;
    if (id <= 0) return '';

    final rawFiles = m['files'];
    if (rawFiles is! Map || rawFiles.isEmpty) return '';

    final files = rawFiles.map(
      (key, value) => MapEntry(key.toString(), value),
    );

    const preferred = <String>['100', '101', '102', '10'];
    String? fmt;
    for (final candidate in preferred) {
      if (files.containsKey(candidate)) {
        fmt = candidate;
        break;
      }
    }
    fmt ??= files.keys.first;

    final extension = switch (fmt) {
      '100' => 'mov.mp4',
      '101' => 'mov720.mp4',
      '102' => 'mov480.mp4',
      _ => 'pic.jpg',
    };

    final location = files[fmt];
    var useCdn = true;
    if (location is List && location.isNotEmpty) {
      useCdn = location.first == 1 || location.first == true;
    }

    final base = useCdn ? cdn : root;
    return '$base/posts/${id ~/ 1000}/$id/$id.$extension';
  }

  Artwork _artwork(Map<String, dynamic> m) {
    final id = '${m['id'] ?? ''}';
    final tags = parseTags(m['tags']);
    final file = _fileUrl(m);
    final lower = file.toLowerCase();
    final video =
        lower.endsWith('.mp4') ||
        lower.endsWith('.webm') ||
        lower.endsWith('.m4v');

    return Artwork(
      source: SourceType.rule34vault,
      id: id,
      title: tags.isNotEmpty ? tags.take(4).join(' · ') : 'Post #$id',
      userName: '${m['uploader'] is Map ? ((m['uploader'] as Map)['displayName'] ?? (m['uploader'] as Map)['userName'] ?? '') : ''}',
      previewUrl: video ? '' : file,
      mediaUrl: file,
      isVideo: video,
      tags: tags,
      pageUrls: video || file.isEmpty ? const [] : [file],
      sourceUrl: '$root/post/$id',
    );
  }

  Future<List<Artwork>> list(String query, int page) async {
    if (Safety.blockedQuery(query)) throw Exception('BLOCKED_QUERY');

    final tags = _queryTags(query);
    final body = {
      'includeTags': tags,
      'CountTotal': false,
      'IncludeLinks': true,
      'OrderBy': 0,
      'Skip': (page - 1) * 18,
      'take': 18,
    };

    final r = await http.post(
      Uri.parse('$root/api/v2/post/search/root'),
      headers: {
        'User-Agent': ua,
        'Content-Type': 'application/json',
        'Accept': 'application/json',
        'Referer': '$root/',
      },
      body: jsonEncode(body),
    );
    if (r.statusCode != 200) {
      throw Exception('Rule34Vault HTTP ${r.statusCode}');
    }

    final j = jsonDecode(utf8.decode(r.bodyBytes));
    final rows = j is Map && j['items'] is List
        ? j['items'] as List
        : const [];

    // Search results no longer contain tags. Fetch details before exposing a
    // card so the mandatory minor-content filter can inspect the real tags.
    final detailed = await Future.wait(
      rows.map((raw) async {
        if (raw is! Map) return null;
        final id = '${raw['id'] ?? ''}';
        if (id.isEmpty) return null;
        try {
          final full = await _detailMap(id);
          final item = _artwork(full);
          if (item.tags.isEmpty || Safety.blockedArtwork(item)) return null;
          return item;
        } catch (_) {
          return null;
        }
      }),
    );

    return detailed.whereType<Artwork>().toList();
  }

  Future<Artwork> details(Artwork a) async {
    final m = await _detailMap(a.id);
    final enriched = _artwork(m);
    if (enriched.tags.isEmpty || Safety.blockedArtwork(enriched)) {
      throw Exception('BLOCKED_CONTENT');
    }
    return enriched;
  }
}

List<String> similarityTags(List<String> tags) {
  const ignored = <String>{
    'solo',
    '1girl',
    '1boy',
    '2girls',
    '2boys',
    'multiple_girls',
    'multiple_boys',
    'looking_at_viewer',
    'highres',
    'absurdres',
    'explicit',
    'questionable',
    'safe',
    'rating:e',
    'rating:q',
    'rating:s',
    'rating:explicit',
  };
  final out = <String>[];
  for (final raw in tags) {
    final tag = raw.trim();
    if (tag.isEmpty) continue;
    final lower = tag.toLowerCase();
    if (ignored.contains(lower)) continue;
    if (lower.startsWith('rating:')) continue;
    if (Safety.blockedQuery(tag)) continue;
    if (!out.contains(tag)) out.add(tag);
  }
  return out;
}

Map<String, String> mediaHeaders(Artwork a) {
  const ua =
      'Mozilla/5.0 (Linux; Android 16) AppleWebKit/537.36 Chrome/140 Mobile Safari/537.36';
  switch (a.source) {
    case SourceType.pixiv:
      return {
        'User-Agent': PixivRepo.ua,
        'Referer': 'https://www.pixiv.net/',
      };
    case SourceType.rule34vault:
      return {'User-Agent': R34Repo.ua, 'Referer': 'https://rule34.xyz/'};
    case SourceType.gelbooru:
      return {'User-Agent': ua, 'Referer': 'https://gelbooru.com/'};
    case SourceType.danbooru:
      return {'User-Agent': ua, 'Referer': 'https://danbooru.donmai.us/'};
    case SourceType.yandere:
      return {'User-Agent': ua, 'Referer': 'https://yande.re/'};
    case SourceType.konachan:
      return {'User-Agent': ua, 'Referer': 'https://konachan.com/'};
  }
}

extension R34Similarity on R34Repo {
  Future<List<Artwork>> similar(Artwork a) async {
    final tags = similarityTags(a.tags);
    if (tags.isEmpty) return const [];
    final maxTags = tags.length > 3 ? 3 : tags.length;
    for (var n = maxTags; n >= 1; n--) {
      try {
        final rows = await list(tags.take(n).join(','), 1);
        final filtered = rows.where((e) => e.id != a.id).toList();
        if (filtered.isNotEmpty) return filtered.take(40).toList();
      } catch (_) {
        // Try a broader tag set.
      }
    }
    return const [];
  }
}

class BooruRepo {
  final SourceType source;
  const BooruRepo(this.source);

  static const ua =
      'Mozilla/5.0 (Linux; Android 16) AppleWebKit/537.36 Chrome/140 Mobile Safari/537.36';

  String get origin => source.homeUrl.substring(0, source.homeUrl.length - 1);

  String _normaliseQuery(String query) {
    final q = query
        .trim()
        .replaceAll(',', ' ')
        .split(RegExp(r'\s+'))
        .where((e) => e.isNotEmpty)
        .join(' ');
    if (q.isNotEmpty) return q;
    return source == SourceType.danbooru ? 'rating:explicit' : 'rating:e';
  }

  String _url(String path) {
    if (path.isEmpty || path == 'null') return '';
    if (path.startsWith('//')) return 'https:$path';
    if (path.startsWith('/')) return '$origin$path';
    return path;
  }

  List<String> _tags(dynamic value) {
    if (value is List) {
      return value.map((e) => '$e').where((e) => e.isNotEmpty).toList();
    }
    return '$value'
        .split(RegExp(r'\s+'))
        .map((e) => e.trim())
        .where((e) => e.isNotEmpty && e != 'null')
        .toList();
  }

  Future<dynamic> _get(Uri uri) async {
    final r = await http.get(
      uri,
      headers: {
        'User-Agent': ua,
        'Accept': 'application/json',
        'Referer': source.homeUrl,
      },
    );
    if (r.statusCode != 200) {
      throw Exception('${source.label} HTTP ${r.statusCode}');
    }
    return jsonDecode(utf8.decode(r.bodyBytes));
  }

  Future<List<Artwork>> list(String query, int page) async {
    if (Safety.blockedQuery(query)) throw Exception('BLOCKED_QUERY');
    final tags = _normaliseQuery(query);
    dynamic json;

    switch (source) {
      case SourceType.gelbooru:
        json = await _get(
          Uri.parse('https://gelbooru.com/index.php').replace(
            queryParameters: {
              'page': 'dapi',
              's': 'post',
              'q': 'index',
              'json': '1',
              'limit': '60',
              'pid': '${page - 1}',
              'tags': tags,
            },
          ),
        );
        break;
      case SourceType.danbooru:
        json = await _get(
          Uri.parse('https://danbooru.donmai.us/posts.json').replace(
            queryParameters: {
              'limit': '60',
              'page': '$page',
              'tags': tags,
            },
          ),
        );
        break;
      case SourceType.yandere:
        json = await _get(
          Uri.parse('https://yande.re/post.json').replace(
            queryParameters: {
              'limit': '60',
              'page': '$page',
              'tags': tags,
            },
          ),
        );
        break;
      case SourceType.konachan:
        json = await _get(
          Uri.parse('https://konachan.com/post.json').replace(
            queryParameters: {
              'limit': '60',
              'page': '$page',
              'tags': tags,
            },
          ),
        );
        break;
      default:
        throw StateError('Unsupported booru source');
    }

    final rows = <dynamic>[];
    if (json is List) {
      rows.addAll(json);
    } else if (json is Map && json['post'] is List) {
      rows.addAll(json['post'] as List);
    }

    final out = <Artwork>[];
    for (final raw in rows) {
      if (raw is! Map) continue;
      final m = Map<String, dynamic>.from(raw);
      final id = '${m['id'] ?? ''}';
      if (id.isEmpty) continue;

      final tagList = source == SourceType.danbooru
          ? _tags(m['tag_string'] ?? '')
          : _tags(m['tags'] ?? '');

      final file = _url(
        '${m['file_url'] ?? m['large_file_url'] ?? m['sample_url'] ?? ''}',
      );
      final preview = _url(
        '${m['preview_file_url'] ?? m['preview_url'] ?? m['sample_url'] ?? file}',
      );
      final lower = file.toLowerCase();
      final isVideo =
          lower.endsWith('.mp4') ||
          lower.endsWith('.webm') ||
          lower.endsWith('.m4v');

      String sourceUrl;
      switch (source) {
        case SourceType.gelbooru:
          sourceUrl =
              'https://gelbooru.com/index.php?page=post&s=view&id=$id';
          break;
        case SourceType.danbooru:
          sourceUrl = 'https://danbooru.donmai.us/posts/$id';
          break;
        case SourceType.yandere:
          sourceUrl = 'https://yande.re/post/show/$id';
          break;
        case SourceType.konachan:
          sourceUrl = 'https://konachan.com/post/show/$id';
          break;
        default:
          sourceUrl = source.homeUrl;
      }

      final item = Artwork(
        source: source,
        id: id,
        title: tagList.isNotEmpty
            ? tagList.take(4).join(' · ')
            : '${source.label} #$id',
        userName:
            '${m['uploader_name'] ?? m['author'] ?? m['owner'] ?? ''}',
        previewUrl: preview,
        mediaUrl: file,
        isVideo: isVideo,
        tags: tagList,
        pageUrls: isVideo || file.isEmpty ? const [] : [file],
        sourceUrl: sourceUrl,
      );
      if (!Safety.blockedArtwork(item)) out.add(item);
    }
    return out;
  }

  Future<Artwork> details(Artwork a) async {
    if (Safety.blockedArtwork(a)) throw Exception('BLOCKED_CONTENT');
    return a;
  }

  Future<List<Artwork>> similar(Artwork a) async {
    final tags = similarityTags(a.tags);
    if (tags.isEmpty) return const [];
    final maxTags = tags.length > 3 ? 3 : tags.length;
    for (var n = maxTags; n >= 1; n--) {
      try {
        final rows = await list(tags.take(n).join(' '), 1);
        final filtered = rows.where((e) => e.id != a.id).toList();
        if (filtered.isNotEmpty) return filtered.take(40).toList();
      } catch (_) {
        // Some boorus cap tag count; retry with fewer tags.
      }
    }
    return const [];
  }
}

class LocalStore {
  static Future<List<Artwork>> favorites() async {
    final p = await SharedPreferences.getInstance();
    final raw = p.getString('favorites');
    if (raw == null) return [];
    try {
      return (jsonDecode(raw) as List)
          .map((e) => Artwork.fromJson(Map<String, dynamic>.from(e as Map)))
          .toList();
    } catch (_) {
      return [];
    }
  }

  static Future<bool> isFavorite(Artwork a) async {
    final all = await favorites();
    return all.any((e) => e.source == a.source && e.id == a.id);
  }

  static Future<bool> toggleFavorite(Artwork a) async {
    final p = await SharedPreferences.getInstance();
    final all = await favorites();
    final i = all.indexWhere((e) => e.source == a.source && e.id == a.id);
    final now;
    if (i >= 0) {
      all.removeAt(i);
      now = false;
    } else {
      all.insert(0, a);
      now = true;
    }
    await p.setString('favorites', jsonEncode(all.map((e) => e.toJson()).toList()));
    return now;
  }

  static Future<List<Map<String, dynamic>>> downloads() async {
    final p = await SharedPreferences.getInstance();
    final raw = p.getString('downloads');
    if (raw == null) return [];
    try {
      return (jsonDecode(raw) as List)
          .map((e) => Map<String, dynamic>.from(e as Map))
          .toList();
    } catch (_) {
      return [];
    }
  }

  static Future<void> addDownload(Map<String, dynamic> d) async {
    final p = await SharedPreferences.getInstance();
    final all = await downloads();
    all.insert(0, d);
    await p.setString('downloads', jsonEncode(all.take(200).toList()));
  }
}

class DownloadService {
  static String safeName(String s) =>
      s.replaceAll(RegExp(r'[^a-zA-Z0-9._-]+'), '_').replaceAll(RegExp(r'_+'), '_');

  static Future<List<String>> download(Artwork a) async {
    final base = await getExternalStorageDirectory();
    if (base == null) throw Exception('Storage unavailable');
    final dir = Directory('${base.path}/PixVault');
    await dir.create(recursive: true);

    final urls = a.pageUrls.isNotEmpty
        ? a.pageUrls
        : (a.mediaUrl.isNotEmpty ? [a.mediaUrl] : [a.previewUrl]);
    final saved = <String>[];
    final pixiv = PixivRepo();

    for (var i = 0; i < urls.length; i++) {
      final u = urls[i];
      if (u.isEmpty) continue;
      final r = await http.get(
        Uri.parse(u),
        headers: a.source == SourceType.pixiv
            ? await pixiv.headers(referer: a.sourceUrl)
            : mediaHeaders(a),
      );
      if (r.statusCode != 200) throw Exception('Download HTTP ${r.statusCode}');
      var ext = Uri.parse(u).path.split('.').last.toLowerCase();
      if (ext.length > 5 || ext.contains('/')) ext = a.isVideo ? 'mp4' : 'jpg';
      final path =
          '${dir.path}/${a.source.short}_${safeName(a.id)}_${i + 1}.$ext';
      await File(path).writeAsBytes(r.bodyBytes);
      saved.add(path);
      await LocalStore.addDownload({
        'title': a.title,
        'source': a.source.label,
        'path': path,
        'date': DateTime.now().toIso8601String(),
      });
    }
    return saved;
  }
}

class HomeShell extends StatefulWidget {
  const HomeShell({super.key});

  @override
  State<HomeShell> createState() => _HomeShellState();
}

class _HomeShellState extends State<HomeShell> {
  int index = 0;

  @override
  Widget build(BuildContext context) {
    final pages = [
      const BrowsePage(),
      const FavoritesPage(),
      const DownloadsPage(),
      const SettingsPage(),
    ];
    return Scaffold(
      body: IndexedStack(index: index, children: pages),
      bottomNavigationBar: NavigationBar(
        selectedIndex: index,
        onDestinationSelected: (i) => setState(() => index = i),
        destinations: const [
          NavigationDestination(icon: Icon(Icons.grid_view_rounded), label: 'Browse'),
          NavigationDestination(icon: Icon(Icons.favorite_border), label: 'Favorites'),
          NavigationDestination(icon: Icon(Icons.download_outlined), label: 'Downloads'),
          NavigationDestination(icon: Icon(Icons.settings_outlined), label: 'Settings'),
        ],
      ),
    );
  }
}

class BrowsePage extends StatefulWidget {
  final SourceType initialSource;
  final String initialQuery;

  const BrowsePage({
    super.key,
    this.initialSource = SourceType.rule34vault,
    this.initialQuery = '',
  });

  @override
  State<BrowsePage> createState() => _BrowsePageState();
}

class _BrowsePageState extends State<BrowsePage> {
  final q = TextEditingController();
  final pixiv = PixivRepo();
  final r34 = R34Repo();
  late SourceType source;
  List<Artwork> items = [];
  bool loading = false;
  String? error;
  int page = 1;

  @override
  void initState() {
    super.initState();
    source = widget.initialSource;
    q.text = widget.initialQuery;
    Future.microtask(_search);
  }

  @override
  void dispose() {
    q.dispose();
    super.dispose();
  }

  Future<List<Artwork>> _fetch(String query, int nextPage) {
    switch (source) {
      case SourceType.pixiv:
        return pixiv.list(query, nextPage);
      case SourceType.rule34vault:
        return r34.list(query, nextPage);
      case SourceType.gelbooru:
      case SourceType.danbooru:
      case SourceType.yandere:
      case SourceType.konachan:
        return BooruRepo(source).list(query, nextPage);
    }
  }

  Future<void> _search({bool append = false}) async {
    if (loading) return;
    final query = q.text.trim();
    if (Safety.blockedQuery(query)) {
      setState(() {
        error = 'This search term is blocked by the safety filter.';
        items = [];
      });
      return;
    }
    setState(() {
      loading = true;
      error = null;
      if (!append) page = 1;
    });
    try {
      final next = await _fetch(query, page);
      if (!mounted) return;
      setState(() {
        if (append) {
          final known = items.map((e) => '${e.source.name}:${e.id}').toSet();
          items.addAll(
            next.where((e) => !known.contains('${e.source.name}:${e.id}')),
          );
        } else {
          items = next;
        }
        if (next.isNotEmpty) page++;
      });
    } catch (e) {
      if (!mounted) return;
      final msg = '$e';
      setState(() {
        error = msg.contains('PIXIV_LOGIN_REQUIRED')
            ? 'Pixiv R-18 requires login. Tap the account icon above.'
            : msg.contains('BLOCKED_QUERY')
                ? 'This search term is blocked.'
                : 'Could not load from ${source.label}: $msg';
      });
    } finally {
      if (mounted) setState(() => loading = false);
    }
  }

  Future<void> _loginPixiv() async {
    await Navigator.of(context).push(
      MaterialPageRoute(builder: (_) => const PixivLoginPage()),
    );
    if (source == SourceType.pixiv) _search();
  }

  @override
  Widget build(BuildContext context) {
    const visibleSources = <SourceType>[
      SourceType.rule34vault,
      SourceType.yandere,
      SourceType.pixiv,
    ];
    final segments = visibleSources
        .map(
          (value) => ButtonSegment<SourceType>(
            value: value,
            label: Text(value.label),
          ),
        )
        .toList();

    return Scaffold(
      appBar: AppBar(
        title: const Text('PixVault'),
        actions: [
          if (source == SourceType.pixiv)
            IconButton(
              tooltip: 'Pixiv login',
              onPressed: _loginPixiv,
              icon: const Icon(Icons.account_circle_outlined),
            ),
        ],
        bottom: PreferredSize(
          preferredSize: const Size.fromHeight(112),
          child: Padding(
            padding: const EdgeInsets.fromLTRB(12, 0, 12, 10),
            child: Column(
              children: [
                SingleChildScrollView(
                  scrollDirection: Axis.horizontal,
                  child: SegmentedButton<SourceType>(
                    showSelectedIcon: false,
                    segments: segments,
                    selected: {source},
                    onSelectionChanged: (selection) {
                      setState(() {
                        source = selection.first;
                        items = [];
                        page = 1;
                        error = null;
                      });
                      _search();
                    },
                  ),
                ),
                const SizedBox(height: 10),
                TextField(
                  controller: q,
                  textInputAction: TextInputAction.search,
                  onSubmitted: (_) => _search(),
                  decoration: InputDecoration(
                    hintText: source == SourceType.pixiv
                        ? 'Search Pixiv R-18 tags'
                        : 'Search tags on ${source.label}',
                    prefixIcon: const Icon(Icons.search),
                    suffixIcon: IconButton(
                      onPressed: () => _search(),
                      icon: const Icon(Icons.arrow_forward_rounded),
                    ),
                    filled: true,
                    border: OutlineInputBorder(
                      borderRadius: BorderRadius.circular(18),
                      borderSide: BorderSide.none,
                    ),
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
      body: RefreshIndicator(
        onRefresh: () => _search(),
        child: CustomScrollView(
          physics: const AlwaysScrollableScrollPhysics(),
          slivers: [
            if (error != null)
              SliverToBoxAdapter(
                child: Padding(
                  padding: const EdgeInsets.all(18),
                  child: Card(
                    child: Padding(
                      padding: const EdgeInsets.all(18),
                      child: Column(
                        children: [
                          const Icon(Icons.info_outline, size: 34),
                          const SizedBox(height: 8),
                          Text(error!, textAlign: TextAlign.center),
                          if (source == SourceType.pixiv &&
                              error!.contains('login'))
                            Padding(
                              padding: const EdgeInsets.only(top: 12),
                              child: FilledButton(
                                onPressed: _loginPixiv,
                                child: const Text('Log in to Pixiv'),
                              ),
                            ),
                        ],
                      ),
                    ),
                  ),
                ),
              ),
            SliverPadding(
              padding: const EdgeInsets.all(8),
              sliver: SliverGrid(
                delegate: SliverChildBuilderDelegate(
                  (context, i) {
                    final a = items[i];
                    return InkWell(
                      borderRadius: BorderRadius.circular(14),
                      onTap: () => Navigator.of(context).push(
                        MaterialPageRoute(
                          builder: (_) => DetailPage(initial: a),
                        ),
                      ),
                      child: Card(
                        clipBehavior: Clip.antiAlias,
                        child: Stack(
                          fit: StackFit.expand,
                          children: [
                            if (!a.isVideo && a.previewUrl.isNotEmpty)
                              CachedNetworkImage(
                                imageUrl: a.previewUrl,
                                httpHeaders: mediaHeaders(a),
                                fit: BoxFit.cover,
                                placeholder: (_, __) => const Center(
                                  child: CircularProgressIndicator(
                                    strokeWidth: 2,
                                  ),
                                ),
                                errorWidget: (_, __, ___) => const Icon(
                                  Icons.broken_image_outlined,
                                  size: 42,
                                ),
                              )
                            else
                              Container(
                                color: const Color(0xff181d22),
                                child: const Center(
                                  child: Icon(
                                    Icons.play_circle_outline,
                                    size: 56,
                                  ),
                                ),
                              ),
                            Align(
                              alignment: Alignment.bottomCenter,
                              child: Container(
                                width: double.infinity,
                                padding: const EdgeInsets.all(8),
                                decoration: const BoxDecoration(
                                  gradient: LinearGradient(
                                    begin: Alignment.bottomCenter,
                                    end: Alignment.topCenter,
                                    colors: [
                                      Color(0xdd000000),
                                      Color(0x00000000),
                                    ],
                                  ),
                                ),
                                child: Text(
                                  a.title,
                                  maxLines: 2,
                                  overflow: TextOverflow.ellipsis,
                                  style: const TextStyle(fontSize: 12),
                                ),
                              ),
                            ),
                            Positioned(
                              top: 7,
                              right: 7,
                              child: Container(
                                padding: const EdgeInsets.symmetric(
                                  horizontal: 7,
                                  vertical: 3,
                                ),
                                decoration: BoxDecoration(
                                  color: Colors.black87,
                                  borderRadius: BorderRadius.circular(8),
                                ),
                                child: Text(
                                  a.source.short,
                                  style: const TextStyle(fontSize: 10),
                                ),
                              ),
                            ),
                          ],
                        ),
                      ),
                    );
                  },
                  childCount: items.length,
                ),
                gridDelegate: const SliverGridDelegateWithFixedCrossAxisCount(
                  crossAxisCount: 2,
                  childAspectRatio: .72,
                  crossAxisSpacing: 4,
                  mainAxisSpacing: 4,
                ),
              ),
            ),
            SliverToBoxAdapter(
              child: Padding(
                padding: const EdgeInsets.fromLTRB(16, 8, 16, 30),
                child: loading
                    ? const Center(child: CircularProgressIndicator())
                    : items.isNotEmpty
                        ? OutlinedButton.icon(
                            onPressed: () => _search(append: true),
                            icon: const Icon(Icons.expand_more),
                            label: const Text('Load more'),
                          )
                        : const SizedBox.shrink(),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class DetailPage extends StatefulWidget {
  final Artwork initial;
  const DetailPage({super.key, required this.initial});

  @override
  State<DetailPage> createState() => _DetailPageState();
}

class _DetailPageState extends State<DetailPage> {
  late Artwork a;
  bool loading = true;
  bool favorite = false;
  String? error;
  VideoPlayerController? video;
  List<Artwork> similar = [];
  bool similarLoading = true;

  @override
  void initState() {
    super.initState();
    a = widget.initial;
    _load();
  }

  Future<void> _load() async {
    favorite = await LocalStore.isFavorite(a);
    try {
      switch (a.source) {
        case SourceType.pixiv:
          a = await PixivRepo().details(a);
          break;
        case SourceType.rule34vault:
          a = await R34Repo().details(a);
          break;
        case SourceType.gelbooru:
        case SourceType.danbooru:
        case SourceType.yandere:
        case SourceType.konachan:
          a = await BooruRepo(a.source).details(a);
          break;
      }
      if (a.isVideo && a.mediaUrl.isNotEmpty) {
        video = VideoPlayerController.networkUrl(
          Uri.parse(a.mediaUrl),
          httpHeaders: mediaHeaders(a),
        );
        await video!.initialize();
        await video!.setLooping(true);
      }
    } catch (e) {
      error = '$e';
    }
    if (mounted) setState(() => loading = false);
    if (error == null) _loadSimilar();
  }

  Future<void> _loadSimilar() async {
    try {
      List<Artwork> rows;
      switch (a.source) {
        case SourceType.pixiv:
          rows = await PixivRepo().recommendations(a);
          break;
        case SourceType.rule34vault:
          rows = await R34Repo().similar(a);
          break;
        case SourceType.gelbooru:
        case SourceType.danbooru:
        case SourceType.yandere:
        case SourceType.konachan:
          rows = await BooruRepo(a.source).similar(a);
          break;
      }
      if (!mounted) return;
      setState(() {
        similar = rows.where((e) => e.id != a.id).take(40).toList();
        similarLoading = false;
      });
    } catch (_) {
      if (mounted) setState(() => similarLoading = false);
    }
  }

  @override
  void dispose() {
    video?.dispose();
    super.dispose();
  }

  Future<void> _download() async {
    final messenger = ScaffoldMessenger.of(context);
    messenger.showSnackBar(const SnackBar(content: Text('Downloading…')));
    try {
      final paths = await DownloadService.download(a);
      if (!mounted) return;
      messenger.showSnackBar(
        SnackBar(
          content: Text('Saved ${paths.length} file(s) in PixVault folder'),
        ),
      );
    } catch (e) {
      if (!mounted) return;
      messenger.showSnackBar(
        SnackBar(content: Text('Download failed: $e')),
      );
    }
  }

  void _searchTag(String tag) {
    if (Safety.blockedQuery(tag)) return;
    Navigator.of(context).push(
      MaterialPageRoute(
        builder: (_) => BrowsePage(
          initialSource: a.source,
          initialQuery: tag,
        ),
      ),
    );
  }

  Widget _similarCard(Artwork item) {
    return SizedBox(
      width: 150,
      child: Card(
        clipBehavior: Clip.antiAlias,
        child: InkWell(
          onTap: () => Navigator.of(context).push(
            MaterialPageRoute(
              builder: (_) => DetailPage(initial: item),
            ),
          ),
          child: Stack(
            fit: StackFit.expand,
            children: [
              if (!item.isVideo && item.previewUrl.isNotEmpty)
                CachedNetworkImage(
                  imageUrl: item.previewUrl,
                  httpHeaders: mediaHeaders(item),
                  fit: BoxFit.cover,
                  errorWidget: (_, __, ___) =>
                      const Icon(Icons.broken_image_outlined),
                )
              else
                const Center(
                  child: Icon(Icons.play_circle_outline, size: 48),
                ),
              Align(
                alignment: Alignment.bottomCenter,
                child: Container(
                  width: double.infinity,
                  padding: const EdgeInsets.all(7),
                  color: Colors.black87,
                  child: Text(
                    item.title,
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(fontSize: 11),
                  ),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final urls = a.pageUrls.isNotEmpty
        ? a.pageUrls
        : (a.mediaUrl.isNotEmpty ? [a.mediaUrl] : [a.previewUrl]);

    return Scaffold(
      appBar: AppBar(
        title: Text(a.source.label),
        actions: [
          IconButton(
            onPressed: () async {
              final now = await LocalStore.toggleFavorite(a);
              if (mounted) setState(() => favorite = now);
            },
            icon: Icon(favorite ? Icons.favorite : Icons.favorite_border),
          ),
          IconButton(
            onPressed: _download,
            icon: const Icon(Icons.download_outlined),
          ),
          IconButton(
            onPressed: () => launchUrl(
              Uri.parse(a.sourceUrl),
              mode: LaunchMode.externalApplication,
            ),
            icon: const Icon(Icons.open_in_new),
          ),
        ],
      ),
      body: loading
          ? const Center(child: CircularProgressIndicator())
          : error != null
              ? Center(
                  child: Padding(
                    padding: const EdgeInsets.all(24),
                    child: Text(
                      error!.contains('BLOCKED_CONTENT')
                          ? 'This post was hidden by the safety filter.'
                          : 'Could not load details: $error',
                      textAlign: TextAlign.center,
                    ),
                  ),
                )
              : ListView(
                  children: [
                    if (a.isVideo && video != null)
                      AspectRatio(
                        aspectRatio: video!.value.aspectRatio == 0
                            ? 16 / 9
                            : video!.value.aspectRatio,
                        child: Stack(
                          alignment: Alignment.center,
                          children: [
                            VideoPlayer(video!),
                            IconButton.filledTonal(
                              iconSize: 42,
                              onPressed: () {
                                setState(() {
                                  video!.value.isPlaying
                                      ? video!.pause()
                                      : video!.play();
                                });
                              },
                              icon: Icon(
                                video!.value.isPlaying
                                    ? Icons.pause_rounded
                                    : Icons.play_arrow_rounded,
                              ),
                            ),
                          ],
                        ),
                      )
                    else if (urls.where((e) => e.isNotEmpty).isNotEmpty)
                      SizedBox(
                        height: MediaQuery.sizeOf(context).height * .64,
                        child: PageView(
                          children: urls
                              .where((e) => e.isNotEmpty)
                              .map(
                                (u) => InteractiveViewer(
                                  minScale: 1,
                                  maxScale: 5,
                                  child: CachedNetworkImage(
                                    imageUrl: u,
                                    httpHeaders: mediaHeaders(a),
                                    fit: BoxFit.contain,
                                    errorWidget: (_, __, ___) => const Center(
                                      child: Icon(
                                        Icons.broken_image_outlined,
                                        size: 54,
                                      ),
                                    ),
                                  ),
                                ),
                              )
                              .toList(),
                        ),
                      ),
                    Padding(
                      padding: const EdgeInsets.fromLTRB(16, 16, 16, 4),
                      child: Text(
                        a.title,
                        style: Theme.of(context).textTheme.titleLarge,
                      ),
                    ),
                    if (a.userName.isNotEmpty)
                      Padding(
                        padding: const EdgeInsets.symmetric(horizontal: 16),
                        child: Text(a.userName),
                      ),
                    if (a.tags.isNotEmpty)
                      Padding(
                        padding: const EdgeInsets.all(16),
                        child: Wrap(
                          spacing: 7,
                          runSpacing: 7,
                          children: a.tags
                              .take(60)
                              .where((t) => !Safety.blockedQuery(t))
                              .map(
                                (t) => ActionChip(
                                  avatar: const Icon(Icons.search, size: 16),
                                  label: Text(t),
                                  onPressed: () => _searchTag(t),
                                ),
                              )
                              .toList(),
                        ),
                      ),
                    Padding(
                      padding: const EdgeInsets.fromLTRB(16, 6, 16, 8),
                      child: Row(
                        children: [
                          Text(
                            'Similar',
                            style: Theme.of(context).textTheme.titleLarge,
                          ),
                          const Spacer(),
                          if (similarLoading)
                            const SizedBox(
                              width: 20,
                              height: 20,
                              child: CircularProgressIndicator(strokeWidth: 2),
                            ),
                        ],
                      ),
                    ),
                    if (!similarLoading && similar.isEmpty)
                      const Padding(
                        padding: EdgeInsets.fromLTRB(16, 4, 16, 18),
                        child: Text('No similar posts found for these tags.'),
                      )
                    else if (similar.isNotEmpty)
                      SizedBox(
                        height: 230,
                        child: ListView.separated(
                          padding: const EdgeInsets.symmetric(horizontal: 10),
                          scrollDirection: Axis.horizontal,
                          itemCount: similar.length,
                          separatorBuilder: (_, __) => const SizedBox(width: 4),
                          itemBuilder: (_, i) => _similarCard(similar[i]),
                        ),
                      ),
                    const SizedBox(height: 40),
                  ],
                ),
    );
  }
}

class PixivLoginPage extends StatefulWidget {
  const PixivLoginPage({super.key});

  @override
  State<PixivLoginPage> createState() => _PixivLoginPageState();
}

class _PixivLoginPageState extends State<PixivLoginPage> {
  late final WebViewController controller;

  @override
  void initState() {
    super.initState();
    controller = WebViewController()
      ..setJavaScriptMode(JavaScriptMode.unrestricted)
      ..loadRequest(Uri.parse('https://www.pixiv.net/'));
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Pixiv login'),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: const Text('Done'),
          ),
        ],
      ),
      body: WebViewWidget(controller: controller),
    );
  }
}

class FavoritesPage extends StatefulWidget {
  const FavoritesPage({super.key});

  @override
  State<FavoritesPage> createState() => _FavoritesPageState();
}

class _FavoritesPageState extends State<FavoritesPage> {
  Future<List<Artwork>> data = LocalStore.favorites();

  void reload() => setState(() => data = LocalStore.favorites());

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Favorites')),
      body: FutureBuilder<List<Artwork>>(
        future: data,
        builder: (context, s) {
          final items = s.data ?? [];
          if (s.connectionState != ConnectionState.done) {
            return const Center(child: CircularProgressIndicator());
          }
          if (items.isEmpty) return const Center(child: Text('No favorites yet'));
          return ListView.separated(
            itemCount: items.length,
            separatorBuilder: (_, __) => const Divider(height: 1),
            itemBuilder: (context, i) {
              final a = items[i];
              return ListTile(
                leading: SizedBox(
                  width: 54,
                  height: 54,
                  child: a.previewUrl.isNotEmpty
                      ? CachedNetworkImage(
                          imageUrl: a.previewUrl,
                          httpHeaders: mediaHeaders(a),
                          fit: BoxFit.cover,
                        )
                      : const Icon(Icons.play_circle_outline),
                ),
                title: Text(a.title, maxLines: 1, overflow: TextOverflow.ellipsis),
                subtitle: Text(a.source.label),
                onTap: () async {
                  await Navigator.of(context).push(
                    MaterialPageRoute(builder: (_) => DetailPage(initial: a)),
                  );
                  reload();
                },
              );
            },
          );
        },
      ),
    );
  }
}

class DownloadsPage extends StatefulWidget {
  const DownloadsPage({super.key});

  @override
  State<DownloadsPage> createState() => _DownloadsPageState();
}

class _DownloadsPageState extends State<DownloadsPage> {
  Future<List<Map<String, dynamic>>> data = LocalStore.downloads();

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Downloads')),
      body: FutureBuilder<List<Map<String, dynamic>>>(
        future: data,
        builder: (context, s) {
          final rows = s.data ?? [];
          if (s.connectionState != ConnectionState.done) {
            return const Center(child: CircularProgressIndicator());
          }
          if (rows.isEmpty) return const Center(child: Text('No downloads yet'));
          return ListView.separated(
            itemCount: rows.length,
            separatorBuilder: (_, __) => const Divider(height: 1),
            itemBuilder: (_, i) {
              final d = rows[i];
              return ListTile(
                leading: const Icon(Icons.insert_drive_file_outlined),
                title: Text('${d['title'] ?? 'Download'}', maxLines: 1),
                subtitle: Text(
                  '${d['source'] ?? ''}\n${d['path'] ?? ''}',
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis,
                ),
                isThreeLine: true,
              );
            },
          );
        },
      ),
    );
  }
}

class SettingsPage extends StatefulWidget {
  const SettingsPage({super.key});

  @override
  State<SettingsPage> createState() => _SettingsPageState();
}

class _SettingsPageState extends State<SettingsPage> {
  Future<bool> logged = PixivRepo().hasLogin();

  Future<void> _openLogin() async {
    await Navigator.of(context).push(
      MaterialPageRoute(builder: (_) => const PixivLoginPage()),
    );
    setState(() => logged = PixivRepo().hasLogin());
  }

  Future<void> _logout() async {
    await WebViewCookieManager().clearCookies();
    setState(() => logged = PixivRepo().hasLogin());
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Settings')),
      body: ListView(
        children: [
          FutureBuilder<bool>(
            future: logged,
            builder: (_, s) => ListTile(
              leading: const Icon(Icons.account_circle_outlined),
              title: const Text('Pixiv session'),
              subtitle: Text(s.data == true ? 'Logged in' : 'Not logged in'),
              trailing: FilledButton.tonal(
                onPressed: _openLogin,
                child: Text(s.data == true ? 'Open' : 'Login'),
              ),
            ),
          ),
          ListTile(
            leading: const Icon(Icons.logout),
            title: const Text('Clear Pixiv login'),
            onTap: _logout,
          ),
          const Divider(),
          const ListTile(
            leading: Icon(Icons.shield_outlined),
            title: Text('18+ safety filter'),
            subtitle: Text(
              'Explicit search terms and tags indicating minors are blocked and cannot be disabled.',
            ),
          ),
          const ListTile(
            leading: Icon(Icons.info_outline),
            title: Text('PixVault 0.2.1'),
            subtitle: Text(
              'Verified-source build: Rule34Vault/XYZ and yande.re work without credentials; Pixiv requires login. Sources currently blocked by API authentication or Cloudflare are hidden.',
            ),
          ),
        ],
      ),
    );
  }
}
