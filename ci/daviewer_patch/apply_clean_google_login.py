from pathlib import Path

# --- web_login_screen.dart ---
p = Path("DAViewer/lib/features/web_login/web_login_screen.dart")
s = p.read_text()

old = """  InAppWebViewController? _controller;
  StreamSubscription<Uri>? _launchSub;
  Uri? _pendingAuthUri;
  bool _loading = true;
  bool _closeAfterReport = false;
  int _reportSeq = 0;
  double _progress = 0;
  late final AuthController _authController;

  WebViewOAuthBridge? get _bridge =>
      ref.read(runtimeProvider).webViewOAuthBridge;
"""
new = """  InAppWebViewController? _controller;
  StreamSubscription<Uri>? _launchSub;
  Uri? _pendingAuthUri;
  bool _loading = true;
  bool _closeAfterReport = false;
  bool _loginChoiceMade = false;
  bool _externalLogin = false;
  int _reportSeq = 0;
  double _progress = 0;
  late final AuthController _authController;
  late final WebViewOAuthBridge? _oauthBridge;
"""
if old not in s:
    raise SystemExit("field anchor changed")
s = s.replace(old, new, 1)

old = """  @override
  void initState() {
    super.initState();
    _authController = ref.read(authControllerProvider.notifier);
    final bridge = _bridge;
    if (bridge != null) {
      _launchSub = bridge.launchRequests.listen(_loadAuthRequest);
    }
    // Single unified login: this screen hosts the WebView that establishes BOTH
    // the DeviantArt web session and the OAuth authorization. Any "login"
    // button just opens this screen; once the WebView is subscribed here, we
    // auto-start the OAuth authorize so it completes in this same WebView.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      final auth = ref.read(authControllerProvider);
      if (!auth.oauthSignedIn && !auth.isLoggingIn && mounted) {
        _authController.login();
      }
    });
  }

  @override
  void dispose() {
    unawaited(_launchSub?.cancel());
    super.dispose();
  }
"""
new = """  @override
  void initState() {
    super.initState();
    _authController = ref.read(authControllerProvider.notifier);
    _oauthBridge = ref.read(runtimeProvider).webViewOAuthBridge;
    final bridge = _oauthBridge;
    if (bridge != null) {
      _launchSub = bridge.launchRequests.listen(_loadAuthRequest);
    }
  }

  @override
  void dispose() {
    unawaited(_launchSub?.cancel());
    if (_externalLogin) {
      _oauthBridge?.finishExternalAuthorization();
    }
    super.dispose();
  }

  Future<void> _startEmbeddedLogin() async {
    if (_loginChoiceMade) return;
    setState(() {
      _loginChoiceMade = true;
      _externalLogin = false;
    });
    await _authController.login();
    if (!mounted) return;
    if (!ref.read(authControllerProvider).oauthSignedIn) {
      setState(() => _loginChoiceMade = false);
    }
  }

  Future<void> _startExternalLogin() async {
    if (_loginChoiceMade) return;
    final bridge = _oauthBridge;
    if (bridge == null) return;
    bridge.launchNextExternally();
    setState(() {
      _loginChoiceMade = true;
      _externalLogin = true;
    });
    await _authController.login();
    if (!mounted) return;
    if (!ref.read(authControllerProvider).oauthSignedIn) {
      bridge.finishExternalAuthorization();
      setState(() {
        _loginChoiceMade = false;
        _externalLogin = false;
      });
    }
  }

  Future<void> _cancelExternalLogin() async {
    _oauthBridge?.finishExternalAuthorization();
    await _authController.cancelLogin();
    if (!mounted) return;
    setState(() {
      _loginChoiceMade = false;
      _externalLogin = false;
    });
  }

  Future<void> _reopenExternalLogin() async {
    await _oauthBridge?.reopenExternalAuthorization();
  }
"""
if old not in s:
    raise SystemExit("init/dispose anchor changed")
s = s.replace(old, new, 1)

s = s.replace("_bridge?.addCallback(uri);", "_oauthBridge?.addCallback(uri);")

old = """    // When OAuth finishes, close only after the deviantart home page reports
    // the real web session (onLoadStop + _reportWebSession). Closing on a fixed
    // timer could pop before the home page loads and leave the web session
    // recorded as signed-out.
    ref.listen<AuthState>(authControllerProvider, (previous, next) {
      if (previous?.status != AuthStatus.signedIn &&
          next.status == AuthStatus.signedIn &&
          mounted &&
          !_closeAfterReport) {
        _closeAfterReport = true;
        // Fallback: if the home page never reports (navigation stalls), close
        // after a generous timeout so the user isn't stuck.
        Future<void>.delayed(const Duration(seconds: 8), () {
          if (mounted && _closeAfterReport) {
            _closeAfterReport = false;
            _closeScreen();
          }
        });
      }
    });
"""
new = """    // Embedded DeviantArt login waits for the WebView cookie/CSRF report.
    // Google/social login runs entirely in the system browser, so a successful
    // OAuth callback is enough to close this screen.
    ref.listen<AuthState>(authControllerProvider, (previous, next) {
      if (previous?.status != AuthStatus.signedIn &&
          next.status == AuthStatus.signedIn &&
          mounted) {
        if (_externalLogin) {
          _oauthBridge?.finishExternalAuthorization();
          WidgetsBinding.instance.addPostFrameCallback((_) => _closeScreen());
          return;
        }
        if (_closeAfterReport) return;
        _closeAfterReport = true;
        Future<void>.delayed(const Duration(seconds: 8), () {
          if (mounted && _closeAfterReport) {
            _closeAfterReport = false;
            _closeScreen();
          }
        });
      }
    });
"""
if old not in s:
    raise SystemExit("listener anchor changed")
s = s.replace(old, new, 1)

s = s.replace(
"""        bottom: _loading
            ? PreferredSize(""",
"""        bottom: _loading && _loginChoiceMade && !_externalLogin
            ? PreferredSize(""",
1,
)

old = """          if (_closeAfterReport) _LoginSuccessOverlay(s: s),
"""
new = """          if (!auth.oauthSignedIn && !_loginChoiceMade)
            _LoginChoiceOverlay(
              s: s,
              onGoogle: _startExternalLogin,
              onDeviantArt: _startEmbeddedLogin,
            ),
          if (!auth.oauthSignedIn && _externalLogin)
            _ExternalLoginWaitingOverlay(
              s: s,
              onReopen: _reopenExternalLogin,
              onCancel: _cancelExternalLogin,
            ),
          if (_closeAfterReport) _LoginSuccessOverlay(s: s),
"""
if old not in s:
    raise SystemExit("stack overlay anchor changed")
s = s.replace(old, new, 1)

marker = """/// A slim hint shown while sign-in is in progress. It sets expectations for
"""
widgets = r"""
final class _LoginChoiceOverlay extends StatelessWidget {
  const _LoginChoiceOverlay({
    required this.s,
    required this.onGoogle,
    required this.onDeviantArt,
  });

  final AppStrings s;
  final VoidCallback onGoogle;
  final VoidCallback onDeviantArt;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return ColoredBox(
      color: theme.scaffoldBackgroundColor,
      child: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 440),
          child: Padding(
            padding: const EdgeInsets.all(24),
            child: Card(
              child: Padding(
                padding: const EdgeInsets.all(24),
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  children: <Widget>[
                    const Icon(Icons.account_circle_outlined, size: 56),
                    const SizedBox(height: 16),
                    Text(
                      s.signInWelcomeTitle,
                      style: theme.textTheme.titleLarge,
                      textAlign: TextAlign.center,
                    ),
                    const SizedBox(height: 8),
                    Text(
                      s.loginRouteChoiceHint,
                      style: theme.textTheme.bodyMedium,
                      textAlign: TextAlign.center,
                    ),
                    const SizedBox(height: 24),
                    SizedBox(
                      width: double.infinity,
                      child: FilledButton.icon(
                        onPressed: onGoogle,
                        icon: const Icon(Icons.open_in_browser),
                        label: Text(s.signInWithGoogleBrowser),
                      ),
                    ),
                    const SizedBox(height: 12),
                    SizedBox(
                      width: double.infinity,
                      child: OutlinedButton.icon(
                        onPressed: onDeviantArt,
                        icon: const Icon(Icons.login),
                        label: Text(s.signInWithDeviantArtAccount),
                      ),
                    ),
                  ],
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

final class _ExternalLoginWaitingOverlay extends StatelessWidget {
  const _ExternalLoginWaitingOverlay({
    required this.s,
    required this.onReopen,
    required this.onCancel,
  });

  final AppStrings s;
  final VoidCallback onReopen;
  final VoidCallback onCancel;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return ColoredBox(
      color: theme.scaffoldBackgroundColor,
      child: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 440),
          child: Padding(
            padding: const EdgeInsets.all(24),
            child: Card(
              child: Padding(
                padding: const EdgeInsets.all(24),
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  children: <Widget>[
                    const CircularProgressIndicator(),
                    const SizedBox(height: 20),
                    Text(
                      s.googleBrowserWaiting,
                      textAlign: TextAlign.center,
                      style: theme.textTheme.bodyMedium,
                    ),
                    const SizedBox(height: 16),
                    Wrap(
                      alignment: WrapAlignment.center,
                      spacing: 8,
                      children: <Widget>[
                        TextButton.icon(
                          onPressed: onReopen,
                          icon: const Icon(Icons.open_in_browser),
                          label: Text(s.reopenBrowser),
                        ),
                        TextButton.icon(
                          onPressed: onCancel,
                          icon: const Icon(Icons.close),
                          label: Text(s.cancel),
                        ),
                      ],
                    ),
                  ],
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

"""
if marker not in s:
    raise SystemExit("widget insertion anchor changed")
s = s.replace(marker, widgets + marker, 1)
p.write_text(s)

# --- l10n strings ---
p = Path("DAViewer/lib/core/l10n/app_strings.dart")
s = p.read_text()
anchor = """  String get signInOrRegister =>
      _lang == AppLanguage.zh ? '登录或注册' : 'Sign in or create an account';
"""
addition = anchor + """  String get signInWithGoogleBrowser => _lang == AppLanguage.zh
      ? 'Google / 社交账号（系统浏览器）'
      : 'Google / social account (system browser)';
  String get signInWithDeviantArtAccount => _lang == AppLanguage.zh
      ? 'DeviantArt 账号（App 内）'
      : 'DeviantArt account (in app)';
  String get loginRouteChoiceHint => _lang == AppLanguage.zh
      ? 'Google 登录必须使用系统浏览器；DeviantArt 用户名或邮箱登录仍在 App 内完成。'
      : 'Google sign-in uses your system browser. DeviantArt username/email sign-in stays inside the app.';
  String get googleBrowserWaiting => _lang == AppLanguage.zh
      ? '已在系统浏览器打开 DeviantArt 官方授权页。选择 Google 并完成登录后会自动返回 DAViewer。'
      : 'The official DeviantArt authorization page is open in your system browser. Choose Google and finish sign-in; DAViewer will resume automatically.';
  String get reopenBrowser =>
      _lang == AppLanguage.zh ? '重新打开浏览器' : 'Reopen browser';
"""
if anchor not in s:
    raise SystemExit("l10n anchor changed")
s = s.replace(anchor, addition, 1)
p.write_text(s)

# --- Home: prefer original web personalized feed, fallback only when a valid
# OAuth session exists without WebView cookies (e.g. Google system-browser login).
p = Path("DAViewer/lib/features/home/home_screen.dart")
s = p.read_text()
old = "import 'home_providers.dart';\nimport 'update_banner.dart';"
new = "import 'home_providers.dart';\nimport 'oauth_home_fallback.dart';\nimport 'update_banner.dart';"
if old not in s:
    raise SystemExit("home import anchor changed")
s = s.replace(old, new, 1)

old = """    if (shouldRefreshPersonalizedFeedOnResume(
      backgroundDuration: DateTime.now().difference(backgroundedAt),
      routeIsCurrent: routeIsCurrent,
      recommendedTabIsActive: recommendedTabIsActive,
      scrollOffset: scrollOffset,
    )) {
      unawaited(ref.read(personalizedFeedProvider.notifier).refreshSilently());
    }"""
new = """    if (shouldRefreshPersonalizedFeedOnResume(
      backgroundDuration: DateTime.now().difference(backgroundedAt),
      routeIsCurrent: routeIsCurrent,
      recommendedTabIsActive: recommendedTabIsActive,
      scrollOffset: scrollOffset,
    )) {
      final webSignedIn =
          ref.read(webSessionControllerProvider).isLoggedIn == true;
      if (webSignedIn) {
        unawaited(
          ref.read(personalizedFeedProvider.notifier).refreshSilently(),
        );
      } else if (ref.read(authControllerProvider).oauthSignedIn) {
        unawaited(
          ref.read(oauthHomeFeedProvider.notifier).refreshSilently(),
        );
      }
    }"""
if old not in s:
    raise SystemExit("home resume anchor changed")
s = s.replace(old, new, 1)

old = """    final webSignedIn = ref.watch(
      webSessionControllerProvider.select((web) => web.isLoggedIn == true),
    );
    if (!webSignedIn) {
      return LoginPrompt(
        s: s,
        onLogin: () => context.push('/web-login'),
        message: s.recommendedSignInHint,
      );
    }
    final feed = ref.watch(personalizedFeedProvider);

    return ArtworkFeedGrid(
      scrollController: _scrollController,
      feed: feed,
      emptyMessage: s.noRecommendations,
      errorMessage: s.recommendedFeedLoadFailure,
      onRefresh: () => ref.read(personalizedFeedProvider.notifier).refresh(),
      onLoadMore: () => ref.read(personalizedFeedProvider.notifier).loadMore(),
    );"""
new = """    final webSignedIn = ref.watch(
      webSessionControllerProvider.select((web) => web.isLoggedIn == true),
    );
    final oauthSignedIn = ref.watch(
      authControllerProvider.select((auth) => auth.oauthSignedIn),
    );
    if (!webSignedIn && !oauthSignedIn) {
      return LoginPrompt(
        s: s,
        onLogin: () => context.push('/web-login'),
        message: s.recommendedSignInHint,
      );
    }

    final feedProvider =
        webSignedIn ? personalizedFeedProvider : oauthHomeFeedProvider;
    final feed = ref.watch(feedProvider);

    return ArtworkFeedGrid(
      scrollController: _scrollController,
      feed: feed,
      emptyMessage: s.noRecommendations,
      errorMessage: s.recommendedFeedLoadFailure,
      onRefresh: () => ref.read(feedProvider.notifier).refresh(),
      onLoadMore: () => ref.read(feedProvider.notifier).loadMore(),
    );"""
if old not in s:
    raise SystemExit("home build anchor changed")
s = s.replace(old, new, 1)
p.write_text(s)
