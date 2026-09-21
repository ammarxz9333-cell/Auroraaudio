from pathlib import Path

p = Path("DAViewer/lib/features/settings/settings_screen.dart")
s = p.read_text()
s = s.replace(
    "import '../../core/data/web_session.dart';\n",
    "import '../../core/cache/artwork_page_cache.dart';\nimport '../../core/data/web_session.dart';\n",
    1,
)
old = """  await DefaultCacheManager().emptyCache();
  if (context.mounted) {"""
new = """  await DefaultCacheManager().emptyCache();
  await ArtworkPageCacheStore.clear();
  if (context.mounted) {"""
if old not in s:
    raise SystemExit("clear cache anchor changed")
p.write_text(s.replace(old, new, 1))
