part of '../main.dart';

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
            : msg.contains('SOURCE_ACCESS_BLOCKED')
                ? '${source.label} is refusing direct API access. Use Rule34Vault/XYZ or yande.re for now.'
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
        title: const Text('PixVault  ·  Explore'),
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
                gridDelegate: const SliverGridDelegateWithMaxCrossAxisExtent(
                  maxCrossAxisExtent: 230,
                  childAspectRatio: .72,
                  crossAxisSpacing: 6,
                  mainAxisSpacing: 6,
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
        title: Text('${a.source.label}  ·  ${a.title}'),
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
                title: Text('${d['title'] ?? 'Download'}', maxLines: 1, overflow: TextOverflow.ellipsis),
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
            title: Text('PixVault 0.3.0'),
            subtitle: Text(
              'Modular v0.3 build. Verified sources are kept behind a common repository layer; Pixiv requires login. Sources blocked by authentication or anti-bot protection remain hidden instead of failing silently.',
            ),
          ),
        ],
      ),
    );
  }
}
