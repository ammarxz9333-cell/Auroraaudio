from pathlib import Path

p = Path("DAKit/packages/dakit_api/lib/src/oauth/oauth_session.dart")
s = p.read_text()

old = """  int _generation = 0;
  bool _loggingOut = false;
"""
new = """  int _generation = 0;
  bool _loggingOut = false;
  String? _suspectRefreshToken;
  int _invalidRefreshConfirmations = 0;
"""
if old not in s:
    raise SystemExit("session state anchor changed")
s = s.replace(old, new, 1)

old = """    await _store.write(tokens);
    if (_loggingOut || expected != _generation) {
"""
new = """    await _store.write(tokens);
    _resetInvalidRefreshEvidence();
    if (_loggingOut || expected != _generation) {
"""
if old not in s:
    raise SystemExit("session save anchor changed")
s = s.replace(old, new, 1)

old = """    _generation += 1;
    _loggingOut = true;
    AuthTokens? current;
"""
new = """    _generation += 1;
    _loggingOut = true;
    _resetInvalidRefreshEvidence();
    AuthTokens? current;
"""
if old not in s:
    raise SystemExit("logout reset anchor changed")
s = s.replace(old, new, 1)

old = """  Future<AuthTokens> _performRefresh(
    AuthTokens current,
    int expectedGeneration,
  ) async {
    late final AuthTokens updated;
    try {
      updated = await _tokenClient.refresh(config: config, current: current);
    } on DAKitException catch (error) {
      if (error.code == 'oauth.refresh.invalid') {
        await _invalidateRefreshToken(error, expectedGeneration);
      }
      rethrow;
    }
    _ensureUsable(expectedGeneration);
    await _store.write(updated);
    if (_loggingOut || expectedGeneration != _generation) {
      await _store.clear();
      throw _changedSession();
    }
    return updated;
  }

"""
new = """  Future<AuthTokens> _performRefresh(
    AuthTokens current,
    int expectedGeneration,
  ) async {
    late final AuthTokens updated;
    try {
      updated = await _tokenClient.refresh(config: config, current: current);
    } on DAKitException catch (error) {
      if (error.code == 'oauth.refresh.invalid') {
        // A second engine/isolate may have already refreshed and persisted a
        // replacement token. Never delete that newer session because an older
        // in-flight refresh was rejected.
        AuthTokens? latest;
        try {
          latest = await _store.read();
        } on Object {
          latest = null;
        }
        final latestRefresh = latest?.refreshToken;
        if (latest != null &&
            latestRefresh != null &&
            latestRefresh.isNotEmpty &&
            latestRefresh != current.refreshToken) {
          _resetInvalidRefreshEvidence();
          return _performRefresh(latest, expectedGeneration);
        }

        if (_shouldInvalidateRefreshToken(error, current)) {
          await _invalidateRefreshToken(error, expectedGeneration);
        }
      } else {
        _resetInvalidRefreshEvidence();
      }
      rethrow;
    }
    _ensureUsable(expectedGeneration);
    await _store.write(updated);
    _resetInvalidRefreshEvidence();
    if (_loggingOut || expectedGeneration != _generation) {
      await _store.clear();
      throw _changedSession();
    }
    return updated;
  }

"""
if old not in s:
    raise SystemExit("refresh block anchor changed")
s = s.replace(old, new, 1)

anchor = """  Future<void> _invalidateRefreshToken(
"""
helpers = """  bool _shouldInvalidateRefreshToken(
    DAKitException error,
    AuthTokens current,
  ) {
    final status = error.details['status'];

    // Preserve the historical behavior for synthetic/custom endpoints that do
    // not expose an HTTP status. Real DeviantArt rejections include 400/401.
    if (status is! int) return true;

    // 429 and 5xx are transient service conditions, never credential proof.
    if (status != 400 && status != 401) {
      _resetInvalidRefreshEvidence();
      return false;
    }

    final refreshToken = current.refreshToken;
    if (refreshToken == null || refreshToken.isEmpty) return true;

    if (_suspectRefreshToken != refreshToken) {
      _suspectRefreshToken = refreshToken;
      _invalidRefreshConfirmations = 1;
      return false;
    }

    _invalidRefreshConfirmations += 1;
    return _invalidRefreshConfirmations >= 3;
  }

  void _resetInvalidRefreshEvidence() {
    _suspectRefreshToken = null;
    _invalidRefreshConfirmations = 0;
  }

"""
if anchor not in s:
    raise SystemExit("invalidation helper anchor changed")
s = s.replace(anchor, helpers + anchor, 1)

p.write_text(s)
