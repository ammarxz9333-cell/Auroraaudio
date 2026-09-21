from pathlib import Path

p = Path("DAKit/packages/dakit_api/lib/src/oauth/oauth_endpoint.dart")
s = p.read_text()

old = """      final invalidRefreshToken = _isInvalidRefreshTokenResponse(
        form: form,
        provider: provider,
        description: description,
      );
      final failure = DAKitException(
        kind: DAKitFailureKind.authentication,
        code: invalidRefreshToken
            ? 'oauth.refresh.invalid'
            : provider == null
            ? 'oauth.http.failed'
            : 'oauth.provider.$provider',
        message: description ?? 'The OAuth endpoint request failed.',
        retryable: (error.response?.statusCode ?? 0) >= 500,
        details: <String, Object?>{
          if (error.response?.statusCode case final int status)
            'status': status,
          'provider_error': ?provider,
          ...description == null
              ? const <String, Object?>{}
              : <String, Object?>{
                  'provider_description': _boundedDescription(description),
                },
        },
        cause: error,
      );
"""
new = """      final status = error.response?.statusCode ?? 0;
      // A throttled token endpoint must never be interpreted as a rejected
      // refresh token, even if the provider reuses an invalid_request body.
      final invalidRefreshToken =
          (status == 400 || status == 401) &&
          _isInvalidRefreshTokenResponse(
            form: form,
            provider: provider,
            description: description,
          );
      final kind = invalidRefreshToken
          ? DAKitFailureKind.authentication
          : status == 429
          ? DAKitFailureKind.rateLimit
          : status >= 500
          ? DAKitFailureKind.upstream
          : DAKitFailureKind.authentication;
      final code = invalidRefreshToken
          ? 'oauth.refresh.invalid'
          : status == 429
          ? 'oauth.http.rate_limited'
          : status >= 500
          ? 'oauth.http.$status'
          : provider == null
          ? 'oauth.http.failed'
          : 'oauth.provider.$provider';
      final failure = DAKitException(
        kind: kind,
        code: code,
        message: description ?? 'The OAuth endpoint request failed.',
        retryable: status == 429 || status >= 500,
        details: <String, Object?>{
          if (status != 0) 'status': status,
          'provider_error': ?provider,
          ...description == null
              ? const <String, Object?>{}
              : <String, Object?>{
                  'provider_description': _boundedDescription(description),
                },
          if (error.response?.headers.value('retry-after')
              case final String retryAfter)
            'retry_after': retryAfter,
        },
        cause: error,
      );
"""
if old not in s:
    raise SystemExit("OAuth failure classification anchor changed")
p.write_text(s.replace(old, new, 1))
