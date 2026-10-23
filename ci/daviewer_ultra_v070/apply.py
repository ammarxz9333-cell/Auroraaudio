"""DAViewer Ultra v0.7.2 migration patches for upstream v0.7.0.

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
replace_once("pubspec.yaml", "version: 0.7.0+239", "version: 0.7.2+702")
replace_once(
    "android/app/src/main/AndroidManifest.xml",
    'android:label="DA Viewer"',
    'android:label="DAViewer Ultra"',
)

# Allow this CI-only personal artifact to build without upload-key secrets.
# This uses a debug key and cannot update a differently-signed installation.
replace_once(
    "android/app/build.gradle.kts",
    """                error(
                    "Release signing is not configured. " +
                    "Create android/key.properties and " +
                    "android/app/upload-keystore.p12 before building a release APK. " +
                    "See README.md / CI signing secrets."
                )""",
    '                signingConfigs.getByName("debug")',
)

# Preserve the externally launched Android authorization as the default.
# Allow the user to opt into the embedded flow if the browser route fails;
# both reuse the secure PKCE callback stream without leaking credentials.
replace_once(
    "lib/core/auth/webview_oauth_bridge.dart",
    """  WebViewOAuthBridge({ExternalUriLauncher? fallback})
    : _fallback = fallback ?? const DesktopUriLauncher();""",
    """  WebViewOAuthBridge({
    ExternalUriLauncher? fallback,
    this.preferExternalBrowser = false,
  }) : _fallback = fallback ?? const DesktopUriLauncher();

  bool preferExternalBrowser;""",
)
replace_once(
    "lib/core/auth/webview_oauth_bridge.dart",
    "  Future<void> launch(Uri uri) async {\n",
    """  Future<void> launch(Uri uri) async {
    if (preferExternalBrowser) {
      AppLogger.instance.info('oauthBridge', 'authorize target=system');
      await _fallback.launch(uri);
      return;
    }
""",
)
replace_once(
    "lib/core/runtime/app_runtime.dart",
    "import '../network/dynamic_proxy_dio.dart';\n",
    "import '../network/dynamic_proxy_dio.dart';\n"
    "import '../network/oauth_network_fallback.dart';\n",
)
replace_once(
    "lib/core/runtime/app_runtime.dart",
    "final webViewOAuthBridge = WebViewOAuthBridge();",
    "final webViewOAuthBridge = WebViewOAuthBridge(\n"
    "      preferExternalBrowser: Platform.isAndroid,\n"
    "    );",
)
replace_once(
    "lib/core/runtime/app_runtime.dart",
    "      endpoint: dio == null ? null : DioOAuthEndpoint(dio: dio),\n",
    """      endpoint: dio == null
          ? null
          : SafeOAuthNetworkFallback(
              primary: DioOAuthEndpoint(dio: dio),
              direct: DioOAuthEndpoint(
                dio: createDynamicProxyDio(
                  () => 'DIRECT',
                  options: BaseOptions(
                    connectTimeout: const Duration(seconds: 15),
                    receiveTimeout: const Duration(seconds: 30),
                    headers: <String, Object>{
                      'User-Agent': 'DAViewer-Ultra/0.7.2',
                      'Accept-Encoding': 'gzip',
                    },
                  ),
                ),
              ),
              source: () => proxyController?.source ?? ProxySource.direct,
            ),
""",
)
replace_once(
    "lib/core/runtime/app_runtime.dart",
    """        receiveTimeout: const Duration(seconds: 30),
      ),
    );
    dio.interceptors.add(PlatformAiInterceptor());""",
    """        receiveTimeout: const Duration(seconds: 30),
        headers: <String, Object>{
          'User-Agent': 'DAViewer-Ultra/0.7.2',
          'Accept-Encoding': 'gzip',
        },
      ),
    );
    dio.interceptors.add(PlatformAiInterceptor());""",
)

# Clear diagnosis and real recovery controls instead of a generic red banner
# sitting over a white WebView. The error code is safe; tokens and callbacks
# must never appear in the visible message.
replace_once(
    "lib/features/web_login/web_login_screen.dart",
    "import 'package:flutter/material.dart';\n",
    "import 'package:dakit_core/dakit_core.dart';\n"
    "import 'package:flutter/material.dart';\n",
)
replace_once(
    "lib/core/l10n/app_strings.dart",
    "  String get loginNetworkFailure => _lang == AppLanguage.zh\n",
    "  String get loginTryEmbedded => _lang == AppLanguage.zh\n"
    "      ? '改用应用内登录'\n"
    "      : 'Try in-app sign-in';\n"
    "  String get loginNetworkFailure => _lang == AppLanguage.zh\n",
)
replace_once(
    "lib/features/web_login/web_login_screen.dart",
    """              if (auth.error != null)
                _LoginErrorBanner(
                  message: friendlyLoginErrorMessage(auth.error!, s),
                ),""",
    """              if (auth.error != null) ...<Widget>[
                _LoginErrorBanner(
                  message: auth.error is DAKitException
                      ? '${friendlyLoginErrorMessage(auth.error!, s)} '
                        '(${(auth.error as DAKitException).code})'
                      : friendlyLoginErrorMessage(auth.error!, s),
                ),
                Padding(
                  padding: const EdgeInsets.all(12),
                  child: Wrap(
                    spacing: 8,
                    runSpacing: 8,
                    children: <Widget>[
                      FilledButton.tonalIcon(
                        onPressed: auth.isLoggingIn
                            ? null
                            : () => unawaited(_authController.retryLogin()),
                        icon: const Icon(Icons.refresh),
                        label: Text(s.retry),
                      ),
                      if (Platform.isAndroid)
                        OutlinedButton(
                          onPressed: auth.isLoggingIn
                              ? null
                              : () {
                                  ref
                                      .read(runtimeProvider)
                                      .webViewOAuthBridge
                                      ?.preferExternalBrowser = false;
                                  unawaited(_authController.retryLogin());
                                },
                          child: Text(s.loginTryEmbedded),
                        ),
                    ],
                  ),
                ),
              ],""",
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
    "lib/core/network/oauth_network_fallback.dart",
    "test/oauth_network_fallback_test.dart",
    "test/oauth_launcher_switch_test.dart",
):
    if not (ROOT / relative).is_file():
        raise SystemExit(f"missing bundled file: {relative}")

print("DAViewer Ultra v0.7.2 login recovery patches applied")
