from pathlib import Path

p = Path("DAViewer/lib/core/auth/auth_controller.dart")
s = p.read_text()

old = """bool isDefinitiveCredentialFailure(DAKitException error) {
  if (error.code == 'oauth.session.missing' ||
      error.code == 'oauth.refresh.missing' ||
      error.code == 'oauth.refresh.invalid' ||
      error.code.contains('invalid_grant')) {
    return true;
  }
"""
new = """bool isDefinitiveCredentialFailure(DAKitException error) {
  // A refresh rejection is not authoritative at the UI layer. OAuthSession
  // confirms repeated 400/401 invalid-refresh replies and emits session
  // invalidations only when the persisted session is truly dead. Treating the
  // first oauth.refresh.invalid here as final made the UI sign out while the
  // SDK deliberately kept the token for confirmation.
  if (error.code == 'oauth.session.missing' ||
      error.code == 'oauth.refresh.missing') {
    return true;
  }
"""
if old not in s:
    raise SystemExit("definitive credential block changed")
s = s.replace(old, new, 1)

old = """  if (error.kind != DAKitFailureKind.authentication ||
      error.code != 'oauth.provider.invalid_request') {
    return false;
  }
  final description =
      '${error.details['provider_description'] ?? error.message}'
          .replaceAll('_', ' ')
          .replaceAll(RegExp(r'\s+'), ' ')
          .toLowerCase();
  return description.contains('refresh token') &&
      (description.contains('invalid') || description.contains('revoked'));
}
"""
new = """  return false;
}
"""
if old not in s:
    raise SystemExit("legacy invalid_request heuristic changed")
s = s.replace(old, new, 1)

p.write_text(s)

final_text = p.read_text()
segment = final_text.split(
    "bool isDefinitiveCredentialFailure", 1
)[1].split("/// Owns the app", 1)[0]
if "error.code == 'oauth.refresh.invalid'" in segment:
    raise SystemExit("app layer still treats refresh.invalid as definitive")
if "error.code.contains('invalid_grant')" in segment:
    raise SystemExit("app layer still treats invalid_grant as definitive")
print("Auth UI now waits for OAuthSession invalidation confirmation.")
