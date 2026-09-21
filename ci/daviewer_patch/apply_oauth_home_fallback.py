from pathlib import Path

p = Path("DAViewer/lib/features/home/home_providers.dart")
s = p.read_text()

old = """        var csrf = ref.read(webSessionControllerProvider).csrf;
        var cookieHeader = await webSession.cookieHeader();
        var page = await _tryFetchRfy(dio, csrf, cookieHeader, request);
        if (page == null) {
          // Stale session after a restart: re-read the CSRF from the persisted
          // cookies (headless page load) and retry once.
          await ref.read(webSessionRefresherProvider).refresh();
          csrf = ref.read(webSessionControllerProvider).csrf;
          cookieHeader = await webSession.cookieHeader();
          page = await _tryFetchRfy(dio, csrf, cookieHeader, request);
        }
        if (page == null) {
          throw const DAKitException(
            kind: DAKitFailureKind.authentication,
            code: 'web.session.unavailable',
            message: 'The personalized feed requires a signed-in web session.',
          );
        }
        ref.read(artworkStoreProvider.notifier).putAll(page.items);
        return page;"""
new = """        var csrf = ref.read(webSessionControllerProvider).csrf;
        var cookieHeader = await webSession.cookieHeader();
        var page = await _tryFetchRfy(dio, csrf, cookieHeader, request);
        if (page == null && (csrf.isNotEmpty || cookieHeader.isNotEmpty)) {
          // A real web session may simply be stale after a restart: refresh
          // that browser session once before falling back.
          await ref.read(webSessionRefresherProvider).refresh();
          csrf = ref.read(webSessionControllerProvider).csrf;
          cookieHeader = await webSession.cookieHeader();
          page = await _tryFetchRfy(dio, csrf, cookieHeader, request);
        }
        // Google/social OAuth completes in the system browser. Android cannot
        // copy Chrome's HttpOnly DeviantArt cookies into the app WebView, so a
        // valid OAuth account can legitimately exist without the website RFY
        // session. Keep Home functional via the official browse/home feed.
        page ??= await OfficialDiscoveryRepository(runtime.transport!).browse(
          request,
        );
        ref.read(artworkStoreProvider.notifier).putAll(page.items);
        return page;"""
if old not in s:
    raise SystemExit("home provider anchor changed")
s = s.replace(old, new, 1)
p.write_text(s)

p = Path("DAViewer/lib/features/home/home_screen.dart")
s = p.read_text()
old = """    final webSignedIn = ref.watch(
      webSessionControllerProvider.select((web) => web.isLoggedIn == true),
    );
    if (!webSignedIn) {
      return LoginPrompt(
        s: s,
        onLogin: () => context.push('/web-login'),
        message: s.recommendedSignInHint,
      );
    }
    final feed = ref.watch(personalizedFeedProvider);"""
new = """    final webSignedIn = ref.watch(
      webSessionControllerProvider.select((web) => web.isLoggedIn == true),
    );
    final oauthSignedIn = ref.watch(
      authControllerProvider.select((auth) => auth.oauthSignedIn),
    );
    if (!webSignedIn && !oauthSignedIn) {
      return LoginPrompt(
        s: s,
        onLogin: () => context.push('/web-login'),
        message: s.recommendedSignInHint,
      );
    }
    final feed = ref.watch(personalizedFeedProvider);"""
if old not in s:
    raise SystemExit("home screen anchor changed")
s = s.replace(old, new, 1)
p.write_text(s)
