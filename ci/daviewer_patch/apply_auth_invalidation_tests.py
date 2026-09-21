from pathlib import Path

p = Path("DAViewer/test/auth_restore_policy_test.dart")
s = p.read_text()

old = """  test('only missing or revoked credentials require authorization', () {
    expect(
      shouldPreserveSessionAfterRestoreFailure(
        const DAKitException(
          kind: DAKitFailureKind.authentication,
          code: 'oauth.session.missing',
          message: 'missing',
        ),
      ),
      isFalse,
    );
    expect(
      shouldPreserveSessionAfterRestoreFailure(
        const DAKitException(
          kind: DAKitFailureKind.authentication,
          code: 'oauth.error.invalid_grant',
          message: 'revoked',
        ),
      ),
      isFalse,
    );
    expect(
      shouldPreserveSessionAfterRestoreFailure(
        const DAKitException(
          kind: DAKitFailureKind.authentication,
          code: 'oauth.refresh.invalid',
          message: 'refresh token invalid',
        ),
      ),
      isFalse,
    );
  });

  test('legacy DeviantArt invalid_request refresh response is definitive', () {
    expect(
      shouldPreserveSessionAfterRestoreFailure(
        const DAKitException(
          kind: DAKitFailureKind.authentication,
          code: 'oauth.provider.invalid_request',
          message: 'The refresh_token is invalid.',
          details: <String, Object?>{
            'provider_description': 'The refresh_token is invalid.',
          },
        ),
      ),
      isFalse,
    );
    expect(
      shouldPreserveSessionAfterRestoreFailure(
        const DAKitException(
          kind: DAKitFailureKind.authentication,
          code: 'oauth.provider.invalid_request',
          message: 'The authorization code is invalid.',
        ),
      ),
      isTrue,
    );
  });
"""
new = """  test('only actually missing credentials force authorization immediately', () {
    expect(
      shouldPreserveSessionAfterRestoreFailure(
        const DAKitException(
          kind: DAKitFailureKind.authentication,
          code: 'oauth.session.missing',
          message: 'missing',
        ),
      ),
      isFalse,
    );
    expect(
      shouldPreserveSessionAfterRestoreFailure(
        const DAKitException(
          kind: DAKitFailureKind.authentication,
          code: 'oauth.refresh.missing',
          message: 'missing refresh token',
        ),
      ),
      isFalse,
    );
  });

  test('refresh rejection waits for OAuthSession confirmation', () {
    expect(
      shouldPreserveSessionAfterRestoreFailure(
        const DAKitException(
          kind: DAKitFailureKind.authentication,
          code: 'oauth.refresh.invalid',
          message: 'refresh token invalid',
        ),
      ),
      isTrue,
    );
    expect(
      shouldPreserveSessionAfterRestoreFailure(
        const DAKitException(
          kind: DAKitFailureKind.authentication,
          code: 'oauth.provider.invalid_request',
          message: 'The refresh_token is invalid.',
          details: <String, Object?>{
            'provider_description': 'The refresh_token is invalid.',
          },
        ),
      ),
      isTrue,
    );
    expect(
      shouldPreserveSessionAfterRestoreFailure(
        const DAKitException(
          kind: DAKitFailureKind.authentication,
          code: 'oauth.error.invalid_grant',
          message: 'provider rejected refresh',
        ),
      ),
      isTrue,
    );
  });
"""
if old not in s:
    raise SystemExit("auth restore policy test anchor changed")
p.write_text(s.replace(old, new, 1))
print("Auth restore policy expectations updated for confirmed invalidation.")
