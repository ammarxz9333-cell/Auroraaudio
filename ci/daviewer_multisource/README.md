# DAViewer Ultra MultiSource v0.8.0

This patch adds a MultiSource gallery search to existing DAViewer Ultra without
replacing DeviantArt functionality.

## Implemented API adapters
- Danbooru: API with general-only query and result-side rating validation.
- Safebooru: image search through publicly documented DAPI.
- Wallhaven: SFW-only search with per-item purity enforcement.
- Openverse: openly licensed images and mature filter.
- Rule34: separate adult-art source, disabled until explicit adult confirmation.
  It is never included in All Sources.
- Reddit: still-image and multi-image gallery parser, only over OAuth Reddit
  Data API if the application has approved access. Excludes NSFW, spoilers,
  videos, GIFs, text posts and unrelated external websites.

## External-only searches
Pixiv, ArtStation, Behance, Zerochan and Gelbooru use browser links while
their in-app authorization/API terms are evaluated. No unauthorized scraping.
Do not claim all images or all art websites are reachable.

## Reddit API and retention
Reddit explicitly requires developer approval before API access. No client
secret or access token is committed to the APK or repository.

An approved developer can currently enter a temporary OAuth access token and
their truthful registered User-Agent via the MultiSource screen key icon.
Stored via Flutter Secure Storage on the device; authorization/refresh is
not yet automatic. Expired tokens require reauthorization. Image requests use
Image.network rather than a persistent disk-image cache. No persistent
Reddit post database is created.

Reddit API search uses the official OAuth search listing and cursor-based
pagination. Rate limits, deletions and commercial-use restrictions must
be respected.

Official sources:
https://support.reddithelp.com/hc/en-us/articles/14945211791892-Developer-Platform-Accessing-Reddit-Data
https://support.reddithelp.com/hc/en-us/articles/16160319875092-Reddit-Data-API-Wiki
https://support.reddithelp.com/hc/en-us/articles/42728983564564-Responsible-Builder-Policy
https://www.reddit.com/dev/api/

## Build
The GitHub Actions workflow build-daviewer-multisource-v080.yml:
- clones the verified DAViewer upstream version v0.7.0;
- installs the prior local Ultra v0.7.2 fixes;
- adds the new search screen, API parsers and tests;
- validates format, Flutter analysis and the complete test suite;
- builds a release-mode APK and packages SHA-256 checksums.

Signing is temporary CI debug signing, which cannot update APK installations
signed with a different key. Back up application data before migration.

CI results and real-device functionality are not claimed until independently
verified. Reddit availability depends on developer approval.
