from pathlib import Path

p = Path("DAKit/packages/dakit_api/lib/src/http/official_api_client.dart")
s = p.read_text()

old = """    var refreshed = false;
    var serverRetries = 0;
    var rateLimitRetries = 0;

    while (true) {
"""
new = """    var refreshed = false;
    var serverRetries = 0;
    var rateLimitRetries = 0;
    var networkRetries = 0;

    while (true) {
"""
if old not in s:
    raise SystemExit("network retry counter anchor changed")
s = s.replace(old, new, 1)

old = """      } on DAKitException {
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
"""
new = """      } on DAKitException {
        rethrow;
      } on DioException catch (error) {
        final failure = _dioFailure(error);
        _record(
          DiagnosticLevel.error,
          failure.code,
          path,
          started,
          method: method,
          retry: networkRetries,
        );

        if (retryTransientResponses && failure.retryable) {
          if (cached != null && _cacheStaleUsable(cached)) {
            _record(
              DiagnosticLevel.warning,
              'api.cache.stale_network',
              path,
              started,
              method: method,
              retry: networkRetries,
            );
            return cached.value;
          }
          if (networkRetries < config.retryPolicy.maxRetries) {
            networkRetries += 1;
            await _delay(config.retryPolicy.delayFor(networkRetries));
            continue;
          }
        }
        throw failure;
      }
"""
if old not in s:
    raise SystemExit("Dio failure block changed")
s = s.replace(old, new, 1)

p.write_text(s)
print("Transient GET network errors now retry and may use stale cache.")
