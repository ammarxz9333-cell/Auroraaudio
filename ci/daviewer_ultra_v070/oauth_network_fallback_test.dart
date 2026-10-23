import 'package:dakit_api/dakit_api.dart';
import 'package:dakit_core/dakit_core.dart';
import 'package:daviewer/core/network/oauth_network_fallback.dart';
import 'package:daviewer/core/network/proxy_controller.dart';
import 'package:flutter_test/flutter_test.dart';

final class _Endpoint implements OAuthEndpoint {
  _Endpoint(this.response, {this.failure});
  final Map<String, Object?> response;
  final Object? failure;
  int calls = 0;

  @override
  Future<Map<String, Object?>> postForm(
    Uri endpoint,
    Map<String, String> form,
  ) async {
    calls++;
    if (failure case final error?) throw error;
    return response;
  }
}

DAKitException get _disconnected => const DAKitException(
  kind: DAKitFailureKind.network,
  code: 'network.connection',
  message: 'Disconnected',
);

void main() {
  final tokenUrl = Uri.parse('https://www.deviantart.com/oauth2/token');
  const response = <String, Object?>{'token_type': 'Bearer'};

  test('only failed detected system proxy falls back to direct HTTPS', () async {
    final primary = _Endpoint(const {}, failure: _disconnected);
    final direct = _Endpoint(response);
    final endpoint = SafeOAuthNetworkFallback(
      primary: primary,
      direct: direct,
      source: () => ProxySource.system,
    );
    expect(await endpoint.postForm(tokenUrl, {'grant_type': 'authorization_code'}), response);
    expect(primary.calls, 1);
    expect(direct.calls, 1);
  });

  test('manual override is never bypassed', () async {
    final primary = _Endpoint(const {}, failure: _disconnected);
    final direct = _Endpoint(response);
    final endpoint = SafeOAuthNetworkFallback(
      primary: primary,
      direct: direct,
      source: () => ProxySource.manual,
    );
    await expectLater(endpoint.postForm(tokenUrl, const {}), throwsA(isA<DAKitException>()));
    expect(direct.calls, 0);
  });

  test('HTTP and invalid-token failures never retry', () async {
    final failure = const DAKitException(
      kind: DAKitFailureKind.authentication,
      code: 'oauth.provider.invalid_grant',
      message: 'Code rejected',
    );
    final primary = _Endpoint(const {}, failure: failure);
    final direct = _Endpoint(response);
    final endpoint = SafeOAuthNetworkFallback(
      primary: primary,
      direct: direct,
      source: () => ProxySource.system,
    );
    await expectLater(endpoint.postForm(tokenUrl, const {}), throwsA(isA<DAKitException>()));
    expect(direct.calls, 0);
  });

  test('connection failure cannot redirect tokens to another origin', () {
    expect(mayRetryOAuthDirect(source: ProxySource.system,
      endpoint: Uri.parse('https://example.org/oauth2/token'),
      error: _disconnected), isFalse);
  });

  test('send or receive timeouts are not replayed (single-use code)', () {
    for (final code in ['network.send_timeout', 'network.receive_timeout']) {
      expect(mayRetryOAuthDirect(
        source: ProxySource.system,
        endpoint: tokenUrl,
        error: DAKitException(
          kind: DAKitFailureKind.network,
          code: code,
          message: code,
        ),
      ), isFalse);
    }
  });
}
