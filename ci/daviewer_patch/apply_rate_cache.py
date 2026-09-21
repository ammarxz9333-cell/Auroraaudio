from pathlib import Path

p = Path("DAKit/packages/dakit_api/lib/src/http/official_api_client.dart")
s = p.read_text()

old = """  final Map<String, Future<Map<String, Object?>>> _inFlightGets =
      <String, Future<Map<String, Object?>>>{};
  final ApiConfig config;
"""
new = """  final Map<String, Future<Map<String, Object?>>> _inFlightGets =
      <String, Future<Map<String, Object?>>>{};
  final Map<String, _CachedJson> _readCache = <String, _CachedJson>{};
  String? _cacheAccessToken;
  final ApiConfig config;
"""
if old not in s:
    raise SystemExit("cache field anchor changed")
s = s.replace(old, new, 1)

old = """    final uri = _resolve(path);
    var tokens = await _session.validTokens();
    var refreshed = false;
    var serverRetries = 0;
    var rateLimitRetries = 0;

    while (true) {
"""
new = """    final uri = _resolve(path);
    var tokens = await _session.validTokens();
    _syncCacheIdentity(tokens.accessToken);
    final readKey = method == 'GET' ? _readRequestKey(path, query) : null;
    var cached = readKey == null ? null : _readCache[readKey];
    if (cached != null && _cacheFresh(cached)) {
      return cached.value;
    }
    var refreshed = false;
    var serverRetries = 0;
    var rateLimitRetries = 0;

    while (true) {
"""
if old not in s:
    raise SystemExit("request start anchor changed")
s = s.replace(old, new, 1)

old = """        if (status == 401 && !refreshed) {
          tokens = await _session.validTokens(forceRefresh: true);
          refreshed = true;
          continue;
        }
"""
new = """        if (status == 401 && !refreshed) {
          tokens = await _session.validTokens(forceRefresh: true);
          _syncCacheIdentity(tokens.accessToken);
          cached = readKey == null ? null : _readCache[readKey];
          refreshed = true;
          continue;
        }
"""
if old not in s:
    raise SystemExit("401 anchor changed")
s = s.replace(old, new, 1)

old = """        if (status == 429) {
          rateLimitRetries += 1;
          if (retryTransientResponses &&
              rateLimitRetries <= config.rateLimitPolicy.maxRetries) {
            continue;
          }
          throw _responseFailure(response);
        }
"""
new = """        if (status == 429) {
          rateLimitRetries += 1;
          if (cached != null && _cacheStaleUsable(cached)) {
            _record(
              DiagnosticLevel.warning,
              'api.cache.stale_rate_limit',
              path,
              started,
              method: method,
              status: status,
              retry: serverRetries + rateLimitRetries,
              rateLimitWait: rateLimitWait,
            );
            return cached.value;
          }
          if (retryTransientResponses &&
              rateLimitRetries <= config.rateLimitPolicy.maxRetries) {
            continue;
          }
          throw _responseFailure(response);
        }
"""
if old not in s:
    raise SystemExit("429 anchor changed")
s = s.replace(old, new, 1)

old = """        if (retryTransientResponses &&
            _retryableServerStatus(status) &&
            serverRetries < config.retryPolicy.maxRetries) {
          serverRetries += 1;
          await _delay(config.retryPolicy.delayFor(serverRetries));
          continue;
        }
"""
new = """        if (retryTransientResponses && _retryableServerStatus(status)) {
          if (cached != null && _cacheStaleUsable(cached)) {
            _record(
              DiagnosticLevel.warning,
              'api.cache.stale_upstream',
              path,
              started,
              method: method,
              status: status,
              retry: serverRetries,
            );
            return cached.value;
          }
          if (serverRetries < config.retryPolicy.maxRetries) {
            serverRetries += 1;
            await _delay(config.retryPolicy.delayFor(serverRetries));
            continue;
          }
        }
"""
if old not in s:
    raise SystemExit("server retry anchor changed")
s = s.replace(old, new, 1)

old = """        if (data is! Map<String, Object?>) {
          throw const DAKitException(
            kind: DAKitFailureKind.parsing,
            code: 'api.response.invalid_json',
            message: 'The official API returned an unexpected response body.',
          );
        }
        return data;
"""
new = """        if (data is! Map<String, Object?>) {
          throw const DAKitException(
            kind: DAKitFailureKind.parsing,
            code: 'api.response.invalid_json',
            message: 'The official API returned an unexpected response body.',
          );
        }
        if (readKey != null) {
          final value = Map<String, Object?>.unmodifiable(data);
          _readCache[readKey] = _CachedJson(value, _now().toUtc());
          return value;
        }
        return data;
"""
if old not in s:
    raise SystemExit("success cache anchor changed")
s = s.replace(old, new, 1)

anchor = """  Uri _resolve(String path) {
"""
helpers = """  void _syncCacheIdentity(String accessToken) {
    if (_cacheAccessToken == accessToken) return;
    _cacheAccessToken = accessToken;
    _readCache.clear();
  }

  bool _cacheFresh(_CachedJson cached) =>
      _cacheAge(cached) <= config.rateLimitPolicy.freshCacheTtl;

  bool _cacheStaleUsable(_CachedJson cached) =>
      _cacheAge(cached) <= config.rateLimitPolicy.staleCacheTtl;

  Duration _cacheAge(_CachedJson cached) {
    final age = _now().toUtc().difference(cached.storedAt);
    return age.isNegative ? Duration.zero : age;
  }

"""
if anchor not in s:
    raise SystemExit("helper insertion anchor changed")
s = s.replace(anchor, helpers + anchor, 1)

anchor = """final class _RateLimitGate {
"""
cache_class = """final class _CachedJson {
  const _CachedJson(this.value, this.storedAt);

  final Map<String, Object?> value;
  final DateTime storedAt;
}

"""
if anchor not in s:
    raise SystemExit("cache class anchor changed")
s = s.replace(anchor, cache_class + anchor, 1)

p.write_text(s)
