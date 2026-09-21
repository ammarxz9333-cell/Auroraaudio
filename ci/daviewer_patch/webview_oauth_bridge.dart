import 'dart:async';

import 'package:dakit_core/dakit_core.dart';
import 'package:flutter/foundation.dart';

import '../network/desktop_uri_launcher.dart';

/// Routes OAuth through either the embedded WebView (DeviantArt account login)
/// or the system browser (Google/social login).
///
/// External routing is deliberately one-shot so an abandoned Google attempt
/// cannot leak into a later embedded DeviantArt login.
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

  void launchNextExternally() {
    _launchNextExternally = true;
  }

  void cancelExternalLaunch() {
    _launchNextExternally = false;
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
