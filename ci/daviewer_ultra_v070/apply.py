"""DAViewer Ultra v0.7.1 migration patches for upstream v0.7.0.

Each anchor is checked. A changed upstream source fails CI rather than silently
building a partially modified app. All changes affect the CI checkout only.
"""
from pathlib import Path

ROOT = Path("DAViewer")


def replace_once(relative: str, old: str, new: str) -> None:
    target = ROOT / relative
    source = target.read_text(encoding="utf-8")
    n = source.count(old)
    if n != 1:
        raise SystemExit(f"{relative}: expected exactly one anchor, got {n}: {old[:80]!r}")
    target.write_text(source.replace(old, new, 1), encoding="utf-8")


# Use a strictly higher code than both the previous Ultra and upstream builds.
replace_once("pubspec.yaml", "version: 0.7.0+239", "version: 0.7.1+701")
replace_once(
    "android/app/src/main/AndroidManifest.xml",
    'android:label="DA Viewer"',
    'android:label="DAViewer Ultra"',
)

# The app has a public OAuth client ID, app_links callbacks and the Android
# intent filter already. Provider sign-in in embedded WebViews is often blocked.
# Open authorization in the system browser on Android; retain the unified
# WebView implementation on desktop and an optional browser session in-app.
replace_once(
    "lib/core/runtime/app_runtime.dart",
    "import '../network/dynamic_proxy_dio.dart';\n",
    "import '../network/desktop_uri_launcher.dart';\n"
    "import '../network/dynamic_proxy_dio.dart';\n",
)
replace_once(
    "lib/core/runtime/app_runtime.dart",
    "      launcher: webViewOAuthBridge,\n",
    "      launcher: Platform.isAndroid\n"
    "          ? const DesktopUriLauncher()\n"
    "          : webViewOAuthBridge,\n",
)

# External-browser OAuth and the embedded WebView have isolated cookie stores.
# Do not leave users stuck on a spinner awaiting a second website sign-in after
# the OAuth account was verified. They can still open web login as needed for
# the site's separate personalized web session.
replace_once(
    "lib/features/web_login/web_login_screen.dart",
    "        // A failed home navigation must keep the browser error/retry visible,\n",
    "        if (Platform.isAndroid) {\n"
    "          _closeAfterReport = false;\n"
    "          _requestClose(reason: 'oauth_completed_external');\n"
    "          return;\n"
    "        }\n"
    "        // A failed home navigation must keep the browser error/retry visible,\n",
)

# A valid, empty intermediary response carrying a next cursor is not EOF.
# Both web search and official fallback must preserve its continuation.
search = ROOT / "lib/features/search/search_providers.dart"
source = search.read_text(encoding="utf-8")
old = "return page.items.isEmpty\n"
if source.count(old) != 2:
    raise SystemExit(f"search continuation anchors changed: {source.count(old)}")
search.write_text(
    source.replace(old, "return page.items.isEmpty && !page.hasMore\n"),
    encoding="utf-8",
)

# Expose a manual continuation when an API page contains no items or a masonry
# viewport doesn't reliably generate a second scroll event near its bottom.
replace_once(
    "lib/shared/widgets/artwork_feed_grid.dart",
    """    } else if (widget.feed.items.isEmpty) {
      body = AppEmptyState(
        message: widget.emptyMessage,
        actionLabel: widget.emptyActionLabel,
        onAction: widget.emptyOnAction,
      );""",
    """    } else if (widget.feed.items.isEmpty) {
      body = AppEmptyState(
        message: widget.emptyMessage,
        actionLabel: widget.feed.hasMore && widget.onLoadMore != null
            ? s.loadMoreFiltered
            : widget.emptyActionLabel,
        onAction: widget.feed.hasMore && widget.onLoadMore != null
            ? (widget.onRetryLoadMore ?? widget.onLoadMore)
            : widget.emptyOnAction,
      );""",
)
replace_once(
    "lib/shared/widgets/artwork_feed_grid.dart",
    """                return const SizedBox(height: 16);
              },
            ),
          ),""",
    """                if (widget.feed.hasMore &&
                    widget.onLoadMore != null &&
                    !widget.feed.isLoading) {
                  return Center(
                    child: Padding(
                      padding: const EdgeInsets.only(bottom: 16),
                      child: TextButton(
                        onPressed: widget.onRetryLoadMore ?? widget.onLoadMore,
                        child: Text(s.loadMoreFiltered),
                      ),
                    ),
                  );
                }
                return const SizedBox(height: 16);
              },
            ),
          ),""",
)

# Daily Deviations has an optional date argument, NOT an offset API. Build
# cursor pagination by walking calendar days backward. Never invent "offset"
# for this endpoint; an empty day is not end-of-history.
file = ROOT / "lib/features/home/home_feeds.dart"
source = file.read_text(encoding="utf-8")
source = source.replace("import '../../shared/widgets/app_refresh_indicator.dart';\n", "", 1)
source = source.replace(
    "import 'home_providers.dart';\n",
    "import 'daily_archive_provider.dart';\n"
    "import '../../shared/widgets/artwork_feed_grid.dart';\n",
    1,
)
start = source.find("    final daily = ref.watch(dailyDeviationsProvider);\n")
if start == -1:
    raise SystemExit("DailyFeed initial provider anchor changed")
end = source.find("\n  }\n}\n", start)
if end == -1:
    raise SystemExit("DailyFeed build end anchor changed")
replacement = """    final feed = ref.watch(dailyArchiveProvider);
    return ArtworkFeedGrid(
      feed: feed,
      emptyMessage: s.noDaily,
      onRefresh: () => ref.read(dailyArchiveProvider.notifier).forceRefresh(),
      onLoadMore: () => ref.read(dailyArchiveProvider.notifier).loadMore(),
      onRetryLoadMore: () =>
          ref.read(dailyArchiveProvider.notifier).retryLoadMore(),
      emptyActionLabel: feed.hasMore ? s.loadMoreFiltered : null,
      emptyOnAction: feed.hasMore
          ? () => ref.read(dailyArchiveProvider.notifier).loadMore()
          : null,
    );"""
source = source[:start] + replacement + source[end:]
file.write_text(source, encoding="utf-8")

# Locally bundled source/test are installed by the workflow before this patch.
for relative in (
    "lib/features/home/daily_archive_provider.dart",
    "test/daily_archive_cursor_test.dart",
):
    if not (ROOT / relative).is_file():
        raise SystemExit(f"missing bundled file: {relative}")

print("DAViewer Ultra v0.7.1 patches applied")
