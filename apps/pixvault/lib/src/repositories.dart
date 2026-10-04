part of '../main.dart';

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

    // rule34.xyz currently reports storage codes such as [2], but the
    // corresponding media URL is served from rule34xyz.b-cdn.net.
    // The same path on the site origin returns 404.
    return '$cdn/posts/${id ~/ 1000}/$id/$id.$extension';
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
      if (r.statusCode == 401 || r.statusCode == 403) {
        throw Exception('SOURCE_ACCESS_BLOCKED:${source.label}:${r.statusCode}');
      }
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
