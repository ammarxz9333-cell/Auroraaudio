import 'package:daviewer/core/auth/android_native_oauth_callback_source.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
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
}
