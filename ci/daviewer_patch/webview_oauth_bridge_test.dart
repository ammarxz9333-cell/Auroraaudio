import 'package:dakit_core/dakit_core.dart';
import 'package:daviewer/core/auth/webview_oauth_bridge.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('Google/social OAuth external route is one-shot', () async {
    final fallback = _RecordingLauncher();
    final bridge = WebViewOAuthBridge(fallback: fallback);
    final embedded = <Uri>[];
    final subscription = bridge.launchRequests.listen(embedded.add);

    final first = Uri.parse(
      'https://www.deviantart.com/oauth2/authorize?state=google',
    );
    final second = Uri.parse(
      'https://www.deviantart.com/oauth2/authorize?state=embedded',
    );

    bridge.launchNextExternally();
    await bridge.launch(first);
    await bridge.launch(second);
    await Future<void>.delayed(Duration.zero);

    expect(fallback.launched, <Uri>[first]);
    expect(embedded, <Uri>[second]);
    expect(bridge.canReopenExternalAuthorization, isTrue);

    await bridge.reopenExternalAuthorization();
    expect(fallback.launched, <Uri>[first, first]);

    bridge.finishExternalAuthorization();
    expect(bridge.canReopenExternalAuthorization, isFalse);

    await subscription.cancel();
    await bridge.dispose();
  });

  test('cancelled external choice does not affect next embedded login', () async {
    final fallback = _RecordingLauncher();
    final bridge = WebViewOAuthBridge(fallback: fallback);
    final embedded = <Uri>[];
    final subscription = bridge.launchRequests.listen(embedded.add);
    final uri = Uri.parse('https://www.deviantart.com/oauth2/authorize');

    bridge.launchNextExternally();
    bridge.cancelExternalLaunch();
    await bridge.launch(uri);
    await Future<void>.delayed(Duration.zero);

    expect(fallback.launched, isEmpty);
    expect(embedded, <Uri>[uri]);

    await subscription.cancel();
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
