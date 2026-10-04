from pathlib import Path

root = Path("DAViewer")

def replace_once(path: str, old: str, new: str) -> None:
    p = root / path
    text = p.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected exactly 1 match, got {count}")
    p.write_text(text.replace(old, new, 1), encoding="utf-8")

# Brand this personal fork separately at the version level.
replace_once(
    "pubspec.yaml",
    "version: 0.5.7+219",
    "version: 0.6.0+600",
)

# Android: OAuth providers (especially Google) reject embedded WebViews.
# Use the system browser + app-link callback on Android while preserving the
# upstream single-WebView flow on desktop.
replace_once(
    "lib/core/runtime/app_runtime.dart",
    """    final webViewOAuthBridge = WebViewOAuthBridge();
    final webViewProxyManager = proxyController == null
        ? null
        : WebViewProxyManager(proxyController);
    final oauth = DAKitOAuthClient(
""",
    """    final useSystemBrowserOAuth = Platform.isAndroid;
    final webViewOAuthBridge =
        useSystemBrowserOAuth ? null : WebViewOAuthBridge();
    final webViewProxyManager = proxyController == null
        ? null
        : WebViewProxyManager(proxyController);
    final oauth = DAKitOAuthClient(
""",
)

replace_once(
    "lib/core/runtime/app_runtime.dart",
    """      launcher: webViewOAuthBridge,
      callbacks: MergedCallbackUriSource(
        initial: AppLinksCallbackUriSource(),
        others: <CallbackUriSource>[webViewOAuthBridge.callbacks],
      ),
""",
    """      launcher: useSystemBrowserOAuth
          ? const SystemUriLauncher()
          : webViewOAuthBridge!,
      callbacks: useSystemBrowserOAuth
          ? AppLinksCallbackUriSource()
          : MergedCallbackUriSource(
              initial: AppLinksCallbackUriSource(),
              others: <CallbackUriSource>[webViewOAuthBridge!.callbacks],
            ),
""",
)

# System-browser OAuth cannot populate Android WebView cookies. Do not trap the
# user on the login screen waiting for a web-session confirmation that belongs
# to a different cookie jar. Web-only personalization remains optional.
replace_once(
    "lib/features/web_login/web_login_screen.dart",
    """    // When OAuth finishes, close only after the deviantart home page reports
    // the real web session (onLoadStop + _reportWebSession). Closing on a fixed
    // timer could pop before the home page loads and leave the web session
    // recorded as signed-out.
    ref.listen<AuthState>(authControllerProvider, (previous, next) {
      if (previous?.status != AuthStatus.signedIn &&
          next.status == AuthStatus.signedIn &&
          mounted &&
          !_closeAfterReport) {
        _closeAfterReport = true;
        if (_serverConfirmedWebSession) {
          _closeAfterReport = false;
          WidgetsBinding.instance.addPostFrameCallback((_) => _closeScreen());
        }
      }
    });
""",
    """    // Android OAuth completes in the system browser. Its cookies are
    // intentionally isolated from this WebView, so OAuth success is enough to
    // leave this screen. Desktop keeps upstream's unified WebView close gate.
    ref.listen<AuthState>(authControllerProvider, (previous, next) {
      if (previous?.status != AuthStatus.signedIn &&
          next.status == AuthStatus.signedIn &&
          mounted &&
          !_closeAfterReport) {
        if (Platform.isAndroid) {
          _closeAfterReport = false;
          WidgetsBinding.instance.addPostFrameCallback((_) => _closeScreen());
          return;
        }
        _closeAfterReport = true;
        if (_serverConfirmedWebSession) {
          _closeAfterReport = false;
          WidgetsBinding.instance.addPostFrameCallback((_) => _closeScreen());
        }
      }
    });
""",
)

home = root / "lib/features/home/home_providers.dart"
text = home.read_text(encoding="utf-8")
if "import '../../core/runtime/app_runtime.dart';" not in text:
    anchor = "import '../../core/feed/artwork_feed_controller.dart';\n"
    if anchor not in text:
        raise SystemExit("home import anchor changed")
    text = text.replace(
        anchor,
        anchor + "import '../../core/runtime/app_runtime.dart';\n",
        1,
    )

old_gate = """        final webSessionState = ref.read(webSessionControllerProvider);
        if (webSessionState.isLoggedIn != true ||
            webSessionState.username.trim().isEmpty) {
          AppLogger.instance.warning(
            'home',
            'web session not confirmed; showing web-session notice',
          );
          throw const DAKitException(
            kind: DAKitFailureKind.authentication,
            code: 'web.session.unavailable',
            message: 'The personalized feed requires a signed-in web session.',
          );
        }
"""
new_gate = """        final webSessionState = ref.read(webSessionControllerProvider);
        if (webSessionState.isLoggedIn != true ||
            webSessionState.username.trim().isEmpty) {
          if (ref.read(authControllerProvider).oauthSignedIn) {
            AppLogger.instance.info(
              'home',
              'web session unavailable; using official Discover fallback',
            );
            final fallback = await _fetchOfficialHomeFallback(runtime, request);
            ref.read(artworkStoreProvider.notifier).putAll(fallback.items);
            return fallback;
          }
          AppLogger.instance.warning(
            'home',
            'web session not confirmed; showing web-session notice',
          );
          throw const DAKitException(
            kind: DAKitFailureKind.authentication,
            code: 'web.session.unavailable',
            message: 'The personalized feed requires a signed-in web session.',
          );
        }
"""
if text.count(old_gate) != 1:
    raise SystemExit(f"home web-session gate mismatch: {text.count(old_gate)}")
text = text.replace(old_gate, new_gate, 1)

old_verdict = """        if (verdict == WebSessionStatusState.anonymous) {
          AppLogger.instance.warning(
            'home',
            'rfy skipped: web session verdict anonymous '
                'claimed=${webSessionState.username}',
          );
          throw const DAKitException(
            kind: DAKitFailureKind.authentication,
            code: 'web.session.unavailable',
            message: 'The personalized feed requires a signed-in web session.',
          );
        }
"""
new_verdict = """        if (verdict == WebSessionStatusState.anonymous) {
          if (ref.read(authControllerProvider).oauthSignedIn) {
            AppLogger.instance.info(
              'home',
              'web session anonymous; using official Discover fallback',
            );
            final fallback = await _fetchOfficialHomeFallback(runtime, request);
            ref.read(artworkStoreProvider.notifier).putAll(fallback.items);
            return fallback;
          }
          AppLogger.instance.warning(
            'home',
            'rfy skipped: web session verdict anonymous '
                'claimed=${webSessionState.username}',
          );
          throw const DAKitException(
            kind: DAKitFailureKind.authentication,
            code: 'web.session.unavailable',
            message: 'The personalized feed requires a signed-in web session.',
          );
        }
"""
if text.count(old_verdict) != 1:
    raise SystemExit(f"home verdict gate mismatch: {text.count(old_verdict)}")
text = text.replace(old_verdict, new_verdict, 1)

old_failure = """        if (page == null) {
          // The retry already refreshed the request context. A still-failing
          // rfy request is a feed failure (WAF/proxy/CSRF), not a login
          // verdict: only the authoritative session chain may produce the
          // login prompt, so this must not carry web.session.unavailable.
          throw const DAKitException(
            kind: DAKitFailureKind.network,
            code: 'rfy.feed.unavailable',
            message: 'Recommendations are temporarily unavailable.',
          );
        }
"""
new_failure = """        if (page == null) {
          if (ref.read(authControllerProvider).oauthSignedIn) {
            AppLogger.instance.info(
              'home',
              'rfy unavailable after refresh; using official Discover fallback',
            );
            final fallback = await _fetchOfficialHomeFallback(runtime, request);
            ref.read(artworkStoreProvider.notifier).putAll(fallback.items);
            return fallback;
          }
          // The retry already refreshed the request context. A still-failing
          // rfy request is a feed failure (WAF/proxy/CSRF), not a login
          // verdict: only the authoritative session chain may produce the
          // login prompt, so this must not carry web.session.unavailable.
          throw const DAKitException(
            kind: DAKitFailureKind.network,
            code: 'rfy.feed.unavailable',
            message: 'Recommendations are temporarily unavailable.',
          );
        }
"""
if text.count(old_failure) != 1:
    raise SystemExit(f"home final failure mismatch: {text.count(old_failure)}")
text = text.replace(old_failure, new_failure, 1)

marker = """/// Fetches one rfy page, or `null` when the web session is missing or the
/// request failed (the caller then refreshes the session and retries).
Future<Page<Artwork>?> _tryFetchRfy(
"""
helper = """/// OAuth-only fallback used when Android system-browser OAuth has no matching
/// embedded WebView Cookie/CSRF session, or when the private rfy adapter is
/// temporarily blocked. DeviantArt's official browse/home endpoint is a
/// Discover feed, not the website's personalized rfy feed, so the fallback is
/// intentionally labelled as resilience rather than equivalent personalization.
Future<Page<Artwork>> _fetchOfficialHomeFallback(
  AppRuntime runtime,
  PageRequest request,
) async {
  final transport = runtime.transport;
  if (transport == null) {
    throw const DAKitException(
      kind: DAKitFailureKind.configuration,
      code: 'app.runtime.transport',
      message: 'The official API transport is not available.',
    );
  }
  final offset = int.tryParse(request.cursor ?? '') ?? 0;
  try {
    final json = await transport.getJson(
      'browse/home',
      query: <String, Object?>{
        'limit': request.limit,
        'offset': offset,
        'mature_content': true,
      },
    );
    final rawResults = json['results'];
    const mapper = DeviationMapper();
    final items = <Artwork>[];
    if (rawResults is List) {
      for (final raw in rawResults) {
        if (raw is! Map) continue;
        try {
          items.add(
            mapper.artwork(
              raw.map<String, Object?>(
                (key, value) => MapEntry(key.toString(), value),
              ),
            ),
          );
        } on Object {
          // One malformed deviation must not blank the entire page.
        }
      }
    }
    final hasMore = json['has_more'] == true;
    final nextOffset = json['next_offset'];
    return Page<Artwork>(
      items: items,
      hasMore: hasMore,
      nextCursor: hasMore && nextOffset != null ? '$nextOffset' : null,
    );
  } on Object catch (error, stack) {
    AppLogger.instance.warning(
      'home',
      'official browse/home fallback failed; trying Daily Deviations',
      error,
      stack,
    );
    if (request.cursor != null) {
      return const Page<Artwork>(
        items: <Artwork>[],
        hasMore: false,
        nextCursor: null,
      );
    }
    final items = await OfficialDiscoveryRepository(
      transport,
    ).dailyDeviations();
    return Page<Artwork>(
      items: items,
      hasMore: false,
      nextCursor: null,
    );
  }
}

"""
if text.count(marker) != 1:
    raise SystemExit(f"home helper marker mismatch: {text.count(marker)}")
text = text.replace(marker, helper + marker, 1)
home.write_text(text, encoding="utf-8")

# CI-only signing fallback: release-mode optimizations with the runner's debug
# key. This keeps the artifact installable without storing a private signing
# secret in the repository.
replace_once(
    "android/app/build.gradle.kts",
    """            signingConfig = if (keystorePropertiesFile.exists()) {
                signingConfigs.getByName("release")
            } else {
                error(
                    "Release signing is not configured. " +
                    "Create android/key.properties and " +
                    "android/app/upload-keystore.p12 before building a release APK. " +
                    "See README.md / CI signing secrets."
                )
            }
""",
    """            signingConfig = if (keystorePropertiesFile.exists()) {
                signingConfigs.getByName("release")
            } else {
                signingConfigs.getByName("debug")
            }
""",
)

print("DAViewer Ultra patch applied successfully")
