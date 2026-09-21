from pathlib import Path

p = Path("DAViewer/lib/features/web_login/web_login_screen.dart")
s = p.read_text()

old = """    // Single unified login: this screen hosts the WebView that establishes BOTH
    // the DeviantArt web session and the OAuth authorization. Any "login"
    // button just opens this screen; once the WebView is subscribed here, we
    // auto-start the OAuth authorize so it completes in this same WebView.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      final auth = ref.read(authControllerProvider);
      if (!auth.oauthSignedIn && !auth.isLoggingIn && mounted) {
        _authController.login();
      }
    });
"""
if old not in s:
    raise SystemExit("init auto-login anchor changed")
s = s.replace(old, "", 1)

old_fields = """  double _progress = 0;
  late final AuthController _authController;
"""
new_fields = """  double _progress = 0;
  bool _externalGoogleLogin = false;
  bool _externalGoogleBusy = false;
  late final AuthController _authController;
"""
if old_fields not in s:
    raise SystemExit("field anchor changed")
s = s.replace(old_fields, new_fields, 1)

insert_before = """  /// Reads the web session (CSRF + login state + username) from the WebView
"""
google_method = """  Future<void> _startGoogleLogin() async {
    final bridge = _bridge;
    if (bridge == null || _externalGoogleBusy) return;
    final auth = ref.read(authControllerProvider);
    if (auth.oauthSignedIn) {
      _closeScreen();
      return;
    }

    setState(() {
      _externalGoogleLogin = true;
      _externalGoogleBusy = true;
    });
    bridge.launchNextExternally();
    try {
      await _authController.login();
    } finally {
      bridge.finishExternalAuthorization();
      if (mounted) {
        setState(() => _externalGoogleBusy = false);
      }
    }
  }

"""
if insert_before not in s:
    raise SystemExit("method insertion anchor changed")
s = s.replace(insert_before, google_method + insert_before, 1)

old_report = """      // A signed-in web session (the `userinfo` cookie) means login succeeded
      // and the cookie is captured — leave the login screen automatically.
      // This also covers the "OAuth already signed in, web session lost"
      // re-login case, where the OAuth state never transitions and the old
      // listener-based close never fired.
      if (isLoggedIn && mounted) {
        WidgetsBinding.instance.addPostFrameCallback((_) => _closeScreen());
      } else {
        _maybeClose();
      }
"""
new_report = """      // A DeviantArt web login establishes the Cookie/CSRF session first.
      // If OAuth is not signed in yet, continue the SAME normal app flow by
      // starting authorization now; the bridge routes it into this WebView,
      // where the freshly-created DeviantArt session can authorize without
      // asking for credentials again.
      final auth = ref.read(authControllerProvider);
      if (isLoggedIn && !auth.oauthSignedIn && !auth.isLoggingIn) {
        unawaited(_authController.login());
        return;
      }
      if (isLoggedIn && mounted && auth.oauthSignedIn) {
        WidgetsBinding.instance.addPostFrameCallback((_) => _closeScreen());
      } else {
        _maybeClose();
      }
"""
if old_report not in s:
    raise SystemExit("web report anchor changed")
s = s.replace(old_report, new_report, 1)

old_listener = """      if (previous?.status != AuthStatus.signedIn &&
          next.status == AuthStatus.signedIn &&
          mounted &&
          !_closeAfterReport) {
        _closeAfterReport = true;
"""
new_listener = """      if (previous?.status != AuthStatus.signedIn &&
          next.status == AuthStatus.signedIn &&
          mounted &&
          !_closeAfterReport) {
        if (_externalGoogleLogin) {
          _externalGoogleLogin = false;
          WidgetsBinding.instance.addPostFrameCallback((_) => _closeScreen());
          return;
        }
        _closeAfterReport = true;
"""
if old_listener not in s:
    raise SystemExit("listener anchor changed")
s = s.replace(old_listener, new_listener, 1)

old_column = """              if (!auth.oauthSignedIn) _VerificationHint(s: s),
              Expanded(
"""
new_column = """              if (!auth.oauthSignedIn) ...[
                _VerificationHint(s: s),
                Padding(
                  padding: const EdgeInsets.fromLTRB(12, 10, 12, 6),
                  child: SizedBox(
                    width: double.infinity,
                    child: FilledButton.icon(
                      onPressed: _externalGoogleBusy ? null : _startGoogleLogin,
                      icon: _externalGoogleBusy
                          ? const SizedBox.square(
                              dimension: 18,
                              child: CircularProgressIndicator(strokeWidth: 2),
                            )
                          : const Icon(Icons.account_circle_outlined),
                      label: const Text('Sign in with Google'),
                    ),
                  ),
                ),
                Padding(
                  padding: const EdgeInsets.fromLTRB(12, 0, 12, 8),
                  child: Text(
                    'Google opens in your system browser. '
                    'DeviantArt account login stays below in the app.',
                    textAlign: TextAlign.center,
                    style: theme.textTheme.bodySmall?.copyWith(
                      color: theme.colorScheme.onSurfaceVariant,
                    ),
                  ),
                ),
              ],
              Expanded(
"""
if old_column not in s:
    raise SystemExit("column anchor changed")
s = s.replace(old_column, new_column, 1)

p.write_text(s)
