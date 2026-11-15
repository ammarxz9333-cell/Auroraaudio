import 'dart:async';

import 'package:cached_network_image/cached_network_image.dart';
import 'package:flutter/material.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:url_launcher/url_launcher.dart';

import 'gallery_catalog.dart';
import 'reddit_gallery_search.dart';

/// Search images across independent sources without depending on DA OAuth.
///
/// Public APIs are searched with safety filters. Reddit has a separate,
/// explicitly configured *approved* OAuth bearer token. Pixiv/other sites
/// are opened in their own browser until a supported API integration exists.
final class MultiSourceScreen extends StatefulWidget {
  const MultiSourceScreen({super.key});

  @override
  State<MultiSourceScreen> createState() => _MultiSourceScreenState();
}

final class _MultiSourceScreenState extends State<MultiSourceScreen> {
  static const _storage = FlutterSecureStorage();
  static const _tokenKey = 'multisource.reddit.oauth_access_token';
  static const _agentKey = 'multisource.reddit.user_agent';
  final _catalog = GalleryCatalog();
  final _queryField = TextEditingController();
  final List<GalleryItem> _items = <GalleryItem>[];
  final Map<GallerySource, bool> _sourceMore = <GallerySource, bool>{};

  GallerySource _source = GallerySource.all;
  String _query = '';
  String? _nextRedditCursor;
  bool _adultEnabled = false;
  bool _loading = false;
  String? _error;
  int _page = 0;
  int _generation = 0;

  @override
  void dispose() {
    _generation++;
    _queryField.dispose();
    super.dispose();
  }

  Future<void> _search({bool reset = false}) async {
    final query = _query.trim();
    if (query.isEmpty || _loading && !reset) return;
    final generation = ++_generation;
    if (reset) {
      _items.clear();
      _sourceMore.clear();
      _page = 0;
      _nextRedditCursor = null;
    }
    final nextPage = _page + 1;
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      if (_source == GallerySource.reddit) {
        final token = await _storage.read(key: _tokenKey);
        final agent = await _storage.read(key: _agentKey);
        if (token == null || token.isEmpty ||
            agent == null || agent.isEmpty) {
          throw StateError('Configure an approved Reddit OAuth session first.');
        }
        final reddit = RedditGallerySearch(
          accessToken: () => _storage.read(key: _tokenKey),
          userAgent: agent,
        );
        final results = await reddit.search(
          query,
          after: _nextRedditCursor,
        );
        if (!mounted || generation != _generation) return;
        setState(() {
          _items.addAll(results.items);
          _nextRedditCursor = results.after;
          _page = nextPage;
        });
      } else {
        final sources = _source == GallerySource.all
            ? safeGallerySources
            : <GallerySource>[_source];
        final active = sources.where((s) =>
            nextPage == 1 || (_sourceMore[s] ?? false)).toList();
        final results = await Future.wait(
          active.map((source) async {
            try {
              final page = await _catalog.search(
                source,
                query,
                nextPage,
                adultEnabled: _adultEnabled,
              );
              return (source: source, page: page, error: null as Object?);
            } catch (error) {
              return (
                source: source,
                page: null as GalleryPage?,
                error: error as Object?,
              );
            }
          }),
        );
        if (!mounted || generation != _generation) return;
        final errors = <String>[];
        setState(() {
          for (final result in results) {
            if (result.page != null) {
              _items.addAll(result.page!.items);
              _sourceMore[result.source] = result.page!.hasMore;
            } else {
              // Retry failures rather than treating them as permanent EOF.
              _sourceMore[result.source] = true;
              errors.add(result.source.label + ': ' +
                  result.error.toString());
            }
          }
          _page = nextPage;
          if (errors.isNotEmpty) {
            _error = errors.join('\n');
          }
        });
      }
    } catch (error) {
      if (!mounted || generation != _generation) return;
      setState(() => _error = error.toString());
    } finally {
      if (mounted && generation == _generation) {
        setState(() => _loading = false);
      }
    }
  }

  bool get _hasMore {
    if (_source == GallerySource.reddit) {
      return _nextRedditCursor != null && _nextRedditCursor!.isNotEmpty;
    }
    if (_page == 0) return false;
    final sources = _source == GallerySource.all
        ? safeGallerySources
        : <GallerySource>[_source];
    return sources.any((source) => _sourceMore[source] ?? false);
  }

  void _submit(String value) {
    final query = value.trim();
    if (query.isEmpty) return;
    _queryField.text = query;
    setState(() => _query = query);
    unawaited(_search(reset: true));
  }

  void _setSource(GallerySource source) {
    if (source == GallerySource.rule34 && !_adultEnabled) return;
    if (source == _source) return;
    _generation++;
    setState(() {
      _source = source;
      _items.clear();
      _sourceMore.clear();
      _page = 0;
      _nextRedditCursor = null;
      _error = null;
      _loading = false;
    });
    if (_query.isNotEmpty) unawaited(_search(reset: true));
  }

  Future<void> _configureReddit() async {
    final tokenController = TextEditingController();
    final agentController = TextEditingController(
      text: await _storage.read(key: _agentKey) ?? '',
    );
    if (!mounted) {
      tokenController.dispose();
      agentController.dispose();
      return;
    }
    final save = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Reddit approved API access'),
        content: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: <Widget>[
              const Text(
                'Reddit requires app approval and an OAuth session. '
                'Enter a short-lived access token from your approved app. '
                'Never enter a Reddit password or client secret here. '
                'Tokens are stored only in Android secure storage.',
              ),
              const SizedBox(height: 12),
              TextField(
                controller: tokenController,
                obscureText: true,
                decoration: const InputDecoration(
                  labelText: 'OAuth access token',
                ),
              ),
              const SizedBox(height: 10),
              TextField(
                controller: agentController,
                decoration: const InputDecoration(
                  labelText: 'Approved User-Agent',
                  hintText:
                      'android:your.app.id:0.8 (by /u/your_reddit_name)',
                ),
              ),
            ],
          ),
        ),
        actions: <Widget>[
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Save on device'),
          ),
        ],
      ),
    );
    if (save == true &&
        tokenController.text.trim().isNotEmpty &&
        agentController.text.trim().isNotEmpty) {
      await _storage.write(
        key: _tokenKey,
        value: tokenController.text.trim(),
      );
      await _storage.write(
        key: _agentKey,
        value: agentController.text.trim(),
      );
      if (mounted && _source == GallerySource.reddit && _query.isNotEmpty) {
        unawaited(_search(reset: true));
      }
    }
    tokenController.dispose();
    agentController.dispose();
  }

  Future<void> _toggleAdult(bool enabled) async {
    if (enabled) {
      final confirmed = await showDialog<bool>(
        context: context,
        builder: (context) => AlertDialog(
          title: const Text('Enable adult artwork source?'),
          content: const Text(
            'Rule34 contains explicit illustrated sexual content. '
            'It is excluded from the combined search and only appears '
            'when selected separately. Confirm that you are an adult.',
          ),
          actions: <Widget>[
            TextButton(
              onPressed: () => Navigator.pop(context, false),
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () => Navigator.pop(context, true),
              child: const Text('Enable'),
            ),
          ],
        ),
      );
      if (confirmed != true) return;
    }
    setState(() {
      _adultEnabled = enabled;
      if (!enabled && _source == GallerySource.rule34) {
        _source = GallerySource.all;
      }
    });
  }

  Future<void> _openExternal(Uri url) async {
    if (url.scheme != 'https') return;
    await launchUrl(url, mode: LaunchMode.externalApplication);
  }

  Uri _externalSearch(String site) {
    final term = _queryField.text.trim();
    return switch (site) {
      'Pixiv' => Uri.parse(
          'https://www.pixiv.net/en/tags/' +
              Uri.encodeComponent(term) + '/artworks'),
      'ArtStation' => Uri.https('www.artstation.com', '/search', {
          'query': term,
          'sort_by': 'relevance',
        }),
      'Behance' => Uri.https('www.behance.net', '/search/projects', {
          'search': term,
        }),
      'Zerochan' => Uri.https('www.zerochan.net', '/search', {
          'q': term,
        }),
      'Gelbooru' => Uri.https('gelbooru.com', '/index.php', {
          'page': 'post',
          's': 'list',
          'tags': term,
        }),
      _ => Uri.https('www.reddit.com', '/search', {'q': term}),
    };
  }

  void _showImage(GalleryItem item) {
    showDialog<void>(
      context: context,
      builder: (context) => Dialog(
        insetPadding: const EdgeInsets.all(12),
        child: SafeArea(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: <Widget>[
              SizedBox(
                height: MediaQuery.sizeOf(context).height * .68,
                child: InteractiveViewer(
                  minScale: 0.8,
                  maxScale: 6,
                  child: CachedNetworkImage(
                    imageUrl: item.fullUrl.toString(),
                    fit: BoxFit.contain,
                    errorWidget: (_, _, _) =>
                        const Center(child: Icon(Icons.broken_image)),
                  ),
                ),
              ),
              ListTile(
                title: Text(item.title, maxLines: 2),
                subtitle: Text(item.source.label +
                    (item.creator == null ? '' : ' · ' + item.creator!)),
                trailing: IconButton(
                  icon: const Icon(Icons.open_in_new),
                  tooltip: 'Open original post',
                  onPressed: () => _openExternal(item.pageUrl),
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
    final sources = <GallerySource>[
      GallerySource.all,
      ...safeGallerySources,
      GallerySource.reddit,
      if (_adultEnabled) GallerySource.rule34,
    ];
    final colorScheme = Theme.of(context).colorScheme;
    return Scaffold(
      appBar: AppBar(
        title: const Text('DAViewer MultiSource'),
        actions: <Widget>[
          IconButton(
            tooltip: 'Reddit approved OAuth',
            onPressed: _configureReddit,
            icon: const Icon(Icons.key_outlined),
          ),
        ],
      ),
      body: Column(
        children: <Widget>[
          Padding(
            padding: const EdgeInsets.fromLTRB(12, 8, 12, 4),
            child: TextField(
              controller: _queryField,
              textInputAction: TextInputAction.search,
              onSubmitted: _submit,
              decoration: InputDecoration(
                labelText: 'Search art from multiple sites',
                prefixIcon: const Icon(Icons.image_search),
                suffixIcon: IconButton(
                  tooltip: 'Search',
                  onPressed: () => _submit(_queryField.text),
                  icon: const Icon(Icons.search),
                ),
              ),
            ),
          ),
          SizedBox(
            height: 54,
            child: ListView(
              scrollDirection: Axis.horizontal,
              padding: const EdgeInsets.symmetric(horizontal: 10),
              children: <Widget>[
                for (final source in sources)
                  Padding(
                    padding: const EdgeInsets.only(right: 6),
                    child: ChoiceChip(
                      label: Text(source.label),
                      selected: _source == source,
                      onSelected: (_) => _setSource(source),
                    ),
                  ),
              ],
            ),
          ),
          SizedBox(
            height: 44,
            child: ListView(
              scrollDirection: Axis.horizontal,
              padding: const EdgeInsets.symmetric(horizontal: 10),
              children: <Widget>[
                for (final name in <String>[
                  'Pixiv', 'ArtStation', 'Behance', 'Zerochan', 'Gelbooru'
                ])
                  TextButton.icon(
                    onPressed: () => _openExternal(_externalSearch(name)),
                    icon: const Icon(Icons.open_in_new, size: 15),
                    label: Text(name),
                  ),
              ],
            ),
          ),
          if (_adultEnabled || _source == GallerySource.all)
            Row(
              mainAxisAlignment: MainAxisAlignment.end,
              children: <Widget>[
                const Text('Adult drawings (separate source)'),
                Switch(
                  value: _adultEnabled,
                  onChanged: _toggleAdult,
                ),
              ],
            ),
          if (_error != null)
            Padding(
              padding: const EdgeInsets.all(8),
              child: Column(
                children: <Widget>[
                  Text(
                    _error!,
                    maxLines: 3,
                    style: TextStyle(color: colorScheme.error),
                  ),
                  if (_source == GallerySource.reddit)
                    TextButton(
                      onPressed: _configureReddit,
                      child: const Text('Configure Reddit OAuth'),
                    ),
                ],
              ),
            ),
          Expanded(
            child: _items.isEmpty
                ? Center(
                    child: _loading
                        ? const CircularProgressIndicator()
                        : Text(
                            _query.isEmpty
                                ? 'Enter a search to discover artwork'
                                : 'No matching images or source unavailable',
                          ),
                  )
                : GridView.builder(
                    padding: const EdgeInsets.all(8),
                    gridDelegate:
                        const SliverGridDelegateWithMaxCrossAxisExtent(
                      maxCrossAxisExtent: 220,
                      mainAxisSpacing: 8,
                      crossAxisSpacing: 8,
                      childAspectRatio: .74,
                    ),
                    itemCount: _items.length,
                    itemBuilder: (context, index) {
                      final item = _items[index];
                      return InkWell(
                        onTap: () => _showImage(item),
                        child: ClipRRect(
                          borderRadius: BorderRadius.circular(10),
                          child: Stack(
                            fit: StackFit.expand,
                            children: <Widget>[
                              CachedNetworkImage(
                                imageUrl: item.previewUrl.toString(),
                                fit: BoxFit.cover,
                                errorWidget: (_, _, _) =>
                                    const Icon(Icons.broken_image),
                              ),
                              Align(
                                alignment: Alignment.bottomCenter,
                                child: ColoredBox(
                                  color: Colors.black54,
                                  child: SizedBox(
                                    width: double.infinity,
                                    child: Padding(
                                      padding: const EdgeInsets.all(5),
                                      child: Text(
                                        item.source.label,
                                        style: const TextStyle(
                                          color: Colors.white,
                                          fontSize: 11,
                                        ),
                                      ),
                                    ),
                                  ),
                                ),
                              ),
                            ],
                          ),
                        ),
                      );
                    },
                  ),
          ),
          if (_loading) const LinearProgressIndicator(),
          if (!_loading && _hasMore && _query.isNotEmpty)
            Padding(
              padding: const EdgeInsets.all(8),
              child: FilledButton.tonal(
                onPressed: () => _search(),
                child: const Text('Load more images'),
              ),
            ),
        ],
      ),
    );
  }
}
