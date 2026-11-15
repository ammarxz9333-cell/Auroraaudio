"""Integrate DAViewer MultiSource into pinned upstream v0.7.0 after Ultra v0.7.2.

This script is fail-closed: if the upstream code no longer matches the
reviewed anchors, the Android workflow stops without publishing an APK.
"""
from pathlib import Path

ROOT = Path("DAViewer")


def replace_once(relative: str, old: str, new: str) -> None:
    target = ROOT / relative
    source = target.read_text(encoding="utf-8")
    count = source.count(old)
    if count != 1:
        raise SystemExit(
            f"{relative}: expected one anchor, found {count}: {old[:85]!r}"
        )
    target.write_text(source.replace(old, new, 1), encoding="utf-8")


replace_once("pubspec.yaml", "version: 0.7.2+702", "version: 0.8.1+801")

replace_once(
    "lib/app/router.dart",
    "import '../features/search/search_screen.dart';\n",
    "import '../features/search/search_screen.dart';\n"
    "import '../features/multisource/multisource_screen.dart';\n",
)

replace_once(
    "lib/app/router.dart",
    "      GoRoute(path: '/artwork/:id', pageBuilder: artworkDetailPage),\n",
    """      GoRoute(
        path: '/multisource',
        builder: (context, state) => const MultiSourceScreen(),
      ),
      GoRoute(path: '/artwork/:id', pageBuilder: artworkDetailPage),
""",
)

# The public multi-source search must work even before DeviantArt sign-in.
replace_once(
    "lib/app/router.dart",
    "    location == '/watch' ||\n",
    "    location == '/watch' ||\n"
    "    location == '/multisource' ||\n",
)

# A shortcut on the public Home tab means signed-out users can find it.
replace_once(
    "lib/features/home/home_screen.dart",
    "          actions: <Widget>[\n",
    """          actions: <Widget>[
            IconButton(
              tooltip: 'Search art across sites',
              icon: const Icon(Icons.collections_outlined),
              onPressed: () => context.push('/multisource'),
            ),
""",
)

# Existing DeviantArt search remains untouched, with an additional shortcut.
replace_once(
    "lib/features/search/search_screen.dart",
    "        actions: <Widget>[\n",
    """        actions: <Widget>[
          IconButton(
            tooltip: 'Search other art sites',
            icon: const Icon(Icons.collections_outlined),
            onPressed: () => context.push('/multisource'),
          ),
""",
)

for relative in (
    "lib/features/multisource/gallery_catalog.dart",
    "lib/features/multisource/reddit_gallery_search.dart",
    "lib/features/multisource/multisource_screen.dart",
    "test/gallery_catalog_test.dart",
    "test/reddit_gallery_search_test.dart",
    "test/multisource_screen_test.dart",
):
    if not (ROOT / relative).is_file():
        raise SystemExit(f"Missing multisource file {relative}")

print("DAViewer MultiSource v0.8.1: startup fixes and providers installed.")
