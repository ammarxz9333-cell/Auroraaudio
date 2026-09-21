import 'dart:async';
import 'dart:io';

import 'package:dakit_core/dakit_core.dart';
import 'package:flutter/services.dart';

/// Android-native OAuth callback source.
///
/// This bypasses plugin routing for the critical browser -> app OAuth redirect.
/// MainActivity captures both cold-start intents and warm-start onNewIntent()
/// deliveries and forwards only dakit://oauth/callback URIs to this channel.
final class AndroidNativeOAuthCallbackSource
    implements InitialCallbackUriSource {
  AndroidNativeOAuthCallbackSource() {
    if (Platform.isAndroid) {
      _channel.setMethodCallHandler(_handleMethodCall);
    }
  }

  static const MethodChannel _channel = MethodChannel(
    'daviewer/oauth_callback',
  );

  final StreamController<Uri> _controller =
      StreamController<Uri>.broadcast();

  @override
  Future<Uri?> initialUri() async {
    if (!Platform.isAndroid) return null;
    try {
      final raw = await _channel.invokeMethod<String>(
        'getInitialOAuthCallback',
      );
      return _parse(raw);
    } on PlatformException {
      return null;
    } on MissingPluginException {
      return null;
    }
  }

  @override
  Stream<Uri> get uris => _controller.stream;

  Future<dynamic> _handleMethodCall(MethodCall call) async {
    if (call.method != 'oauthCallback') return null;
    final raw = call.arguments;
    if (raw is! String) return null;
    final uri = _parse(raw);
    if (uri != null && !_controller.isClosed) {
      _controller.add(uri);
    }
    return true;
  }

  Uri? _parse(String? raw) {
    if (raw == null || raw.isEmpty) return null;
    final uri = Uri.tryParse(raw);
    if (uri == null ||
        uri.scheme != 'dakit' ||
        uri.host != 'oauth' ||
        uri.path != '/callback') {
      return null;
    }
    return uri;
  }

  Future<void> dispose() async {
    if (Platform.isAndroid) {
      await _channel.setMethodCallHandler(null);
    }
    await _controller.close();
  }
}
