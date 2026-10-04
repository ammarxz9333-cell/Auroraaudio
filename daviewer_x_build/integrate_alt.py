from pathlib import Path

router = Path("daviewer/lib/app/router.dart")
s = router.read_text()
s = s.replace(
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
          return ExternalArtworkDetailScreen(source: source, postId: state.pathParameters['id']!);
        },
      ),
"""
s = s.replace(anchor, anchor + extra, 1)
s = s.replace(
    "    location == '/watch' ||",
    "    location == '/watch' ||\n"
    "    location == '/artsource' ||\n"
    "    location.startsWith('/artsource/') ||",
    1,
)
s += """
ExternalSourceKind? _artSourceFromName(String? value) {
  if (value == null) return null;
  for (final source in ExternalSourceKind.values) {
    if (source.name == value) return source;
  }
  return null;
}
"""
router.write_text(s)

home = Path("daviewer/lib/features/home/home_screen.dart")
s = home.read_text()
needle = """            IconButton(
              tooltip: s.openLinkTooltip,
              onPressed: () => _showOpenLinkDialog(context, s),
              icon: const Icon(Icons.link),
            ),
"""
add = """            IconButton(
              tooltip: 'Art sources',
              onPressed: () => context.push('/artsource'),
              icon: const Icon(Icons.explore_outlined),
            ),
"""
home.write_text(s.replace(needle, needle + add, 1))

gradle = Path("daviewer/android/app/build.gradle.kts")
s = gradle.read_text()
a = s.index("            signingConfig = if (keystorePropertiesFile.exists()) {")
b = s.index("        }\n    }\n}", a)
replacement = """            signingConfig = if (keystorePropertiesFile.exists()) {
                signingConfigs.getByName("release")
            } else {
                signingConfigs.getByName("debug")
            }
"""
s = s[:a] + replacement + s[b:]
gradle.write_text(s)
