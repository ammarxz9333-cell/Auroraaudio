import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:http/http.dart' as http;
import 'package:shared_preferences/shared_preferences.dart';
import 'package:url_launcher/url_launcher.dart';

void main() => runApp(const ImageAtlasApp());

enum MatureMode { hide, blur, show }

extension MatureModeLabel on MatureMode {
  String get label => switch (this) {
        MatureMode.hide => 'Hide',
        MatureMode.blur => 'Blur',
        MatureMode.show => 'Show',
      };
}

class ImageItem {
  const ImageItem({
    required this.id,
    required this.source,
    required this.title,
    required this.creator,
    required this.thumbnailUrl,
    required this.fullUrl,
    required this.pageUrl,
    this.isSensitive = false,
  });

  final String id;
  final String source;
  final String title;
  final String creator;
  final String thumbnailUrl;
  final String fullUrl;
  final String pageUrl;
  final bool isSensitive;

  String get identity {
    final normalized = normalizeUrl(fullUrl);
    return normalized.isNotEmpty ? normalized : '$source::$id';
  }

  Map<String, dynamic> toJson() => <String, dynamic>{
        'id': id,
        'source': source,
        'title': title,
        'creator': creator,
        'thumbnailUrl': thumbnailUrl,
        'fullUrl': fullUrl,
        'pageUrl': pageUrl,
        'isSensitive': isSensitive,
      };

  factory ImageItem.fromJson(Map<String, dynamic> json) => ImageItem(
        id: '${json['id'] ?? ''}',
        source: '${json['source'] ?? ''}',
        title: '${json['title'] ?? ''}',
        creator: '${json['creator'] ?? ''}',
        thumbnailUrl: '${json['thumbnailUrl'] ?? ''}',
        fullUrl: '${json['fullUrl'] ?? ''}',
        pageUrl: '${json['pageUrl'] ?? ''}',
        isSensitive: json['isSensitive'] == true,
      );
}

String normalizeUrl(String raw) {
  final parsed = Uri.tryParse(raw);
  if (parsed == null || !parsed.hasScheme) return raw.trim();
  final query = Map<String, String>.from(parsed.queryParameters)
    ..removeWhere((key, _) {
      final value = key.toLowerCase();
      return value.startsWith('utm_') ||
          value == 'ref' ||
          value == 'tracking';
    });
  return parsed
      .replace(
        scheme: parsed.scheme.toLowerCase(),
        host: parsed.host.toLowerCase(),
        queryParameters: query.isEmpty ? null : query,
        fragment: '',
      )
      .toString();
}

List<ImageItem> dedupeImages(Iterable<ImageItem> items) {
  final seen = <String>{};
  final result = <ImageItem>[];
  for (final item in items) {
    if (item.fullUrl.isEmpty || item.thumbnailUrl.isEmpty) continue;
    if (seen.add(item.identity)) result.add(item);
  }
  return result;
}

bool isOpenverseSensitive(Map<String, dynamic> json) {
  if (json['mature'] == true) return true;
  final sensitivity = json['sensitivity'];
  return sensitivity is List && sensitivity.isNotEmpty;
}

abstract class ImageSource {
  const ImageSource();
  String get name;

  Future<List<ImageItem>> search({
    required String query,
    required int page,
    required bool includeSensitive,
  });
}

class Net {
  static final http.Client _client = http.Client();
  static const Map<String, String> headers = <String, String>{
    'User-Agent': 'ImageAtlas/0.1 Android',
    'Accept': 'application/json,image/*;q=0.9,*/*;q=0.8',
  };

  static Future<http.Response> get(Uri uri) async {
    Object? lastError;
    for (var attempt = 0; attempt < 4; attempt++) {
      try {
        final response = await _client
            .get(uri, headers: headers)
            .timeout(const Duration(seconds: 22));
        if (response.statusCode != 429 && response.statusCode < 500) {
          return response;
        }
        lastError = StateError(
          'HTTP ${response.statusCode} from ${uri.host}',
        );
        final retryAfter =
            int.tryParse(response.headers['retry-after'] ?? '') ?? 0;
        final delay = retryAfter > 0
            ? Duration(seconds: retryAfter.clamp(1, 30))
            : Duration(milliseconds: 700 * (1 << attempt));
        await Future<void>.delayed(delay);
      } on Object catch (error) {
        lastError = error;
        await Future<void>.delayed(
          Duration(milliseconds: 500 * (1 << attempt)),
        );
      }
    }
    throw lastError ?? StateError('Network request failed.');
  }

  static Future<Map<String, dynamic>> jsonObject(Uri uri) async {
    final response = await get(uri);
    if (response.statusCode < 200 || response.statusCode >= 300) {
      throw StateError(
        'HTTP ${response.statusCode} from ${uri.host}',
      );
    }
    final decoded = jsonDecode(response.body);
    if (decoded is! Map<String, dynamic>) {
      throw const FormatException('Expected a JSON object.');
    }
    return decoded;
  }
}

class OpenverseSource extends ImageSource {
  const OpenverseSource();

  @override
  String get name => 'Openverse';

  @override
  Future<List<ImageItem>> search({
    required String query,
    required int page,
    required bool includeSensitive,
  }) async {
    Future<Map<String, dynamic>> request({required bool modernFlag}) {
      final params = <String, String>{
        'q': query,
        'page': '$page',
        'page_size': '40',
        if (includeSensitive)
          modernFlag ? 'include_sensitive_results' : 'mature': 'true',
      };
      return Net.jsonObject(
        Uri.https('api.openverse.org', '/v1/images/', params),
      );
    }

    Map<String, dynamic> payload;
    try {
      payload = await request(modernFlag: true);
    } on Object {
      if (!includeSensitive) rethrow;
      payload = await request(modernFlag: false);
    }

    final rows = payload['results'];
    if (rows is! List) return const <ImageItem>[];
    final items = <ImageItem>[];
    for (final raw in rows.whereType<Map>()) {
      final row = raw.cast<String, dynamic>();
      final full = '${row['url'] ?? ''}';
      final thumb = '${row['thumbnail'] ?? full}';
      final landing = '${row['foreign_landing_url'] ?? full}';
      if (!full.startsWith('http') || !thumb.startsWith('http')) continue;
      final item = ImageItem(
        id: '${row['id'] ?? full}',
        source: name,
        title: '${row['title'] ?? 'Untitled'}',
        creator: '${row['creator'] ?? ''}',
        thumbnailUrl: thumb,
        fullUrl: full,
        pageUrl: landing,
        isSensitive: isOpenverseSensitive(row),
      );
      if (!includeSensitive && item.isSensitive) continue;
      items.add(item);
    }
    return items;
  }
}

class WikimediaSource extends ImageSource {
  const WikimediaSource();

  @override
  String get name => 'Wikimedia';

  @override
  Future<List<ImageItem>> search({
    required String query,
    required int page,
    required bool includeSensitive,
  }) async {
    final offset = (page - 1) * 40;
    final uri = Uri.https(
      'commons.wikimedia.org',
      '/w/api.php',
      <String, String>{
        'action': 'query',
        'generator': 'search',
        'gsrsearch': query,
        'gsrnamespace': '6',
        'gsrlimit': '40',
        'gsroffset': '$offset',
        'prop': 'imageinfo|info',
        'iiprop': 'url|mime|extmetadata',
        'iiurlwidth': '700',
        'inprop': 'url',
        'format': 'json',
        'formatversion': '2',
        'origin': '*',
      },
    );
    final payload = await Net.jsonObject(uri);
    final queryData = payload['query'];
    if (queryData is! Map) return const <ImageItem>[];
    final pages = queryData['pages'];
    if (pages is! List) return const <ImageItem>[];

    final result = <ImageItem>[];
    for (final raw in pages.whereType<Map>()) {
      final row = raw.cast<String, dynamic>();
      final infos = row['imageinfo'];
      if (infos is! List || infos.isEmpty || infos.first is! Map) continue;
      final info = (infos.first as Map).cast<String, dynamic>();
      final full = '${info['url'] ?? ''}';
      final thumb = '${info['thumburl'] ?? full}';
      if (!full.startsWith('http') || !thumb.startsWith('http')) continue;

      var creator = '';
      final metadata = info['extmetadata'];
      if (metadata is Map) {
        final artist = metadata['Artist'];
        if (artist is Map) {
          creator = _stripHtml('${artist['value'] ?? ''}');
        }
      }

      result.add(
        ImageItem(
          id: '${row['pageid'] ?? full}',
          source: name,
          title: '${row['title'] ?? 'Untitled'}'
              .replaceFirst(RegExp(r'^File:'), ''),
          creator: creator,
          thumbnailUrl: thumb,
          fullUrl: full,
          pageUrl: '${row['canonicalurl'] ?? row['fullurl'] ?? full}',
        ),
      );
    }
    return result;
  }

  String _stripHtml(String value) => value
      .replaceAll(RegExp(r'<[^>]*>'), ' ')
      .replaceAll('&nbsp;', ' ')
      .trim();
}

class FlickrFeedSource extends ImageSource {
  const FlickrFeedSource();

  @override
  String get name => 'Flickr';

  @override
  Future<List<ImageItem>> search({
    required String query,
    required int page,
    required bool includeSensitive,
  }) async {
    if (page > 1) return const <ImageItem>[];
    final tags = query
        .split(RegExp(r'\s+'))
        .where((part) => part.trim().isNotEmpty)
        .take(8)
        .join(',');
    if (tags.isEmpty) return const <ImageItem>[];

    final uri = Uri.https(
      'www.flickr.com',
      '/services/feeds/photos_public.gne',
      <String, String>{
        'format': 'json',
        'nojsoncallback': '1',
        'tags': tags,
        'tagmode': 'any',
      },
    );
    final payload = await Net.jsonObject(uri);
    final rows = payload['items'];
    if (rows is! List) return const <ImageItem>[];

    final result = <ImageItem>[];
    for (final raw in rows.whereType<Map>()) {
      final row = raw.cast<String, dynamic>();
      final media = row['media'];
      final thumb = media is Map ? '${media['m'] ?? ''}' : '';
      final full = _largerUrl(thumb);
      final link = '${row['link'] ?? full}';
      final author = '${row['author'] ?? ''}'
          .replaceFirst(RegExp(r'^nobody@flickr\.com \("'), '')
          .replaceFirst(RegExp(r'"\)$'), '');
      if (!full.startsWith('http') || !thumb.startsWith('http')) continue;
      result.add(
        ImageItem(
          id: link,
          source: name,
          title: '${row['title'] ?? 'Untitled'}',
          creator: author,
          thumbnailUrl: thumb,
          fullUrl: full,
          pageUrl: link,
        ),
      );
    }
    return result;
  }

  String _largerUrl(String url) {
    if (url.isEmpty) return url;
    return url.replaceFirst(
      RegExp(r'_[mqsnz]\.(jpg|jpeg|png)$'),
      r'_b.$1',
    );
  }
}

class ImageAtlasApp extends StatefulWidget {
  const ImageAtlasApp({super.key});

  @override
  State<ImageAtlasApp> createState() => _ImageAtlasAppState();
}

class _ImageAtlasAppState extends State<ImageAtlasApp> {
  MatureMode _mode = MatureMode.blur;
  final Map<String, ImageItem> _favorites = <String, ImageItem>{};
  List<String> _history = const <String>[];
  bool _loaded = false;

  @override
  void initState() {
    super.initState();
    unawaited(_restore());
  }

  Future<void> _restore() async {
    final prefs = await SharedPreferences.getInstance();
    final savedName = prefs.getString('matureMode');
    MatureMode? savedMode;
    for (final mode in MatureMode.values) {
      if (mode.name == savedName) savedMode = mode;
    }

    final favorites = <String, ImageItem>{};
    for (final raw in prefs.getStringList('favorites') ?? const <String>[]) {
      try {
        final decoded = jsonDecode(raw);
        if (decoded is Map<String, dynamic>) {
          final item = ImageItem.fromJson(decoded);
          favorites[item.identity] = item;
        }
      } on Object {
        // Ignore one corrupt entry without losing the rest.
      }
    }

    if (!mounted) return;
    setState(() {
      _mode = savedMode ?? MatureMode.blur;
      _favorites
        ..clear()
        ..addAll(favorites);
      _history = prefs.getStringList('history') ?? const <String>[];
      _loaded = true;
    });
  }

  Future<void> _setMode(MatureMode value) async {
    if (value == MatureMode.show && _mode != MatureMode.show) {
      final confirmed = await showDialog<bool>(
        context: context,
        builder: (context) => AlertDialog(
          title: const Text('Adult content setting'),
          content: const Text(
            'Show requests source-labelled sensitive results where a provider '
            'supports that option. Continue only if you are an adult.',
          ),
          actions: <Widget>[
            TextButton(
              onPressed: () => Navigator.pop(context, false),
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () => Navigator.pop(context, true),
              child: const Text('I am 18+'),
            ),
          ],
        ),
      );
      if (confirmed != true) return;
    }
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString('matureMode', value.name);
    if (mounted) setState(() => _mode = value);
  }

  Future<void> _toggleFavorite(ImageItem item) async {
    setState(() {
      if (_favorites.containsKey(item.identity)) {
        _favorites.remove(item.identity);
      } else {
        _favorites[item.identity] = item;
      }
    });
    final prefs = await SharedPreferences.getInstance();
    await prefs.setStringList(
      'favorites',
      _favorites.values.map((item) => jsonEncode(item.toJson())).toList(),
    );
  }

  Future<void> _recordSearch(String query) async {
    final clean = query.trim();
    if (clean.isEmpty) return;
    final next = <String>[
      clean,
      ..._history.where(
        (item) => item.toLowerCase() != clean.toLowerCase(),
      ),
    ].take(15).toList(growable: false);
    final prefs = await SharedPreferences.getInstance();
    await prefs.setStringList('history', next);
    if (mounted) setState(() => _history = next);
  }

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'ImageAtlas',
      debugShowCheckedModeBanner: false,
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(
          seedColor: const Color(0xff7657ff),
          brightness: Brightness.dark,
        ),
        useMaterial3: true,
      ),
      home: !_loaded
          ? const Scaffold(
              body: Center(child: CircularProgressIndicator()),
            )
          : HomeShell(
              matureMode: _mode,
              setMatureMode: _setMode,
              favorites: _favorites,
              toggleFavorite: _toggleFavorite,
              history: _history,
              recordSearch: _recordSearch,
            ),
    );
  }
}

class HomeShell extends StatefulWidget {
  const HomeShell({
    super.key,
    required this.matureMode,
    required this.setMatureMode,
    required this.favorites,
    required this.toggleFavorite,
    required this.history,
    required this.recordSearch,
  });

  final MatureMode matureMode;
  final Future<void> Function(MatureMode) setMatureMode;
  final Map<String, ImageItem> favorites;
  final Future<void> Function(ImageItem) toggleFavorite;
  final List<String> history;
  final Future<void> Function(String) recordSearch;

  @override
  State<HomeShell> createState() => _HomeShellState();
}

class _HomeShellState extends State<HomeShell> {
  var _index = 0;

  @override
  Widget build(BuildContext context) {
    final pages = <Widget>[
      SearchPage(
        matureMode: widget.matureMode,
        favorites: widget.favorites,
        toggleFavorite: widget.toggleFavorite,
        history: widget.history,
        recordSearch: widget.recordSearch,
      ),
      FavoritesPage(
        matureMode: widget.matureMode,
        favorites: widget.favorites,
        toggleFavorite: widget.toggleFavorite,
      ),
      SettingsPage(
        matureMode: widget.matureMode,
        setMatureMode: widget.setMatureMode,
      ),
    ];
    return Scaffold(
      body: IndexedStack(index: _index, children: pages),
      bottomNavigationBar: NavigationBar(
        selectedIndex: _index,
        onDestinationSelected: (value) => setState(() => _index = value),
        destinations: const <NavigationDestination>[
          NavigationDestination(icon: Icon(Icons.search), label: 'Search'),
          NavigationDestination(
            icon: Icon(Icons.favorite_outline),
            selectedIcon: Icon(Icons.favorite),
            label: 'Favorites',
          ),
          NavigationDestination(icon: Icon(Icons.tune), label: 'Settings'),
        ],
      ),
    );
  }
}

class SearchPage extends StatefulWidget {
  const SearchPage({
    super.key,
    required this.matureMode,
    required this.favorites,
    required this.toggleFavorite,
    required this.history,
    required this.recordSearch,
  });

  final MatureMode matureMode;
  final Map<String, ImageItem> favorites;
  final Future<void> Function(ImageItem) toggleFavorite;
  final List<String> history;
  final Future<void> Function(String) recordSearch;

  @override
  State<SearchPage> createState() => _SearchPageState();
}

class _SearchPageState extends State<SearchPage>
    with AutomaticKeepAliveClientMixin {
  final TextEditingController _query = TextEditingController();
  final ScrollController _scroll = ScrollController();
  final List<ImageSource> _sources = const <ImageSource>[
    OpenverseSource(),
    WikimediaSource(),
    FlickrFeedSource(),
  ];
  final Map<String, int> _pages = <String, int>{};
  final Set<String> _exhausted = <String>{};
  final Set<String> _revealed = <String>{};
  List<ImageItem> _items = const <ImageItem>[];
  bool _loading = false;
  String _activeQuery = '';
  String? _message;

  @override
  bool get wantKeepAlive => true;

  @override
  void initState() {
    super.initState();
    _scroll.addListener(() {
      if (_scroll.hasClients &&
          _scroll.position.pixels >=
              _scroll.position.maxScrollExtent - 900) {
        unawaited(_loadMore());
      }
    });
  }

  @override
  void didUpdateWidget(covariant SearchPage oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.matureMode != widget.matureMode &&
        _activeQuery.isNotEmpty) {
      unawaited(_startSearch(_activeQuery));
    }
  }

  @override
  void dispose() {
    _query.dispose();
    _scroll.dispose();
    super.dispose();
  }

  Future<void> _startSearch(String raw) async {
    final query = raw.trim();
    if (query.isEmpty || _loading) return;
    FocusManager.instance.primaryFocus?.unfocus();
    await widget.recordSearch(query);
    setState(() {
      _activeQuery = query;
      _items = const <ImageItem>[];
      _pages
        ..clear()
        ..addEntries(_sources.map((source) => MapEntry(source.name, 1)));
      _exhausted.clear();
      _revealed.clear();
      _message = null;
    });
    await _loadMore();
  }

  Future<void> _loadMore() async {
    if (_loading || _activeQuery.isEmpty) return;
    final active = _sources
        .where((source) => !_exhausted.contains(source.name))
        .toList();
    if (active.isEmpty) return;

    setState(() => _loading = true);
    final includeSensitive = widget.matureMode != MatureMode.hide;
    final futures = active.map((source) async {
      final page = _pages[source.name] ?? 1;
      try {
        final items = await source.search(
          query: _activeQuery,
          page: page,
          includeSensitive: includeSensitive,
        );
        return (
          source: source,
          page: page,
          items: items,
          error: null as Object?,
        );
      } on Object catch (error) {
        return (
          source: source,
          page: page,
          items: const <ImageItem>[],
          error: error as Object?,
        );
      }
    });

    final results = await Future.wait(futures);
    if (!mounted) return;

    final appended = <ImageItem>[..._items];
    final failures = <String>[];
    for (final result in results) {
      if (result.error != null) {
        failures.add(result.source.name);
        continue;
      }
      if (result.items.isEmpty) {
        _exhausted.add(result.source.name);
      } else {
        appended.addAll(result.items);
        _pages[result.source.name] = result.page + 1;
      }
    }

    final deduped = dedupeImages(appended);
    deduped.sort((a, b) {
      return _score(b, _activeQuery).compareTo(_score(a, _activeQuery));
    });

    setState(() {
      _items = deduped;
      _message = failures.isEmpty
          ? null
          : 'Some sources are temporarily unavailable: '
              '${failures.join(', ')}';
      _loading = false;
    });
  }

  int _score(ImageItem item, String query) {
    final title = item.title.toLowerCase();
    final haystack = '${item.title} ${item.creator}'.toLowerCase();
    var score = 0;
    for (final token in query
        .toLowerCase()
        .split(RegExp(r'\s+'))
        .where((token) => token.isNotEmpty)) {
      if (haystack.contains(token)) score += 3;
      if (title.contains(token)) score += 2;
    }
    if (item.source == 'Openverse') score += 1;
    return score;
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    return Scaffold(
      appBar: AppBar(
        title: const Text('ImageAtlas'),
        actions: <Widget>[
          Padding(
            padding: const EdgeInsets.only(right: 12),
            child: Center(
              child: Chip(
                avatar: const Icon(Icons.visibility, size: 16),
                label: Text(widget.matureMode.label),
              ),
            ),
          ),
        ],
      ),
      body: Column(
        children: <Widget>[
          Padding(
            padding: const EdgeInsets.fromLTRB(12, 4, 12, 8),
            child: SearchBar(
              controller: _query,
              hintText: 'Search images across sources',
              leading: const Icon(Icons.search),
              trailing: <Widget>[
                IconButton(
                  onPressed: () => _startSearch(_query.text),
                  icon: const Icon(Icons.arrow_forward),
                ),
              ],
              onSubmitted: _startSearch,
            ),
          ),
          if (_activeQuery.isEmpty && widget.history.isNotEmpty)
            SizedBox(
              height: 46,
              child: ListView.separated(
                padding: const EdgeInsets.symmetric(horizontal: 12),
                scrollDirection: Axis.horizontal,
                itemCount: widget.history.length,
                separatorBuilder: (_, __) => const SizedBox(width: 8),
                itemBuilder: (context, index) {
                  final value = widget.history[index];
                  return ActionChip(
                    label: Text(value),
                    onPressed: () {
                      _query.text = value;
                      unawaited(_startSearch(value));
                    },
                  );
                },
              ),
            ),
          if (_message != null)
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
              child: Text(
                _message!,
                style: TextStyle(color: Theme.of(context).colorScheme.error),
              ),
            ),
          Expanded(
            child: _activeQuery.isEmpty
                ? const EmptyState(
                    icon: Icons.travel_explore,
                    title: 'Search several image sources at once',
                    subtitle:
                        'Openverse, Wikimedia Commons and Flickr public photos are enabled.',
                  )
                : _items.isEmpty && _loading
                    ? const Center(child: CircularProgressIndicator())
                    : _items.isEmpty
                        ? const EmptyState(
                            icon: Icons.image_not_supported_outlined,
                            title: 'No results',
                            subtitle: 'Try a broader or different query.',
                          )
                        : RefreshIndicator(
                            onRefresh: () => _startSearch(_activeQuery),
                            child: ImageGrid(
                              controller: _scroll,
                              items: _items,
                              matureMode: widget.matureMode,
                              favorites: widget.favorites,
                              revealed: _revealed,
                              onReveal: (item) {
                                setState(() => _revealed.add(item.identity));
                              },
                              toggleFavorite: widget.toggleFavorite,
                            ),
                          ),
          ),
          if (_loading && _items.isNotEmpty)
            const Padding(
              padding: EdgeInsets.all(8),
              child: LinearProgressIndicator(),
            ),
        ],
      ),
    );
  }
}

class ImageGrid extends StatelessWidget {
  const ImageGrid({
    super.key,
    required this.items,
    required this.matureMode,
    required this.favorites,
    required this.revealed,
    required this.onReveal,
    required this.toggleFavorite,
    this.controller,
  });

  final List<ImageItem> items;
  final MatureMode matureMode;
  final Map<String, ImageItem> favorites;
  final Set<String> revealed;
  final void Function(ImageItem) onReveal;
  final Future<void> Function(ImageItem) toggleFavorite;
  final ScrollController? controller;

  @override
  Widget build(BuildContext context) {
    return GridView.builder(
      controller: controller,
      physics: const AlwaysScrollableScrollPhysics(),
      padding: const EdgeInsets.all(8),
      gridDelegate: const SliverGridDelegateWithFixedCrossAxisCount(
        crossAxisCount: 2,
        childAspectRatio: 0.78,
        crossAxisSpacing: 8,
        mainAxisSpacing: 8,
      ),
      itemCount: items.length,
      itemBuilder: (context, index) {
        final item = items[index];
        final shouldBlur = matureMode == MatureMode.blur &&
            item.isSensitive &&
            !revealed.contains(item.identity);
        return Card(
          clipBehavior: Clip.antiAlias,
          margin: EdgeInsets.zero,
          child: InkWell(
            onTap: shouldBlur
                ? () => onReveal(item)
                : () => Navigator.of(context).push(
                      MaterialPageRoute<void>(
                        builder: (_) => DetailPage(
                          item: item,
                          isFavorite: favorites.containsKey(item.identity),
                          toggleFavorite: toggleFavorite,
                        ),
                      ),
                    ),
            child: Stack(
              fit: StackFit.expand,
              children: <Widget>[
                NetworkImageTile(url: item.thumbnailUrl, blurred: shouldBlur),
                Positioned(
                  top: 6,
                  left: 6,
                  child: Chip(
                    visualDensity: VisualDensity.compact,
                    label: Text(item.source),
                  ),
                ),
                if (item.isSensitive)
                  const Positioned(
                    top: 6,
                    right: 48,
                    child: Chip(
                      visualDensity: VisualDensity.compact,
                      label: Text('Sensitive'),
                    ),
                  ),
                Positioned(
                  top: 4,
                  right: 4,
                  child: IconButton.filledTonal(
                    onPressed: () => toggleFavorite(item),
                    icon: Icon(
                      favorites.containsKey(item.identity)
                          ? Icons.favorite
                          : Icons.favorite_border,
                    ),
                  ),
                ),
                if (shouldBlur)
                  const Center(
                    child: Card(
                      child: Padding(
                        padding:
                            EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                        child: Text('Sensitive · tap to reveal'),
                      ),
                    ),
                  ),
                Positioned(
                  left: 0,
                  right: 0,
                  bottom: 0,
                  child: Container(
                    padding: const EdgeInsets.fromLTRB(10, 26, 10, 8),
                    decoration: const BoxDecoration(
                      gradient: LinearGradient(
                        begin: Alignment.topCenter,
                        end: Alignment.bottomCenter,
                        colors: <Color>[Colors.transparent, Colors.black87],
                      ),
                    ),
                    child: Text(
                      item.title.isEmpty ? 'Untitled' : item.title,
                      maxLines: 2,
                      overflow: TextOverflow.ellipsis,
                    ),
                  ),
                ),
              ],
            ),
          ),
        );
      },
    );
  }
}

class NetworkImageTile extends StatelessWidget {
  const NetworkImageTile({
    super.key,
    required this.url,
    required this.blurred,
  });

  final String url;
  final bool blurred;

  @override
  Widget build(BuildContext context) {
    final image = Image.network(
      url,
      fit: BoxFit.cover,
      headers: Net.headers,
      errorBuilder: (_, __, ___) => const ColoredBox(
        color: Color(0xff20232a),
        child: Center(child: Icon(Icons.broken_image_outlined, size: 42)),
      ),
      loadingBuilder: (context, child, progress) => progress == null
          ? child
          : const ColoredBox(
              color: Color(0xff20232a),
              child: Center(child: CircularProgressIndicator()),
            ),
    );
    if (!blurred) return image;
    return ImageFiltered(
      imageFilter: ui.ImageFilter.blur(sigmaX: 24, sigmaY: 24),
      child: image,
    );
  }
}

class FavoritesPage extends StatefulWidget {
  const FavoritesPage({
    super.key,
    required this.matureMode,
    required this.favorites,
    required this.toggleFavorite,
  });

  final MatureMode matureMode;
  final Map<String, ImageItem> favorites;
  final Future<void> Function(ImageItem) toggleFavorite;

  @override
  State<FavoritesPage> createState() => _FavoritesPageState();
}

class _FavoritesPageState extends State<FavoritesPage> {
  final Set<String> _revealed = <String>{};

  @override
  Widget build(BuildContext context) {
    final items = widget.favorites.values.toList(growable: false);
    return Scaffold(
      appBar: AppBar(title: const Text('Favorites')),
      body: items.isEmpty
          ? const EmptyState(
              icon: Icons.favorite_border,
              title: 'No favorites yet',
              subtitle: 'Tap the heart on any image to save it here.',
            )
          : ImageGrid(
              items: items,
              matureMode: widget.matureMode,
              favorites: widget.favorites,
              revealed: _revealed,
              onReveal: (item) {
                setState(() => _revealed.add(item.identity));
              },
              toggleFavorite: widget.toggleFavorite,
            ),
    );
  }
}

class SettingsPage extends StatelessWidget {
  const SettingsPage({
    super.key,
    required this.matureMode,
    required this.setMatureMode,
  });

  final MatureMode matureMode;
  final Future<void> Function(MatureMode) setMatureMode;

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Settings')),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: <Widget>[
          Text(
            'Sensitive content',
            style: Theme.of(context).textTheme.titleLarge,
          ),
          const SizedBox(height: 8),
          const Text(
            'Uses each source’s supported sensitivity controls. The app does '
            'not bypass logins, private content, age gates or provider rules.',
          ),
          const SizedBox(height: 16),
          SegmentedButton<MatureMode>(
            segments: MatureMode.values
                .map(
                  (mode) => ButtonSegment<MatureMode>(
                    value: mode,
                    label: Text(mode.label),
                  ),
                )
                .toList(growable: false),
            selected: <MatureMode>{matureMode},
            onSelectionChanged: (selection) {
              if (selection.isNotEmpty) {
                unawaited(setMatureMode(selection.first));
              }
            },
          ),
          const SizedBox(height: 24),
          const ListTile(
            contentPadding: EdgeInsets.zero,
            leading: Icon(Icons.hub_outlined),
            title: Text('Enabled sources'),
            subtitle: Text(
              'Openverse · Wikimedia Commons · Flickr public feed',
            ),
          ),
          const ListTile(
            contentPadding: EdgeInsets.zero,
            leading: Icon(Icons.layers_outlined),
            title: Text('Broad search'),
            subtitle: Text(
              'Openverse itself indexes multiple upstream media providers, '
              'while the other adapters add direct source coverage.',
            ),
          ),
        ],
      ),
    );
  }
}

class DetailPage extends StatefulWidget {
  const DetailPage({
    super.key,
    required this.item,
    required this.isFavorite,
    required this.toggleFavorite,
  });

  final ImageItem item;
  final bool isFavorite;
  final Future<void> Function(ImageItem) toggleFavorite;

  @override
  State<DetailPage> createState() => _DetailPageState();
}

class _DetailPageState extends State<DetailPage> {
  static const MethodChannel _downloads =
      MethodChannel('imageatlas/downloads');
  late bool _favorite = widget.isFavorite;
  bool _saving = false;

  Future<void> _save() async {
    if (_saving) return;
    setState(() => _saving = true);
    try {
      http.Response response;
      try {
        response = await Net.get(Uri.parse(widget.item.fullUrl));
        if (response.statusCode < 200 || response.statusCode >= 300) {
          throw StateError('Original returned ${response.statusCode}');
        }
      } on Object {
        response = await Net.get(Uri.parse(widget.item.thumbnailUrl));
        if (response.statusCode < 200 || response.statusCode >= 300) {
          throw StateError('Image returned ${response.statusCode}');
        }
      }

      final mime =
          response.headers['content-type']?.split(';').first ?? 'image/jpeg';
      final extension = switch (mime) {
        'image/png' => 'png',
        'image/webp' => 'webp',
        'image/gif' => 'gif',
        _ => 'jpg',
      };
      final filename =
          'imageatlas_${widget.item.source}_${DateTime.now().millisecondsSinceEpoch}.$extension';

      await _downloads.invokeMethod<String>(
        'saveImage',
        <String, Object>{
          'bytes': Uint8List.fromList(response.bodyBytes),
          'filename': filename,
          'mime': mime,
        },
      );
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('Saved to Pictures/ImageAtlas')),
      );
    } on Object catch (error) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Save failed: $error')),
      );
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  Future<void> _openSource() async {
    final uri = Uri.tryParse(widget.item.pageUrl);
    if (uri == null) return;
    await launchUrl(uri, mode: LaunchMode.externalApplication);
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: Text(widget.item.source),
        actions: <Widget>[
          IconButton(
            onPressed: () async {
              await widget.toggleFavorite(widget.item);
              if (mounted) setState(() => _favorite = !_favorite);
            },
            icon: Icon(_favorite ? Icons.favorite : Icons.favorite_border),
          ),
        ],
      ),
      body: Column(
        children: <Widget>[
          Expanded(
            child: InteractiveViewer(
              minScale: 0.8,
              maxScale: 5,
              child: Center(
                child: Image.network(
                  widget.item.fullUrl,
                  headers: Net.headers,
                  fit: BoxFit.contain,
                  errorBuilder: (_, __, ___) => Image.network(
                    widget.item.thumbnailUrl,
                    headers: Net.headers,
                    fit: BoxFit.contain,
                  ),
                ),
              ),
            ),
          ),
          SafeArea(
            top: false,
            child: Padding(
              padding: const EdgeInsets.fromLTRB(16, 10, 16, 14),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: <Widget>[
                  Text(
                    widget.item.title.isEmpty ? 'Untitled' : widget.item.title,
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                    style: Theme.of(context).textTheme.titleMedium,
                  ),
                  if (widget.item.creator.isNotEmpty) ...<Widget>[
                    const SizedBox(height: 4),
                    Text(
                      widget.item.creator,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                    ),
                  ],
                  const SizedBox(height: 12),
                  Row(
                    children: <Widget>[
                      Expanded(
                        child: FilledButton.icon(
                          onPressed: _saving ? null : _save,
                          icon: _saving
                              ? const SizedBox.square(
                                  dimension: 18,
                                  child: CircularProgressIndicator(
                                    strokeWidth: 2,
                                  ),
                                )
                              : const Icon(Icons.download),
                          label: const Text('Save'),
                        ),
                      ),
                      const SizedBox(width: 10),
                      Expanded(
                        child: OutlinedButton.icon(
                          onPressed: _openSource,
                          icon: const Icon(Icons.open_in_new),
                          label: const Text('Source'),
                        ),
                      ),
                    ],
                  ),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class EmptyState extends StatelessWidget {
  const EmptyState({
    super.key,
    required this.icon,
    required this.title,
    required this.subtitle,
  });

  final IconData icon;
  final String title;
  final String subtitle;

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(32),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: <Widget>[
            Icon(icon, size: 54),
            const SizedBox(height: 14),
            Text(title, style: Theme.of(context).textTheme.titleLarge),
            const SizedBox(height: 8),
            Text(
              subtitle,
              textAlign: TextAlign.center,
              style: Theme.of(context).textTheme.bodyMedium,
            ),
          ],
        ),
      ),
    );
  }
}
