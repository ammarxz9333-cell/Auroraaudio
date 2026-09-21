import 'dart:async';
import 'dart:io';

import 'package:dakit_core/dakit_core.dart';
import 'package:flutter/services.dart';

Uri? parseAndroidOAuthCallback(String? raw) {
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

/// Android-native OAuth callback source with replay protection.
///
/// Android can deliver the OAuth intent before the Dart OAuth coordinator has
/// attached its stream listener. In that case MainActivity keeps the callback
/// pending, while this source also buffers any early MethodChannel delivery and
/// replays it as soon as the first Dart listener subscribes.
final class AndroidNativeOAuthCallbackSource
    implements InitialCallbackUriSource {
  AndroidNativeOAuthCallbackSource({
    MethodChannel? channel,
    bool? forceAndroid,
  }) : _channel = channel ?? const MethodChannel('daviewer/oauth_callback'),
       _isAndroid = forceAndroid ?? Platform.isAndroid {
    _controller = StreamController<Uri>.broadcast(
      onListen: () {
        unawaited(_replayPending());
      },
    );
    if (_isAndroid) {
      _channel.setMethodCallHandler(_handleMethodCall);
    }
  }

  final MethodChannel _channel;
  final bool _isAndroid;
  late final StreamController<Uri> _controller;
  Uri? _buffered;

  @override
  Future<Uri?> initialUri() async {
    if (!_isAndroid) return null;
    try {
      final raw = await _channel.invokeMethod<String>(
        'getPendingOAuthCallback',
      );
      return parseAndroidOAuthCallback(raw);
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
    if (raw is! String) return false;
    final uri = parseAndroidOAuthCallback(raw);
    if (uri == null || _controller.isClosed) return false;

    if (_controller.hasListener) {
      _controller.add(uri);
      return true;
    }

    _buffered = uri;
    return false;
  }

  Future<void> _replayPending() async {
    if (!_isAndroid || _controller.isClosed) return;

    final buffered = _buffered;
    if (buffered != null) {
      _buffered = null;
      _controller.add(buffered);
      try {
        await _channel.invokeMethod<void>(
          'ackOAuthCallback',
          <String, Object>{'callback': buffered.toString()},
        );
      } on Object {
        // The Dart copy has already been delivered; a stale native copy is
        // harmless and will be superseded by the next callback.
      }
      return;
    }

    try {
      final raw = await _channel.invokeMethod<String>(
        'getPendingOAuthCallback',
      );
      final uri = parseAndroidOAuthCallback(raw);
      if (uri != null && !_controller.isClosed && _controller.hasListener) {
        _controller.add(uri);
      }
    } on Object {
      // Best effort: the live MethodChannel delivery remains active.
    }
  }

  Future<void> dispose() async {
    if (_isAndroid) {
      _channel.setMethodCallHandler(null);
    }
    await _controller.close();
  }
}
