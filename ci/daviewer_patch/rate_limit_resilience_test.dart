import 'package:dakit_api/dakit_api.dart';
import 'package:dakit_core/dakit_core.dart';
import 'package:dio/dio.dart';
import 'package:test/test.dart';

void main() {
  test('uses one conservative retry on 429 and then succeeds', () async {
    final clock = FakeClock(DateTime.utc(2026, 9, 21, 8));
    final harness = Harness(
      clock: clock,
      responses: <ResponseSpec>[
        const ResponseSpec(429, <String, Object?>{'error': 'rate_limit'}),
        const ResponseSpec(200, <String, Object?>{'status': 'success'}),
      ],
    );

    final result = await harness.client.getJson('browse/dailydeviations');

    expect(result['status'], 'success');
    expect(harness.requests, 2);
    expect(clock.delays, <Duration>[const Duration(seconds: 5)]);
  });

  test('honours Retry-After when it is longer than local backoff', () async {
    final clock = FakeClock(DateTime.utc(2026, 9, 21, 8));
    final harness = Harness(
      clock: clock,
      responses: <ResponseSpec>[
        const ResponseSpec(
          429,
          <String, Object?>{'error': 'rate_limit'},
          headers: <String, List<String>>{'retry-after': <String>['45']},
        ),
        const ResponseSpec(200, <String, Object?>{'status': 'success'}),
      ],
    );

    await harness.client.getJson('browse/dailydeviations');

    expect(clock.delays, <Duration>[const Duration(seconds: 45)]);
  });

  test('coalesces identical concurrent GET requests', () async {
    final clock = FakeClock(DateTime.utc(2026, 9, 21, 8));
    final harness = Harness(
      clock: clock,
      responses: <ResponseSpec>[
        const ResponseSpec(200, <String, Object?>{'status': 'success'}),
      ],
    );

    final first = harness.client.getJson(
      'browse/home',
      query: const <String, Object?>{'limit': 50},
    );
    final second = harness.client.getJson(
      'browse/home',
      query: const <String, Object?>{'limit': 50},
    );
    await Future.wait(<Future<Map<String, Object?>>>[first, second]);

    expect(harness.requests, 1);
  });

  test('serves a fresh cached GET without touching the network', () async {
    final clock = FakeClock(DateTime.utc(2026, 9, 21, 8));
    final harness = Harness(
      clock: clock,
      responses: <ResponseSpec>[
        const ResponseSpec(200, <String, Object?>{'value': 'cached'}),
      ],
    );

    final first = await harness.client.getJson('browse/home');
    clock.current = clock.current.add(const Duration(minutes: 5));
    final second = await harness.client.getJson('browse/home');

    expect(first['value'], 'cached');
    expect(second['value'], 'cached');
    expect(harness.requests, 1);
  });

  test('429 cooldown serves stale cache without another provider request', () async {
    final clock = FakeClock(DateTime.utc(2026, 9, 21, 8));
    final harness = Harness(
      clock: clock,
      responses: <ResponseSpec>[
        const ResponseSpec(200, <String, Object?>{'value': 'cached'}),
        const ResponseSpec(
          429,
          <String, Object?>{'error': 'rate_limit'},
          headers: <String, List<String>>{'retry-after': <String>['60']},
        ),
      ],
    );

    final first = await harness.client.getJson('browse/home');
    clock.current = clock.current.add(const Duration(minutes: 11));
    final second = await harness.client.getJson('browse/home');
    final third = await harness.client.getJson('browse/home');

    expect(first['value'], 'cached');
    expect(second['value'], 'cached');
    expect(third['value'], 'cached');
    expect(harness.requests, 2);
  });

  test('paces distinct official API requests through the shared gate', () async {
    final clock = FakeClock(DateTime.utc(2026, 9, 21, 8));
    final harness = Harness(
      clock: clock,
      minimumSpacing: const Duration(milliseconds: 350),
      responses: <ResponseSpec>[
        const ResponseSpec(200, <String, Object?>{'status': 'success'}),
        const ResponseSpec(200, <String, Object?>{'status': 'success'}),
      ],
    );

    await harness.client.getJson('placebo');
    await harness.client.getJson('browse/dailydeviations');

    expect(clock.delays, <Duration>[const Duration(milliseconds: 350)]);
  });
}

final class Harness {
  Harness({
    required FakeClock clock,
    required List<ResponseSpec> responses,
    Duration minimumSpacing = Duration.zero,
  }) {
    final dio = Dio();
    dio.interceptors.add(
      InterceptorsWrapper(
        onRequest: (options, handler) {
          requests += 1;
          final spec = responses.removeAt(0);
          handler.resolve(
            Response<Object?>(
              requestOptions: options,
              statusCode: spec.status,
              data: spec.data,
              headers: Headers.fromMap(spec.headers),
            ),
          );
        },
      ),
    );
    client = OfficialApiClient(
      session: StaticTokenProvider(
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
        userAgent: 'DAKit-RateLimit-Test/1.0',
        rateLimitPolicy: RateLimitPolicy(minimumSpacing: minimumSpacing),
      ),
    );
  }

  late final OfficialApiClient client;
  int requests = 0;
}

final class ResponseSpec {
  const ResponseSpec(
    this.status,
    this.data, {
    this.headers = const <String, List<String>>{},
  });

  final int status;
  final Object? data;
  final Map<String, List<String>> headers;
}

final class FakeClock {
  FakeClock(this.current);

  DateTime current;
  final List<Duration> delays = <Duration>[];

  DateTime now() => current;

  Future<void> delay(Duration duration) async {
    delays.add(duration);
    current = current.add(duration);
  }
}

final class StaticTokenProvider implements AuthTokenProvider {
  StaticTokenProvider(this.tokens);

  final AuthTokens tokens;

  @override
  Future<AuthTokens> validTokens({bool forceRefresh = false}) async => tokens;
}
