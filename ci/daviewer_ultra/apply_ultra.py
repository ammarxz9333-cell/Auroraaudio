from pathlib import Path

root = Path("DAViewer")


def replace_once(path: str, old: str, new: str) -> None:
    p = root / path
    text = p.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected exactly 1 match, got {count}")
    p.write_text(text.replace(old, new, 1), encoding="utf-8")


# Personal build identity.
replace_once(
    "pubspec.yaml",
    "version: 0.5.7+219",
    "version: 0.6.0+600",
)
replace_once(
    "android/app/src/main/AndroidManifest.xml",
    'android:label="DA Viewer"',
    'android:label="DA Viewer Ultra"',
)

# Android OAuth: use the real system browser. Google/social providers commonly
# reject embedded WebViews. A native callback source buffers warm/cold-start
# dakit://oauth/callback intents until Dart is actually listening.
replace_once(
    "lib/core/runtime/app_runtime.dart",
    "import '../auth/webview_oauth_bridge.dart';\n",
    "import '../auth/android_native_oauth_callback_source.dart';\n"
    "import '../auth/webview_oauth_bridge.dart';\n",
)
replace_once(
    "lib/core/runtime/app_runtime.dart",
    "import '../diagnostics/app_logger.dart';\n",
    "import '../diagnostics/app_logger.dart';\n"
    "import '../network/desktop_uri_launcher.dart';\n",
)
replace_once(
    "lib/core/runtime/app_runtime.dart",
    """    this.webViewOAuthBridge,
    this.webViewProxyManager,
  });""",
    """    this.webViewOAuthBridge,
    this.webViewProxyManager,
    this.androidNativeOAuthCallbackSource,
  });""",
)
replace_once(
    "lib/core/runtime/app_runtime.dart",
    """  final WebViewOAuthBridge? webViewOAuthBridge;
  final WebViewProxyManager? webViewProxyManager;

  void Function()? _proxyListener;""",
    """  final WebViewOAuthBridge? webViewOAuthBridge;
  final WebViewProxyManager? webViewProxyManager;
  final AndroidNativeOAuthCallbackSource? androidNativeOAuthCallbackSource;

  void Function()? _proxyListener;""",
)
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
    final androidNativeOAuthCallbackSource = useSystemBrowserOAuth
        ? AndroidNativeOAuthCallbackSource()
        : null;
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
          ? const DesktopUriLauncher()
          : webViewOAuthBridge!,
      callbacks: useSystemBrowserOAuth
          ? androidNativeOAuthCallbackSource!
          : MergedCallbackUriSource(
              initial: AppLinksCallbackUriSource(),
              others: <CallbackUriSource>[webViewOAuthBridge!.callbacks],
            ),
""",
)
replace_once(
    "lib/core/runtime/app_runtime.dart",
    """      webViewOAuthBridge: webViewOAuthBridge,
      webViewProxyManager: webViewProxyManager,
    );""",
    """      webViewOAuthBridge: webViewOAuthBridge,
      webViewProxyManager: webViewProxyManager,
      androidNativeOAuthCallbackSource: androidNativeOAuthCallbackSource,
    );""",
)
replace_once(
    "lib/core/runtime/app_runtime.dart",
    """    unawaited(webViewProxyManager?.dispose() ?? Future<void>.value());
    unawaited(webViewOAuthBridge?.dispose() ?? Future<void>.value());
  }""",
    """    unawaited(webViewProxyManager?.dispose() ?? Future<void>.value());
    unawaited(webViewOAuthBridge?.dispose() ?? Future<void>.value());
    unawaited(
      androidNativeOAuthCallbackSource?.dispose() ?? Future<void>.value(),
    );
  }""",
)

# Disable Flutter's built-in deep-link route handling on Android. The native
# callback bridge owns dakit://oauth/callback and must not race go_router.
replace_once(
    "android/app/src/main/AndroidManifest.xml",
    """            <meta-data
              android:name="io.flutter.embedding.android.NormalTheme"
              android:resource="@style/NormalTheme"
              />""",
    """            <meta-data
              android:name="io.flutter.embedding.android.NormalTheme"
              android:resource="@style/NormalTheme"
              />
            <meta-data
              android:name="flutter_deeplinking_enabled"
              android:value="false" />""",
)

# On Android, browser OAuth cannot populate the embedded WebView cookie jar.
# OAuth success is therefore sufficient to leave the login screen. Re-opening
# the screen later can still establish an optional web session for RFY/private
# website adapters.
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
    """    // Android OAuth completes in the system browser. Browser cookies are
    // isolated from this WebView, so OAuth success itself is the close gate.
    // Desktop keeps upstream's unified WebView contract.
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

# Resilient Home: prefer the real website RFY feed whenever a web session is
# healthy; otherwise keep a valid OAuth user inside the app using the official
# browse/home endpoint, with Daily Deviations as a last-resort first-page
# fallback. mature_content=true is explicit on every official fallback page.
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
helper = """/// Official OAuth fallback for Android browser sign-in or RFY outages.
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
          // One malformed item must not blank the whole feed.
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

# Release-mode optimizations with a CI-only installable signature.
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
