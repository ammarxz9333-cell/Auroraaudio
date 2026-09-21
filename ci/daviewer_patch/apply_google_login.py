from pathlib import Path

home = Path("DAViewer/lib/features/home/home_screen.dart")
text = home.read_text()

old_import = "import 'home_providers.dart';\nimport 'update_banner.dart';"
new_import = "import 'home_providers.dart';\nimport 'oauth_home_fallback.dart';\nimport 'update_banner.dart';"
if old_import not in text:
    raise SystemExit("home import anchor changed")
text = text.replace(old_import, new_import, 1)

old_resume = """    if (shouldRefreshPersonalizedFeedOnResume(
      backgroundDuration: DateTime.now().difference(backgroundedAt),
      routeIsCurrent: routeIsCurrent,
      recommendedTabIsActive: recommendedTabIsActive,
      scrollOffset: scrollOffset,
    )) {
      unawaited(ref.read(personalizedFeedProvider.notifier).refreshSilently());
    }"""
new_resume = """    if (shouldRefreshPersonalizedFeedOnResume(
      backgroundDuration: DateTime.now().difference(backgroundedAt),
      routeIsCurrent: routeIsCurrent,
      recommendedTabIsActive: recommendedTabIsActive,
      scrollOffset: scrollOffset,
    )) {
      final webSignedIn =
          ref.read(webSessionControllerProvider).isLoggedIn == true;
      if (webSignedIn) {
        unawaited(
          ref.read(personalizedFeedProvider.notifier).refreshSilently(),
        );
      } else if (ref.read(authControllerProvider).oauthSignedIn) {
        unawaited(
          ref.read(oauthHomeFeedProvider.notifier).refreshSilently(),
        );
      }
    }"""
if old_resume not in text:
    raise SystemExit("resume anchor changed")
text = text.replace(old_resume, new_resume, 1)

old_build = """    final webSignedIn = ref.watch(
      webSessionControllerProvider.select((web) => web.isLoggedIn == true),
    );
    if (!webSignedIn) {
      return LoginPrompt(
        s: s,
        onLogin: () => context.push('/web-login'),
        message: s.recommendedSignInHint,
      );
    }
    final feed = ref.watch(personalizedFeedProvider);

    return ArtworkFeedGrid(
      scrollController: _scrollController,
      feed: feed,
      emptyMessage: s.noRecommendations,
      errorMessage: s.recommendedFeedLoadFailure,
      onRefresh: () => ref.read(personalizedFeedProvider.notifier).refresh(),
      onLoadMore: () => ref.read(personalizedFeedProvider.notifier).loadMore(),
    );"""
new_build = """    final webSignedIn = ref.watch(
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

    final feedProvider =
        webSignedIn ? personalizedFeedProvider : oauthHomeFeedProvider;
    final feed = ref.watch(feedProvider);

    return ArtworkFeedGrid(
      scrollController: _scrollController,
      feed: feed,
      emptyMessage: s.noRecommendations,
      errorMessage: s.recommendedFeedLoadFailure,
      onRefresh: () => ref.read(feedProvider.notifier).refresh(),
      onLoadMore: () => ref.read(feedProvider.notifier).loadMore(),
    );"""
if old_build not in text:
    raise SystemExit("personalized feed anchor changed")
text = text.replace(old_build, new_build, 1)

home.write_text(text)
