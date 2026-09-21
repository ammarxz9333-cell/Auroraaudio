import 'package:dakit_api/dakit_api.dart';
import 'package:dakit_core/dakit_core.dart';
import 'package:dio/dio.dart';
import 'package:test/test.dart';

void main() {
  final now = DateTime.utc(2026, 9, 21, 12);
  final config = OAuthConfig(
    clientId: '12345',
    redirectUri: Uri.parse('dakit://oauth/callback'),
  );

  test('OAuth 429 is rate-limit, never invalid refresh-token evidence', () {
    final dio = Dio();
    dio.interceptors.add(
      InterceptorsWrapper(
        onRequest: (options, handler) => handler.reject(
          DioException.badResponse(
            statusCode: 429,
            requestOptions: options,
            response: Response<Object?>(
              requestOptions: options,
              statusCode: 429,
              headers: Headers.fromMap(
                const <String, List<String>>{
                  'retry-after': <String>['30'],
                },
              ),
              data: const <String, Object?>{
                'error': 'invalid_request',
                'error_description': 'The refresh_token is invalid.',
              },
            ),
          ),
        ),
      ),
    );

    expect(
      () => DioOAuthEndpoint(dio: dio).postForm(
        Uri.parse('https://www.deviantart.com/oauth2/token'),
        const <String, String>{
          'grant_type': 'refresh_token',
          'refresh_token': 'refresh',
        },
      ),
      throwsA(
        isA<DAKitException>()
            .having(
              (error) => error.kind,
              'kind',
              DAKitFailureKind.rateLimit,
            )
            .having(
              (error) => error.code,
              'code',
              'oauth.http.rate_limited',
            )
            .having((error) => error.retryable, 'retryable', isTrue),
      ),
    );
  });

  test('one or two real invalid-refresh replies do not erase session', () async {
    final store = MemoryTokenStore(
      AuthTokens(
        accessToken: 'old-access',
        tokenType: 'Bearer',
        refreshToken: 'refresh',
        expiresAt: now.subtract(const Duration(minutes: 1)),
      ),
    );
    final session = OAuthSession(
      config: config,
      store: store,
      tokenClient: OAuthTokenClient(
        endpoint: ConfirmedInvalidEndpoint(),
        now: () => now,
      ),
      now: () => now,
    );

    for (var attempt = 0; attempt < 2; attempt += 1) {
      await expectLater(
        session.validTokens(),
        throwsA(
          isA<DAKitException>().having(
            (error) => error.code,
            'code',
            'oauth.refresh.invalid',
          ),
        ),
      );
      await Future<void>.delayed(Duration.zero);
      expect(store.value?.refreshToken, 'refresh');
      expect(session.generation, 0);
    }

    final invalidation = session.invalidations.first;
    await expectLater(
      session.validTokens(),
      throwsA(
        isA<DAKitException>().having(
          (error) => error.code,
          'code',
          'oauth.refresh.invalid',
        ),
      ),
    );

    expect((await invalidation).code, 'oauth.refresh.invalid');
    expect(store.value, isNull);
    expect(session.generation, 1);
  });
}

final class ConfirmedInvalidEndpoint implements OAuthEndpoint {
  @override
  Future<Map<String, Object?>> postForm(
    Uri endpoint,
    Map<String, String> form,
  ) => throw const DAKitException(
    kind: DAKitFailureKind.authentication,
    code: 'oauth.refresh.invalid',
    message: 'The refresh token is invalid.',
    details: <String, Object?>{'status': 400},
  );
}

final class MemoryTokenStore implements TokenStore {
  MemoryTokenStore(this.value);

  AuthTokens? value;

  @override
  Future<void> clear() async => value = null;

  @override
  Future<AuthTokens?> read() async => value;

  @override
  Future<void> write(AuthTokens tokens) async => value = tokens;
}
