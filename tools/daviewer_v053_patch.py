from pathlib import Path

root = Path.cwd()

def replace_once(path, old, new):
    p = root / path
    text = p.read_text(encoding='utf-8')
    count = text.count(old)
    if count != 1:
        raise SystemExit(f'{path}: expected 1 match, got {count}')
    p.write_text(text.replace(old, new, 1), encoding='utf-8')

# 1) Always launch OAuth in the system browser. This makes Google login
# compatible with Google's embedded-browser restrictions.
replace_once(
    'lib/core/auth/webview_oauth_bridge.dart',
    """  @override
  Future<void> launch(Uri uri) async {
    // The WebLoginScreen subscribes in initState right after its route is
    // pushed, so give it a short grace period before falling back to the
    // system browser. Falling back too eagerly would leave the embedded web
    // session out of sync with the OAuth account.
    for (var i = 0; i < 25 && !_launchController.hasListener; i++) {
      await Future<void>.delayed(const Duration(milliseconds: 20));
    }
    if (_launchController.hasListener) {
      debugPrint('[oauth] routing authorize to embedded WebView');
      _launchController.add(uri);
    } else {
      debugPrint('[oauth] NO WebView listener -> system browser fallback');
      await _fallback.launch(uri);
    }
  }
""",
    """  @override
  Future<void> launch(Uri uri) async {
    // OAuth providers such as Google reject embedded WebViews. Keep the
    // complete authorization transaction in the system browser / Custom Tab.
    // AppLinksCallbackUriSource already receives dakit://oauth/callback when
    // the browser returns to the app.
    debugPrint('[oauth] routing authorize to system browser');
    await _fallback.launch(uri);
  }
""",
)

# 2) A system-browser OAuth login cannot populate Android WebView cookies.
# Close the login screen as soon as OAuth succeeds instead of waiting forever
# for a separate WebView session.
replace_once(
    'lib/features/web_login/web_login_screen.dart',
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
    """    // OAuth runs in the system browser. Browser cookies are intentionally
    // separate from Android WebView cookies, so OAuth success itself is the
    // close gate. Website-only features use the OAuth fallback below when a
    // WebView session is unavailable.
    ref.listen<AuthState>(authControllerProvider, (previous, next) {
      if (previous?.status != AuthStatus.signedIn &&
          next.status == AuthStatus.signedIn &&
          mounted) {
        _closeAfterReport = false;
        WidgetsBinding.instance.addPostFrameCallback((_) => _closeScreen());
      }
    });
""",
)

# 3) Keep Home usable after Google login even though system-browser cookies
# cannot be copied into WebView. Prefer DeviantArt's official OAuth browse/home
# endpoint, and fall back to Daily Deviations if that endpoint is unavailable.
home_path = root / 'lib/features/home/home_providers.dart'
home = home_path.read_text(encoding='utf-8')

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
              'web session unavailable; using official OAuth home fallback',
            );
            final page = await _fetchOfficialHomeFallback(runtime, request);
            ref.read(artworkStoreProvider.notifier).putAll(page.items);
            return page;
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

if home.count(old_gate) != 1:
    raise SystemExit('home gate mismatch')
home = home.replace(old_gate, new_gate, 1)

old_failure = """        if (page == null) {
          throw const DAKitException(
            kind: DAKitFailureKind.authentication,
            code: 'web.session.unavailable',
            message: 'The personalized feed requires a signed-in web session.',
          );
        }
"""

new_failure = """        if (page == null) {
          if (ref.read(authControllerProvider).oauthSignedIn) {
            AppLogger.instance.info(
              'home',
              'rfy unavailable; using official OAuth home fallback',
            );
            final fallback = await _fetchOfficialHomeFallback(runtime, request);
            ref.read(artworkStoreProvider.notifier).putAll(fallback.items);
            return fallback;
          }
          throw const DAKitException(
            kind: DAKitFailureKind.authentication,
            code: 'web.session.unavailable',
            message: 'The personalized feed requires a signed-in web session.',
          );
        }
"""

if home.count(old_failure) != 1:
    raise SystemExit('home fallback mismatch')
home = home.replace(old_failure, new_failure, 1)

marker = """/// Fetches one rfy page, or `null` when the web session is missing or the
/// request failed (the caller then refreshes the session and retries).
Future<Page<Artwork>?> _tryFetchRfy(
"""

helper = """/// OAuth-only Home fallback for browser-provider sign-in.
Future<Page<Artwork>> _fetchOfficialHomeFallback(
  AppRuntime runtime,
  PageRequest request,
) async {
  final logger = AppLogger.instance;
  final offset = int.tryParse(request.cursor ?? '') ?? 0;
  try {
    final json = await runtime.transport!.getJson(
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
          final item = raw.map<String, Object?>(
            (key, value) => MapEntry(key.toString(), value),
          );
          items.add(mapper.artwork(item));
        } on Object {
          // Skip malformed entries without failing the page.
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
    logger.warning(
      'home',
      'browse/home fallback failed; trying daily deviations',
      error,
      stack,
    );
    if (request.cursor != null) {
      return Page<Artwork>(
        items: const <Artwork>[],
        hasMore: false,
        nextCursor: null,
      );
    }
    final items = await OfficialDiscoveryRepository(
      runtime.transport!,
    ).dailyDeviations();
    return Page<Artwork>(
      items: items,
      hasMore: false,
      nextCursor: null,
    );
  }
}

"""

if home.count(marker) != 1:
    raise SystemExit('home helper marker mismatch')
home = home.replace(marker, helper + marker, 1)
home_path.write_text(home, encoding='utf-8')

print('DAViewer v0.5.3 patch applied')