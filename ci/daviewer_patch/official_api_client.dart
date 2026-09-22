import 'dart:async';

import 'package:dakit_core/dakit_core.dart';
import 'package:dio/dio.dart';

import 'api_config.dart';
import 'network_adapter.dart';
import 'network_profile.dart';

typedef Delay = Future<void> Function(Duration duration);
typedef Clock = DateTime Function();

abstract interface class OfficialApiTransport {
  Future<Map<String, Object?>> getJson(
    String path, {
    Map<String, Object?> query = const <String, Object?>{},
    CancelToken? cancelToken,
  });
}

abstract interface class OfficialApiMutationTransport
    implements OfficialApiTransport {
  Future<Map<String, Object?>> postFormJson(
    String path, {
    Map<String, Object?> form = const <String, Object?>{},
    CancelToken? cancelToken,
  });
}

final class OfficialApiClient implements OfficialApiMutationTransport {
  factory OfficialApiClient({
    required AuthTokenProvider session,
    Dio? dio,
    NetworkProfile? networkProfile,
    ApiConfig? config,
    DiagnosticSink diagnostics = const NoopDiagnosticSink(),
    Delay? delay,
    Clock? now,
  }) {
    final resolvedConfig = config ?? ApiConfig();
    final resolvedDelay = delay ?? Future<void>.delayed;
    final resolvedNow = now ?? DateTime.now;
    final gate = delay != null || now != null
        ? _RateLimitGate(delay: resolvedDelay, now: resolvedNow)
        : _sharedRateGates.putIfAbsent(
            resolvedConfig.baseUri.origin,
            () => _RateLimitGate(delay: resolvedDelay, now: resolvedNow),
          );
    return OfficialApiClient._(
      session,
      _resolveDio(dio, networkProfile),
      resolvedConfig,
      diagnostics,
      resolvedDelay,
      resolvedNow,
      gate,
    );
  }

  static final Map<String, _RateLimitGate> _sharedRateGates =
      <String, _RateLimitGate>{};

  static Dio _resolveDio(Dio? dio, NetworkProfile? profile) {
    if (dio != null && profile != null) {
      throw const DAKitException(
        kind: DAKitFailureKind.configuration,
        code: 'network.transport.ambiguous',
        message: 'Provide either a Dio client or a network profile, not both.',
      );
    }
    return dio ??
        createNetworkDio(profile: profile ?? NetworkProfile.environment());
  }

  OfficialApiClient._(
    this._session,
    this._dio,
    this.config,
    this._diagnostics,
    this._delay,
    this._now,
    this._rateGate,
  ) {
    _dio.options.connectTimeout = config.connectTimeout;
    _dio.options.receiveTimeout = config.receiveTimeout;
  }

  final AuthTokenProvider _session;
  final Dio _dio;
  final DiagnosticSink _diagnostics;
  final Delay _delay;
  final Clock _now;
  final _RateLimitGate _rateGate;
  final Map<String, Future<Map<String, Object?>>> _inFlightGets =
      <String, Future<Map<String, Object?>>>{};
  final ApiConfig config;

  @override
  Future<Map<String, Object?>> getJson(
    String path, {
    Map<String, Object?> query = const <String, Object?>{},
    CancelToken? cancelToken,
  }) {
    if (cancelToken != null) {
      return _requestJson(
        path,
        method: 'GET',
        query: query,
        cancelToken: cancelToken,
        retryTransientResponses: true,
      );
    }

    final key = _readRequestKey(path, query);
    final existing = _inFlightGets[key];
    if (existing != null) return existing;

    final operation = _requestJson(
      path,
      method: 'GET',
      query: query,
      retryTransientResponses: true,
    );
    _inFlightGets[key] = operation;
    operation.then<void>(
      (_) => _removeInFlight(key, operation),
      onError: (Object _, StackTrace __) => _removeInFlight(key, operation),
    );
    return operation;
  }

  void _removeInFlight(
    String key,
    Future<Map<String, Object?>> operation,
  ) {
    if (identical(_inFlightGets[key], operation)) _inFlightGets.remove(key);
  }

  static String _readRequestKey(
    String path,
    Map<String, Object?> query,
  ) {
    final keys = query.keys.toList(growable: false)..sort();
    final suffix = keys.map((key) => '$key=${query[key]}').join('&');
    return suffix.isEmpty ? path : '$path?$suffix';
  }

  @override
  Future<Map<String, Object?>> postFormJson(
    String path, {
    Map<String, Object?> form = const <String, Object?>{},
    CancelToken? cancelToken,
  }) => _requestJson(
    path,
    method: 'POST',
    form: form,
    cancelToken: cancelToken,
    retryTransientResponses: false,
  );

  Future<Map<String, Object?>> _requestJson(
    String path, {
    required String method,
    required bool retryTransientResponses,
    Map<String, Object?> query = const <String, Object?>{},
    Map<String, Object?>? form,
    CancelToken? cancelToken,
  }) async {
    final uri = _resolve(path);
    var tokens = await _session.validTokens();
    var refreshed = false;
    var serverRetries = 0;
    var rateLimitRetries = 0;

    while (true) {
      final started = _now();
      Duration? rateLimitWait;
      try {
        final response = await _rateGate.schedule(
          config.rateLimitPolicy.minimumSpacing,
          () async {
            final result = await _dio.request<Object?>(
              uri.toString(),
              queryParameters: query,
              data: form,
              cancelToken: cancelToken,
              options: Options(
                method: method,
                headers: <String, Object?>{
                  Headers.acceptHeader: Headers.jsonContentType,
                  'Accept-Encoding': 'gzip',
                  'User-Agent': config.userAgent,
                  'Authorization': '${tokens.tokenType} ${tokens.accessToken}',
                  'dA-minor-version': config.minorVersion.toString(),
                },
                contentType: form == null
                    ? null
                    : Headers.formUrlEncodedContentType,
                responseType: ResponseType.json,
                sendTimeout: config.connectTimeout,
                receiveTimeout: config.receiveTimeout,
                validateStatus: (_) => true,
              ),
            );
            if (result.statusCode == 429) {
              rateLimitWait = _rateLimitDelay(result, rateLimitRetries + 1);
              _rateGate.blockFor(rateLimitWait!);
            }
            return result;
          },
        );
        final status = response.statusCode ?? 0;
        _record(
          status < 400 ? DiagnosticLevel.info : DiagnosticLevel.warning,
          'api.response',
          path,
          started,
          method: method,
          status: status,
          retry: serverRetries + rateLimitRetries,
          rateLimitWait: rateLimitWait,
        );

        if (status == 401 && !refreshed) {
          tokens = await _session.validTokens(forceRefresh: true);
          refreshed = true;
          continue;
        }

        if (status == 429) {
          rateLimitRetries += 1;
          if (retryTransientResponses &&
              rateLimitRetries <= config.rateLimitPolicy.maxRetries) {
            continue;
          }
          throw _responseFailure(response);
        }

        if (retryTransientResponses &&
            _retryableServerStatus(status) &&
            serverRetries < config.retryPolicy.maxRetries) {
          serverRetries += 1;
          await _delay(config.retryPolicy.delayFor(serverRetries));
          continue;
        }
        if (status >= 400) throw _responseFailure(response);

        final data = response.data;
        if (data is! Map<String, Object?>) {
          throw const DAKitException(
            kind: DAKitFailureKind.parsing,
            code: 'api.response.invalid_json',
            message: 'The official API returned an unexpected response body.',
          );
        }
        return data;
      } on DAKitException {
        rethrow;
      } on DioException catch (error) {
        final failure = _dioFailure(error);
        _record(
          DiagnosticLevel.error,
          failure.code,
          path,
          started,
          method: method,
        );
        throw failure;
      }
    }
  }

  Uri _resolve(String path) {
    final candidate = Uri.parse(path);
    if (candidate.hasScheme ||
        candidate.hasAuthority ||
        candidate.hasQuery ||
        candidate.hasFragment ||
        path.startsWith('/')) {
      throw const DAKitException(
        kind: DAKitFailureKind.configuration,
        code: 'api.path.invalid',
        message: 'API paths must be relative and contain no query or fragment.',
      );
    }
    return config.baseUri.resolve(path);
  }

  static bool _retryableServerStatus(int status) =>
      status == 500 || status == 503;

  Duration _rateLimitDelay(Response<Object?> response, int retry) {
    final fallback = config.rateLimitPolicy.delayFor(retry);
    final server = _parseRetryAfter(
      response.headers.value('retry-after'),
      _now().toUtc(),
    );
    if (server == null || server.compareTo(fallback) < 0) return fallback;
    return server;
  }

  static Duration? _parseRetryAfter(String? raw, DateTime now) {
    final value = raw?.trim();
    if (value == null || value.isEmpty) return null;

    final seconds = int.tryParse(value);
    if (seconds != null) {
      return Duration(seconds: seconds < 0 ? 0 : seconds);
    }

    final match = RegExp(
      r'^[A-Za-z]{3},\s+(\d{1,2})\s+([A-Za-z]{3})\s+(\d{4})\s+'
      r'(\d{2}):(\d{2}):(\d{2})\s+GMT$',
    ).firstMatch(value);
    if (match == null) return null;
    const months = <String, int>{
      'Jan': 1,
      'Feb': 2,
      'Mar': 3,
      'Apr': 4,
      'May': 5,
      'Jun': 6,
      'Jul': 7,
      'Aug': 8,
      'Sep': 9,
      'Oct': 10,
      'Nov': 11,
      'Dec': 12,
    };
    final month = months[match.group(2)];
    if (month == null) return null;
    final parsed = DateTime.utc(
      int.parse(match.group(3)!),
      month,
      int.parse(match.group(1)!),
      int.parse(match.group(4)!),
      int.parse(match.group(5)!),
      int.parse(match.group(6)!),
    );
    final difference = parsed.difference(now);
    return difference.isNegative ? Duration.zero : difference;
  }

  DAKitException _responseFailure(Response<Object?> response) {
    final status = response.statusCode ?? 0;
    final data = response.data;
    if (status == 403 && data is String && data.trimLeft().startsWith('<')) {
      return const DAKitException(
        kind: DAKitFailureKind.upstream,
        code: 'api.response.html_403',
        message: 'The service rejected the HTTP client before API processing.',
      );
    }
    final body = data is Map<String, Object?>
        ? data
        : const <String, Object?>{};
    final rawProviderCode = body['error'];
    final providerCode = rawProviderCode is String ? rawProviderCode : null;
    final rawDescription = body['error_description'];
    final description = rawDescription is String ? rawDescription : null;
    final kind = switch (status) {
      401 => DAKitFailureKind.authentication,
      403 => DAKitFailureKind.authorization,
      404 => DAKitFailureKind.notFound,
      429 => DAKitFailureKind.rateLimit,
      >= 500 => DAKitFailureKind.upstream,
      _ => DAKitFailureKind.upstream,
    };
    return DAKitException(
      kind: kind,
      code: providerCode == null
          ? 'api.http.$status'
          : 'api.provider.$providerCode',
      message: description ?? 'The official API request failed.',
      retryable: status == 429 || _retryableServerStatus(status),
      details: <String, Object?>{
        'status': status,
        if (body['error_code'] case final Object code) 'provider_code': code,
        if (description != null)
          'provider_description': _boundedDescription(description),
        if (response.headers.value('retry-after') case final String retryAfter)
          'retry_after': retryAfter,
      },
    );
  }

  static DAKitException _dioFailure(DioException error) {
    if (error.type == DioExceptionType.cancel) {
      return DAKitException(
        kind: DAKitFailureKind.cancelled,
        code: 'api.request.cancelled',
        message: 'The API request was cancelled.',
        cause: error,
      );
    }
    return DAKitException(
      kind: DAKitFailureKind.network,
      code: switch (error.type) {
        DioExceptionType.connectionTimeout => 'network.connect_timeout',
        DioExceptionType.sendTimeout => 'network.send_timeout',
        DioExceptionType.receiveTimeout => 'network.receive_timeout',
        DioExceptionType.badCertificate => 'network.tls_certificate',
        DioExceptionType.connectionError => 'network.connection',
        _ => 'network.request_failed',
      },
      message: 'The API request could not reach the service.',
      retryable: error.type != DioExceptionType.badCertificate,
      cause: error,
    );
  }

  static String _boundedDescription(String value) {
    final normalized = value.replaceAll(RegExp(r'\s+'), ' ').trim();
    return normalized.length <= 240
        ? normalized
        : '${normalized.substring(0, 240)}…';
  }

  void _record(
    DiagnosticLevel level,
    String code,
    String path,
    DateTime started, {
    required String method,
    int? status,
    int? retry,
    Duration? rateLimitWait,
  }) {
    _diagnostics.add(
      DiagnosticEvent(
        stage: DiagnosticStage.http,
        level: level,
        code: code,
        message: 'Official API request event.',
        elapsed: _now().difference(started),
        attributes: <String, Object?>{
          'path': path,
          'method': method,
          ...status == null
              ? const <String, Object?>{}
              : <String, Object?>{'status': status},
          ...retry == null
              ? const <String, Object?>{}
              : <String, Object?>{'retry': retry},
          ...rateLimitWait == null
              ? const <String, Object?>{}
              : <String, Object?>{
                  'rate_limit_wait_ms': rateLimitWait.inMilliseconds,
                },
        },
      ),
    );
  }
}

final class _RateLimitGate {
  _RateLimitGate({required Delay delay, required Clock now})
    : _delay = delay,
      _now = now;

  final Delay _delay;
  final Clock _now;
  Future<void> _tail = Future<void>.value();
  DateTime? _blockedUntil;
  DateTime? _nextAllowed;

  Future<T> schedule<T>(
    Duration minimumSpacing,
    Future<T> Function() operation,
  ) {
    final previous = _tail;
    final release = Completer<void>();
    _tail = release.future;

    return () async {
      await previous;
      try {
        final current = _now().toUtc();
        final target = _later(_blockedUntil, _nextAllowed);
        if (target != null && target.isAfter(current)) {
          final remaining = target.difference(current);
          // Policies are millisecond-granular. Round up so scheduling overhead
          // between blockFor() and the next attempt never shortens the backoff.
          final waitMillis =
              (remaining.inMicroseconds + Duration.microsecondsPerMillisecond - 1) ~/
              Duration.microsecondsPerMillisecond;
          await _delay(Duration(milliseconds: waitMillis));
        }
        final started = _now().toUtc();
        _nextAllowed = started.add(minimumSpacing);
        return await operation();
      } finally {
        if (!release.isCompleted) release.complete();
      }
    }();
  }

  void blockFor(Duration duration) {
    if (duration.inMicroseconds <= 0) return;
    final candidate = _now().toUtc().add(duration);
    if (_blockedUntil == null || candidate.isAfter(_blockedUntil!)) {
      _blockedUntil = candidate;
    }
  }

  static DateTime? _later(DateTime? first, DateTime? second) {
    if (first == null) return second;
    if (second == null) return first;
    return first.isAfter(second) ? first : second;
  }
}
