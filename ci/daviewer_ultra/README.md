# DAViewer Ultra

Personal Android reliability build of DAViewer, carried inside Auroraaudio only
as build/patch infrastructure.

Base source is pinned to upstream DAViewer v0.5.7 commit
`3b4d3af459c1dbd2f21ba686804027b9cbceda6e`.

Ultra changes:
- Android OAuth uses the system browser so Google/provider login is not blocked
  by embedded-WebView OAuth restrictions.
- OAuth success no longer waits forever for unrelated WebView cookies.
- Personalized website `rfy/deviations` remains preferred when a valid web
  session exists.
- If WebView Cookie/CSRF state is unavailable, anonymous, WAF-blocked, or the
  rfy request fails, Home falls back to official `browse/home`.
- If official Discover also fails on the first page, Daily Deviations is the
  final fallback.
- The upstream search fallback, downloads, favourites, watch, comments,
  gallery/collection support, session persistence, and mature-content handling
  from v0.5.7 are retained.
- CI must pass `flutter analyze` and the full upstream `flutter test` suite
  before producing an APK.

The CI artifact is release-mode but signed with the runner debug key. If Android
reports a signature mismatch with an older custom DAViewer install, uninstall
that old build before installing this artifact.
