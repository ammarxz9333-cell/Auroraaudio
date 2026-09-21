import 'package:dakit_core/dakit_core.dart';
import 'package:daviewer/core/auth/webview_oauth_bridge.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('Google external OAuth routing is one-shot', () async {
    final fallback = _RecordingLauncher();
    final bridge = WebViewOAuthBridge(fallback: fallback);
    final embedded = <Uri>[];
    final sub = bridge.launchRequests.listen(embedded.add);

    final googleAttempt = Uri.parse(
      'https://www.deviantart.com/oauth2/authorize?state=google',
    );
    final normalAttempt = Uri.parse(
      'https://www.deviantart.com/oauth2/authorize?state=normal',
    );

    bridge.launchNextExternally();
    await bridge.launch(googleAttempt);
    await bridge.launch(normalAttempt);
    await Future<void>.delayed(Duration.zero);

    expect(fallback.launched, <Uri>[googleAttempt]);
    expect(embedded, <Uri>[normalAttempt]);

    bridge.finishExternalAuthorization();
    await sub.cancel();
    await bridge.dispose();
  });

  test('cancelled external routing does not affect next login', () async {
    final fallback = _RecordingLauncher();
    final bridge = WebViewOAuthBridge(fallback: fallback);
    final embedded = <Uri>[];
    final sub = bridge.launchRequests.listen(embedded.add);
    final uri = Uri.parse('https://www.deviantart.com/oauth2/authorize');

    bridge.launchNextExternally();
    bridge.cancelExternalLaunch();
    await bridge.launch(uri);
    await Future<void>.delayed(Duration.zero);

    expect(fallback.launched, isEmpty);
    expect(embedded, <Uri>[uri]);

    await sub.cancel();
    await bridge.dispose();
  });
}

final class _RecordingLauncher implements ExternalUriLauncher {
  final List<Uri> launched = <Uri>[];

  @override
  Future<void> launch(Uri uri) async {
    launched.add(uri);
  }
}
