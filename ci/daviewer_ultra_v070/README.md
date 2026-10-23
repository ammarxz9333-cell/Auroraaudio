# DAViewer Ultra v0.7.1 (Android)

This directory contains a **new upgrade track**, leaving the original
`ci/daviewer_ultra` patch infrastructure and v0.5.7-based build unchanged.

## Exact upstream

- Upstream project: https://github.com/redtidev1918/DAViewer
- Release: **v0.7.0** (2026-10-17)
- Verified release commit: `6ec59f574cb73819ee0e8889c7ad14ccfa55b613`
- Patch version: **0.7.1+701**
- Workflow: `.github/workflows/build-daviewer-ultra-v070.yml`

The workflow clones upstream at that immutable commit, applies the patches,
runs Dart formatting, Flutter static analysis, the full upstream Flutter test
suite, and attempts a release-mode APK build. Every source anchor is checked.
A source incompatibility or test failure blocks the APK artifact.

## Improvements

- Migrates all native features from upstream 0.7.0, including content filters,
  favourites, watched feed, downloads, artist galleries and richer diagnostics.
- Uses the **external system browser** for Android OAuth provider sign-in (with
  the existing app-links callback path). Desktop sign-in remains unmodified.
- Avoids waiting for unrelated WebView cookies before closing a successful
  Android OAuth screen. Personalized RFY still needs its own website session,
  and the app's existing web-session recovery/UI remains in place.
- Daily Deviations: archives earlier dates using the date parameter and
  continued day cursors; no invented offset API, no arbitrary page count.
  Historical traversal stops at year 2000.
- Search: an empty page with an actual continuation cursor stays paginatable
  instead of being incorrectly treated as end-of-results.
- Masonry grid: provides a manual load-more control even when geometry or a
  no-growth page prevents a normal near-bottom scroll trigger.
- Adds regression tests for month/year/leap-year daily continuation.

## Limits and verification

There is **no claim of unlimited DeviantArt server access**. The code removes
avoidable UI pagination dead-ends; it does not circumvent server rate limits,
private/subscriber-only content, expired sessions or copyright restrictions.
The production behavior of private website endpoints requires real authenticated
device testing in addition to CI tests.

The new Android APK is signed with a temporary CI debug key unless release
signing secrets are configured. Its signature is generally not compatible with
a previously installed upstream DAViewer or earlier Ultra APK. Back up local
data and avoid uninstalling older builds without considering data loss.

The original v0.5.7-specific `apply_ultra.py` and related patch scripts are
intentionally NOT run against upstream v0.7.0; their source anchors and
assumptions no longer match.
