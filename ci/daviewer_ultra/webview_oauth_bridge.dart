import 'dart:async';

import 'package:dakit_core/dakit_core.dart';
import 'package:flutter/foundation.dart';

import '../network/desktop_uri_launcher.dart';

/// Hybrid OAuth routing for the personal Ultra build.
///
/// Normal DeviantArt authorization stays in the embedded WebView so a web
/// Cookie/CSRF session can be established. Google/social authorization can be
/// explicitly routed through the system browser, where providers do not reject
/// the login as an embedded-browser flow.
///
/// External routing is one-shot: an abandoned browser attempt cannot leak into
/// a later embedded authorization.
final class WebViewOAuthBridge implements ExternalUriLauncher {
  WebViewOAuthBridge({ExternalUriLauncher? fallback})
    : _fallback = fallback ?? const DesktopUriLauncher();

  final ExternalUriLauncher _fallback;
  final StreamController<Uri> _launchController =
      StreamController<Uri>.broadcast();
  final StreamController<Uri> _callbackController =
      StreamController<Uri>.broadcast();

  bool _launchNextExternally = false;
  Uri? _externalAuthorizationUri;

  Stream<Uri> get launchRequests => _launchController.stream;

  CallbackUriSource get callbacks =>
      _StreamCallbackSource(_callbackController.stream);

  void addCallback(Uri uri) {
    if (uri.scheme != 'dakit' || uri.host != 'oauth') return;
    _callbackController.add(uri);
  }

  /// Routes only the next OAuth authorize URL through the system browser.
  void launchNextExternally() {
    _launchNextExternally = true;
  }

  bool get canReopenExternalAuthorization => _externalAuthorizationUri != null;

  Future<void> reopenExternalAuthorization() async {
    final uri = _externalAuthorizationUri;
    if (uri != null) await _fallback.launch(uri);
  }

  void finishExternalAuthorization() {
    _externalAuthorizationUri = null;
    _launchNextExternally = false;
  }

  @override
  Future<void> launch(Uri uri) async {
    if (_launchNextExternally) {
      _launchNextExternally = false;
      _externalAuthorizationUri = uri;
      debugPrint('[oauth] routing authorize to system browser');
      try {
        await _fallback.launch(uri);
      } on Object {
        _externalAuthorizationUri = null;
        rethrow;
      }
      return;
    }

    // Give the login route a short window to subscribe before using the
    // existing system-browser safety fallback.
    for (var i = 0; i < 25 && !_launchController.hasListener; i++) {
      await Future<void>.delayed(const Duration(milliseconds: 20));
    }
    if (_launchController.hasListener) {
      debugPrint('[oauth] routing authorize to embedded WebView');
      _launchController.add(uri);
    } else {
      debugPrint('[oauth] no WebView listener -> system browser fallback');
      _externalAuthorizationUri = uri;
      await _fallback.launch(uri);
    }
  }

  Future<void> dispose() async {
    await _launchController.close();
    await _callbackController.close();
  }
}

final class _StreamCallbackSource implements CallbackUriSource {
  const _StreamCallbackSource(this._stream);

  final Stream<Uri> _stream;

  @override
  Stream<Uri> get uris => _stream;
}
