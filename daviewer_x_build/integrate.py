from pathlib import Path

router = Path("daviewer/lib/app/router.dart")
text = router.read_text()
text = text.replace(
    "import '../features/home/home_screen.dart';",
    "import '../features/home/home_screen.dart';\n"
    "import '../features/multisource/models/external_artwork.dart';\n"
    "import '../features/multisource/ui/external_artwork_detail_screen.dart';\n"
    "import '../features/multisource/ui/multisource_discover_screen.dart';",
    1,
)
anchor = """      GoRoute(
        path: '/history',
        builder: (context, state) => const HistoryScreen(),
      ),
"""
extra = """      GoRoute(
        path: '/artsource',
        builder: (context, state) => MultiSourceDiscoverScreen(
          initialQuery: state.uri.queryParameters['q'] ?? '',
          initialSource: _artSourceFromName(state.uri.queryParameters['source']),
        ),
      ),
      GoRoute(
        path: '/artsource/:source/:id',
        builder: (context, state) {
          final source = _artSourceFromName(state.pathParameters['source']);
          if (source == null) {
            return const Scaffold(body: Center(child: Text('Unknown art source.')));
          }
          return ExternalArtworkDetailScreen(
            source: source,
            postId: state.pathParameters['id']!,
          );
        },
      ),
"""
if anchor not in text:
    raise SystemExit("router anchor not found")
text = text.replace(anchor, anchor + extra, 1)
text = text.replace(
    "    location == '/watch' ||",
    "    location == '/watch' ||\n"
    "    location == '/artsource' ||\n"
    "    location.startsWith('/artsource/') ||",
    1,
)
text += """
ExternalSourceKind? _artSourceFromName(String? value) {
  if (value == null) return null;
  for (final source in ExternalSourceKind.values) {
    if (source.name == value) return source;
  }
  return null;
}
"""
router.write_text(text)

home = Path("daviewer/lib/features/home/home_screen.dart")
text = home.read_text()
anchor = """            IconButton(
              tooltip: s.openLinkTooltip,
              onPressed: () => _showOpenLinkDialog(context, s),
              icon: const Icon(Icons.link),
            ),
"""
button = """            IconButton(
              tooltip: 'Art sources',
              onPressed: () => context.push('/artsource'),
              icon: const Icon(Icons.explore_outlined),
            ),
"""
if anchor not in text:
    raise SystemExit("home anchor not found")
home.write_text(text.replace(anchor, anchor + button, 1))
