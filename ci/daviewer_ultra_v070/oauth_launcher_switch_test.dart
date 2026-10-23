import 'package:dakit_core/dakit_core.dart';
import 'package:daviewer/core/auth/webview_oauth_bridge.dart';
import 'package:flutter_test/flutter_test.dart';

final class _ExternalLauncher implements ExternalUriLauncher {
  int launches = 0;

  @override
  Future<void> launch(Uri uri) async {
    launches++;
  }
}

void main() {
  test('Android OAuth can be switched from external browser to WebView', () async {
    final fallback = _ExternalLauncher();
    final bridge = WebViewOAuthBridge(
      fallback: fallback,
      preferExternalBrowser: true,
    );
    final messages = <Uri>[];
    final subscription = bridge.launchRequests.listen(messages.add);
    final uri = Uri.parse('https://www.deviantart.com/oauth2/authorize');

    await bridge.launch(uri);
    expect(fallback.launches, 1);
    expect(messages, isEmpty);

    bridge.preferExternalBrowser = false;
    await bridge.launch(uri);
    await Future<void>.delayed(Duration.zero);
    expect(messages, [uri]);
    expect(fallback.launches, 1);

    await subscription.cancel();
    await bridge.dispose();
  });
}
