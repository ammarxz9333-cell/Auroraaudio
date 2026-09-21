import 'package:dakit_api/dakit_api.dart';
import 'package:dakit_core/dakit_core.dart';
import 'package:dio/dio.dart';
import 'package:test/test.dart';

void main() {
  test('GET retries transient connection errors before surfacing failure', () async {
    final clock = _FakeClock(DateTime.utc(2026, 9, 21, 20));
    var requests = 0;
    final dio = Dio();
    dio.interceptors.add(
      InterceptorsWrapper(
        onRequest: (options, handler) {
          requests += 1;
          if (requests <= 2) {
            handler.reject(
              DioException(
                requestOptions: options,
                type: DioExceptionType.connectionError,
                message: 'temporary connection loss',
              ),
            );
            return;
          }
          handler.resolve(
            Response<Object?>(
              requestOptions: options,
              statusCode: 200,
              data: const <String, Object?>{'status': 'ok'},
            ),
          );
        },
      ),
    );

    final client = OfficialApiClient(
      session: _StaticTokenProvider(
        AuthTokens(
          accessToken: 'access',
          tokenType: 'Bearer',
          expiresAt: clock.now().add(const Duration(hours: 1)),
        ),
      ),
      dio: dio,
      now: clock.now,
      delay: clock.delay,
      config: ApiConfig(
        retryPolicy: const RetryPolicy(maxRetries: 3),
        rateLimitPolicy: const RateLimitPolicy(minimumSpacing: Duration.zero),
      ),
    );

    final result = await client.getJson('browse/home');

    expect(result['status'], 'ok');
    expect(requests, 3);
    expect(clock.delays, const <Duration>[
      Duration(seconds: 1),
      Duration(seconds: 2),
    ]);
  });

  test('GET serves stale cache during transient connection failure', () async {
    final clock = _FakeClock(DateTime.utc(2026, 9, 21, 20));
    var requests = 0;
    final dio = Dio();
    dio.interceptors.add(
      InterceptorsWrapper(
        onRequest: (options, handler) {
          requests += 1;
          if (requests == 1) {
            handler.resolve(
              Response<Object?>(
                requestOptions: options,
                statusCode: 200,
                data: const <String, Object?>{'value': 'cached'},
              ),
            );
            return;
          }
          handler.reject(
            DioException(
              requestOptions: options,
              type: DioExceptionType.connectionError,
              message: 'temporary connection loss',
            ),
          );
        },
      ),
    );

    final client = OfficialApiClient(
      session: _StaticTokenProvider(
        AuthTokens(
          accessToken: 'access',
          tokenType: 'Bearer',
          expiresAt: clock.now().add(const Duration(hours: 1)),
        ),
      ),
      dio: dio,
      now: clock.now,
      delay: clock.delay,
      config: ApiConfig(
        retryPolicy: const RetryPolicy(maxRetries: 3),
        rateLimitPolicy: const RateLimitPolicy(
          minimumSpacing: Duration.zero,
          freshCacheTtl: Duration(seconds: 45),
          staleCacheTtl: Duration(minutes: 15),
        ),
      ),
    );

    final first = await client.getJson('browse/home');
    clock.current = clock.current.add(const Duration(minutes: 1));
    final second = await client.getJson('browse/home');

    expect(first['value'], 'cached');
    expect(second['value'], 'cached');
    expect(requests, 2);
  });
}

final class _FakeClock {
  _FakeClock(this.current);

  DateTime current;
  final List<Duration> delays = <Duration>[];

  DateTime now() => current;

  Future<void> delay(Duration duration) async {
    delays.add(duration);
    current = current.add(duration);
  }
}

final class _StaticTokenProvider implements AuthTokenProvider {
  _StaticTokenProvider(this.tokens);

  final AuthTokens tokens;

  @override
  Future<AuthTokens> validTokens({bool forceRefresh = false}) async => tokens;
}
