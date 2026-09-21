import 'package:daviewer/features/web_login/web_login_screen.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('routes Google OAuth out of the embedded WebView', () {
    expect(
      shouldOpenIdentityProviderExternally(
        Uri.parse('https://accounts.google.com/o/oauth2/v2/auth'),
      ),
      isTrue,
    );
    expect(
      shouldOpenIdentityProviderExternally(
        Uri.parse('https://sub.accounts.google.com/example'),
      ),
      isTrue,
    );
  });

  test('keeps DeviantArt pages in the embedded WebView', () {
    expect(
      shouldOpenIdentityProviderExternally(
        Uri.parse('https://www.deviantart.com/users/login'),
      ),
      isFalse,
    );
    expect(
      shouldOpenIdentityProviderExternally(
        Uri.parse('dakit://oauth/callback?code=x&state=y'),
      ),
      isFalse,
    );
  });

  test('routes other social identity providers externally too', () {
    expect(
      shouldOpenIdentityProviderExternally(
        Uri.parse('https://appleid.apple.com/auth/authorize'),
      ),
      isTrue,
    );
    expect(
      shouldOpenIdentityProviderExternally(
        Uri.parse('https://www.facebook.com/v20.0/dialog/oauth'),
      ),
      isTrue,
    );
  });
}
