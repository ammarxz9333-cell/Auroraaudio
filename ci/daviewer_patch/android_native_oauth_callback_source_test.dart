import 'package:daviewer/core/auth/android_native_oauth_callback_source.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test('accepts only the DAViewer OAuth callback URI', () {
    final uri = parseAndroidOAuthCallback(
      'dakit://oauth/callback?code=abc&state=xyz',
    );
    expect(uri, isNotNull);
    expect(uri!.scheme, 'dakit');
    expect(uri.host, 'oauth');
    expect(uri.path, '/callback');
    expect(uri.queryParameters['code'], 'abc');
    expect(uri.queryParameters['state'], 'xyz');
  });

  test('rejects unrelated or malformed deep links', () {
    expect(parseAndroidOAuthCallback(null), isNull);
    expect(parseAndroidOAuthCallback(''), isNull);
    expect(parseAndroidOAuthCallback('https://example.com/callback'), isNull);
    expect(parseAndroidOAuthCallback('dakit://oauth/other?code=x'), isNull);
    expect(parseAndroidOAuthCallback('dakit://other/callback?code=x'), isNull);
  });

  test('replays a callback that arrived before the Dart listener', () async {
    const channel = MethodChannel('daviewer/oauth_callback_test_replay');
    const raw = 'dakit://oauth/callback?code=early&state=replay';

    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
          if (call.method == 'getPendingOAuthCallback') return raw;
          if (call.method == 'ackOAuthCallback') return true;
          return null;
        });

    final source = AndroidNativeOAuthCallbackSource(
      channel: channel,
      forceAndroid: true,
    );
    addTearDown(() async {
      await source.dispose();
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(channel, null);
    });

    final uri = await source.uris.first.timeout(const Duration(seconds: 1));
    expect(uri.queryParameters['code'], 'early');
    expect(uri.queryParameters['state'], 'replay');
  });

  test('cold-start callback is returned through initialUri', () async {
    const channel = MethodChannel('daviewer/oauth_callback_test_initial');
    const raw = 'dakit://oauth/callback?code=cold&state=start';

    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
          if (call.method == 'getPendingOAuthCallback') return raw;
          return null;
        });

    final source = AndroidNativeOAuthCallbackSource(
      channel: channel,
      forceAndroid: true,
    );
    addTearDown(() async {
      await source.dispose();
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(channel, null);
    });

    final uri = await source.initialUri();
    expect(uri, isNotNull);
    expect(uri!.queryParameters['code'], 'cold');
    expect(uri.queryParameters['state'], 'start');
  });
}
