import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_inappwebview/flutter_inappwebview.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:url_launcher/url_launcher.dart';

import '../../core/auth/auth_controller.dart';
import '../../core/auth/auth_state.dart';
import '../../core/auth/session_state.dart';
import '../../core/auth/web_session_controller.dart';
import '../../core/auth/webview_oauth_bridge.dart';

import 'package:dakit_web/dakit_web.dart';

import '../../core/diagnostics/error_text.dart';
import '../../core/l10n/app_strings.dart';
import '../../core/runtime/runtime_provider.dart';

/// Google blocks OAuth in embedded user-agents. Social-provider navigation must
/// leave the WebView and continue in the user's real browser.
bool shouldOpenIdentityProviderExternally(Uri uri) {
  final host = uri.host.toLowerCase();
  return host == 'accounts.google.com' ||
      host.endsWith('.accounts.google.com') ||
      host == 'appleid.apple.com' ||
      host == 'facebook.com' ||
      host.endsWith('.facebook.com');
}

/// Hosts the embedded DeviantArt WebView.
///
/// DeviantArt-account login remains embedded so it can establish the website
/// Cookie/CSRF session. Google/Apple/Facebook hops are handed to the system
/// browser, while the same OAuth/PKCE transaction keeps waiting for the
/// dakit:// callback through AppLinksCallbackUriSource.
final class WebLoginScreen extends ConsumerStatefulWidget {
  const WebLoginScreen({super.key});

  @override
  ConsumerState<WebLoginScreen> createState() => _WebLoginScreenState();
}

final class _WebLoginScreenState extends ConsumerState<WebLoginScreen> {
  static final Uri _loginUri = Uri.parse(
    'https://www.deviantart.com/users/login',
  );
  static final Uri _homeUri = Uri.parse('https://www.deviantart.com/');
  static final Uri _contentSettingsUri = Uri.parse(
    'https://www.deviantart.com/settings/browsing',
  );

  InAppWebViewController? _controller;
  StreamSubscription<Uri>? _launchSub;
  Uri? _pendingAuthUri;
  int? _popupWindowId;
  bool _loading = true;
  bool _closeAfterReport = false;
  bool _externalProviderUsed = false;
  bool _externalLaunchInProgress = false;
  int _reportSeq = 0;
  double _progress = 0;
  late final AuthController _authController;

  WebViewOAuthBridge? get _bridge =>
      ref.read(runtimeProvider).webViewOAuthBridge;

  @override
  void initState() {
    super.initState();
    _authController = ref.read(authControllerProvider.notifier);
    final bridge = _bridge;
    if (bridge != null) {
      _launchSub = bridge.launchRequests.listen(_loadAuthRequest);
    }
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

  void _loadAuthRequest(Uri uri) {
    final controller = _controller;
    if (controller == null) {
      _pendingAuthUri = uri;
      return;
    }
    controller.loadUrl(urlRequest: URLRequest(url: WebUri(uri.toString())));
  }

  Future<bool> _openProviderExternally(WebUri? webUri) async {
    if (webUri == null) return false;
    final uri = Uri.tryParse(webUri.toString());
    if (uri == null || !shouldOpenIdentityProviderExternally(uri)) return false;

    _externalProviderUsed = true;
    if (_externalLaunchInProgress) return true;
    _externalLaunchInProgress = true;
    try {
      final launched = await launchUrl(
        uri,
        mode: LaunchMode.externalApplication,
      );
      if (!launched && mounted) {
        final s = strings(ref.read(appLanguageProvider));
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text(s.loginBrowserOpenFailed)),
        );
      }
    } finally {
      _externalLaunchInProgress = false;
      if (mounted && _popupWindowId != null) {
        setState(() => _popupWindowId = null);
      }
    }
    return true;
  }

  Future<NavigationActionPolicy?> _handleNavigation(
    InAppWebViewController controller,
    NavigationAction navigationAction,
  ) async {
    final uri = navigationAction.request.url;
    if (uri != null && uri.scheme == 'dakit' && uri.host == 'oauth') {
      _bridge?.addCallback(Uri.parse(uri.toString()));
      unawaited(
        _controller?.loadUrl(
          urlRequest: URLRequest(url: WebUri(_homeUri.toString())),
        ),
      );
      if (mounted && _popupWindowId != null) {
        setState(() => _popupWindowId = null);
      }
      return NavigationActionPolicy.CANCEL;
    }
    if (await _openProviderExternally(uri)) {
      return NavigationActionPolicy.CANCEL;
    }
    return NavigationActionPolicy.ALLOW;
  }

  Future<void> _reportWebSession() async {
    final controller = _controller;
    if (controller == null) return;
    final seq = ++_reportSeq;
    try {
      final raw = await controller.evaluateJavascript(
        source: "JSON.stringify({csrf: window.__CSRF_TOKEN__ || ''})",
      );
      if (seq != _reportSeq) return;
      if (raw is! String || raw.isEmpty) return;
      final data = jsonDecode(raw) as Map<String, dynamic>;
      final csrf = (data['csrf'] as String?) ?? '';
      if (csrf.isEmpty) return;
      final username = await ref.read(webSessionProvider).webUsername();
      final isLoggedIn = username.isNotEmpty;
      debugPrint(
        '[web-session] csrf=${csrf.length} isLoggedIn=$isLoggedIn '
        'username=$username',
      );
      await ref
          .read(webSessionControllerProvider.notifier)
          .report(csrf: csrf, username: username);
      if (isLoggedIn && mounted) {
        WidgetsBinding.instance.addPostFrameCallback((_) => _closeScreen());
      } else {
        _maybeClose();
      }
    } on Object {
      // Best effort; the page may not expose the state during navigation.
    }
  }

  void _maybeClose() {
    if (!_closeAfterReport || !mounted) return;
    _closeAfterReport = false;
    WidgetsBinding.instance.addPostFrameCallback((_) => _closeScreen());
  }

  void _closeScreen() {
    if (!mounted) return;
    final navigator = Navigator.of(context);
    if (navigator.canPop()) {
      navigator.pop();
    } else {
      context.go('/');
    }
  }

  @override
  Widget build(BuildContext context) {
    final s = strings(ref.watch(appLanguageProvider));
    final theme = Theme.of(context);
    final auth = ref.watch(authControllerProvider);

    ref.listen<AuthState>(authControllerProvider, (previous, next) {
      if (previous?.status != AuthStatus.signedIn &&
          next.status == AuthStatus.signedIn &&
          mounted) {
        // A social provider completed in the system browser. Chrome's cookies
        // cannot be copied into Android WebView, so do not wait for a web-cookie
        // report that cannot arrive. OAuth-backed features are ready now; Home
        // has an OAuth fallback for the recommendation tab.
        if (_externalProviderUsed) {
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

    final popupWindowId = _popupWindowId;

    return Scaffold(
      resizeToAvoidBottomInset: false,
      appBar: AppBar(
        title: Text(s.signInWelcomeTitle),
        actions: <Widget>[
          IconButton(
            tooltip: s.settings,
            onPressed: () => context.push('/settings'),
            icon: const Icon(Icons.settings_outlined),
          ),
          IconButton(
            tooltip: s.contentSettings,
            onPressed: () => launchUrl(
              _contentSettingsUri,
              mode: LaunchMode.externalApplication,
            ),
            icon: const Icon(Icons.visibility_outlined),
          ),
          IconButton(
            tooltip: s.done,
            onPressed: _closeScreen,
            icon: const Icon(Icons.check),
          ),
        ],
        bottom: _loading
            ? PreferredSize(
                preferredSize: const Size.fromHeight(3),
                child: LinearProgressIndicator(
                  minHeight: 3,
                  value: _progress > 0 && _progress < 1 ? _progress : null,
                ),
              )
            : null,
      ),
      body: Stack(
        children: <Widget>[
          Column(
            children: <Widget>[
              if (auth.error != null)
                _LoginErrorBanner(
                  message: friendlyLoginErrorMessage(auth.error!, s),
                ),
              if (!auth.oauthSignedIn) _VerificationHint(s: s),
              Expanded(
                child: ColoredBox(
                  color: theme.scaffoldBackgroundColor,
                  child: InAppWebView(
                    initialUrlRequest: URLRequest(
                      url: WebUri(_loginUri.toString()),
                    ),
                    initialSettings: InAppWebViewSettings(
                      javaScriptEnabled: true,
                      userAgent: webUserAgent,
                      supportMultipleWindows: true,
                      javaScriptCanOpenWindowsAutomatically: true,
                    ),
                    onWebViewCreated: (controller) {
                      _controller = controller;
                      final pending = _pendingAuthUri;
                      if (pending != null) {
                        _pendingAuthUri = null;
                        controller.loadUrl(
                          urlRequest: URLRequest(
                            url: WebUri(pending.toString()),
                          ),
                        );
                      }
                    },
                    shouldOverrideUrlLoading: _handleNavigation,
                    onCreateWindow: (controller, action) async {
                      final direct = action.request.url;
                      if (await _openProviderExternally(direct)) return false;
                      if (mounted) {
                        setState(() => _popupWindowId = action.windowId);
                      }
                      return true;
                    },
                    onLoadStart: (controller, url) {
                      if (mounted) setState(() => _loading = true);
                    },
                    onLoadStop: (controller, url) {
                      if (mounted) setState(() => _loading = false);
                      final uri = url;
                      if (uri != null && uri.host == 'www.deviantart.com') {
                        unawaited(_reportWebSession());
                      }
                    },
                    onProgressChanged: (controller, progress) {
                      if (!mounted || !_loading) return;
                      final next = (progress / 100).clamp(0.0, 1.0).toDouble();
                      if ((next - _progress).abs() < 0.02) return;
                      setState(() => _progress = next);
                    },
                  ),
                ),
              ),
            ],
          ),
          if (popupWindowId != null)
            Positioned.fill(
              child: ColoredBox(
                color: theme.scaffoldBackgroundColor,
                child: InAppWebView(
                  key: ValueKey<int>(popupWindowId),
                  windowId: popupWindowId,
                  initialSettings: InAppWebViewSettings(
                    javaScriptEnabled: true,
                    userAgent: webUserAgent,
                    supportMultipleWindows: true,
                    javaScriptCanOpenWindowsAutomatically: true,
                  ),
                  shouldOverrideUrlLoading: _handleNavigation,
                  onCreateWindow: (controller, action) async {
                    if (await _openProviderExternally(action.request.url)) {
                      return false;
                    }
                    return false;
                  },
                  onCloseWindow: (controller) {
                    if (mounted) setState(() => _popupWindowId = null);
                  },
                ),
              ),
            ),
          if (_externalProviderUsed && !auth.oauthSignedIn)
            Positioned(
              left: 12,
              right: 12,
              bottom: 12,
              child: Material(
                elevation: 3,
                borderRadius: BorderRadius.circular(12),
                color: theme.colorScheme.secondaryContainer,
                child: Padding(
                  padding: const EdgeInsets.all(12),
                  child: Row(
                    children: <Widget>[
                      const SizedBox.square(
                        dimension: 20,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      ),
                      const SizedBox(width: 12),
                      Expanded(
                        child: Text(
                          'Google sign-in is open in your browser. '
                          'Finish there and DAViewer will resume automatically.',
                          style: theme.textTheme.bodySmall,
                        ),
                      ),
                    ],
                  ),
                ),
              ),
            ),
          if (_closeAfterReport) _LoginSuccessOverlay(s: s),
        ],
      ),
    );
  }
}

final class _VerificationHint extends StatelessWidget {
  const _VerificationHint({required this.s});

  final AppStrings s;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Material(
      color: scheme.surfaceContainerHighest,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
        child: Row(
          children: <Widget>[
            Icon(
              Icons.shield_outlined,
              size: 16,
              color: scheme.onSurfaceVariant,
            ),
            const SizedBox(width: 8),
            Expanded(
              child: Text(
                s.verificationHint,
                style: Theme.of(context).textTheme.bodySmall
                    ?.copyWith(color: scheme.onSurfaceVariant),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

final class _LoginErrorBanner extends StatelessWidget {
  const _LoginErrorBanner({required this.message});

  final String message;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Material(
      color: scheme.errorContainer,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            Icon(Icons.error_outline, size: 18, color: scheme.onErrorContainer),
            const SizedBox(width: 8),
            Expanded(
              child: Text(
                message,
                style: Theme.of(context).textTheme.bodySmall
                    ?.copyWith(color: scheme.onErrorContainer),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

final class _LoginSuccessOverlay extends StatelessWidget {
  const _LoginSuccessOverlay({required this.s});

  final AppStrings s;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return ColoredBox(
      color: scheme.surface.withValues(alpha: 0.92),
      child: Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: <Widget>[
            const Icon(
              Icons.check_circle_outline,
              size: 56,
              color: Colors.green,
            ),
            const SizedBox(height: 12),
            Text(
              s.loginSuccess,
              style: Theme.of(context).textTheme.titleMedium
                  ?.copyWith(color: scheme.onSurface),
            ),
            const SizedBox(height: 4),
            Text(
              s.syncingAfterLogin,
              style: Theme.of(context).textTheme.bodySmall
                  ?.copyWith(color: scheme.onSurfaceVariant),
            ),
          ],
        ),
      ),
    );
  }
}
