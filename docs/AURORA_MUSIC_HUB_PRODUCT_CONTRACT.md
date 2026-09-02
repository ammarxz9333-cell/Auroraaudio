# Aurora Music Hub — Product Contract

Status: product requirement / implementation-gated by the Aurora execution roadmap.

## Goal

Aurora Music Hub is the user-facing music management layer that sits above Navidrome. Navidrome remains the catalog/streaming backend; Aurora Music Hub owns ingestion, uploads, URL sharing/import, metadata enrichment, remote administration, and the unified management UI used from phones, computers, and the Galaxy S6 touchscreen.

## Access surfaces

The same Aurora library must be manageable from:

1. Galaxy S6 local LVGL UI.
2. Responsive web UI/PWA from Android/iOS/desktop browsers on the LAN.
3. The same authenticated web UI remotely over the Internet through an explicitly enabled secure remote-access layer.
4. OpenSubsonic-compatible clients for normal browsing/playback.

All surfaces use the same users, library, playlists, favourites, play queue, ratings, lyrics, and import job state.

## Core services

- `aurora-music-hub`: upload/import API, metadata pipeline, job queue, authentication integration, library management API.
- `navidrome`: catalog, playback API, playlists, favourites, ratings, lyrics exposure, OpenSubsonic compatibility.
- `aurora-music-player`: local playback into Aurora Source Manager; Navidrome never owns the realtime audio output directly.
- `aurora-source-manager`: arbitrates HDMI/eARC, local music, Bluetooth, and network sources.
- `aurora-plugin-host`: optional link resolvers, remote services, lyrics providers, metadata providers, and legal media-fetch providers.

## Import methods

### 1. File upload

A user can share or upload one or many audio files from a phone or browser. Supported files are placed in a staging area first, never directly into the live library.

Required workflow:

`upload -> validate -> hash -> fingerprint -> identify -> enrich -> deduplicate -> organize -> commit -> rescan`

The original file must be preserved unless the user explicitly chooses transcoding or replacement.

### 2. Share-to-Aurora from another phone

The PWA/native share target accepts:

- audio files;
- one or more URLs;
- text containing URLs.

The request appears immediately in the Music Hub import queue with progress and any action required from the user.

### 3. URL import

Aurora distinguishes URL metadata resolution from media acquisition.

A URL resolver plugin may extract provider identity, title, artist, album/release information, artwork references, and canonical URLs.

A media-fetch plugin may download media only when the source provides a lawful downloadable media asset and the user has the necessary rights/permission. Aurora must not depend on bypassing DRM, subscription protections, or service restrictions.

For services such as Spotify or YouTube where a shared link is not itself a general-purpose downloadable audio asset, Aurora may still:

- resolve metadata;
- match the track against MusicBrainz/AcoustID or the existing local library;
- add the canonical service link;
- hand off playback to an official service integration/plugin when available;
- request a user-provided/local file when no lawful media-fetch path exists.

This separation keeps the product stable even when provider APIs or terms change.

### 4. Watch/inbox folder

Aurora may expose a controlled inbox directory for SFTP/WebDAV/local copy workflows. New files enter the same staging pipeline and are never trusted based only on filename tags.

## Metadata pipeline

Every imported audio file should be normalized with the following evidence order:

1. existing embedded metadata when internally consistent;
2. acoustic fingerprint using Chromaprint/AcoustID;
3. MusicBrainz recording/release match;
4. filename/path hints;
5. explicit user confirmation when confidence is below threshold.

Target fields include:

- track title;
- track artist(s);
- album artist;
- album/release;
- track/disc number;
- release date/year;
- genre(s);
- MusicBrainz IDs;
- AcoustID where available;
- cover art;
- codec/sample rate/bit depth;
- source/import provenance;
- duplicate fingerprint/hash.

Low-confidence matches must not silently overwrite the original tags.

## Library organization

Default managed library path:

`/data/music/<AlbumArtist>/<Album>/<Disc-Track> - <Title>.<ext>`

Exact path formatting is configurable. Navidrome remains tag-driven; filesystem layout is for maintainability, backup, and interoperability only.

## Albums, playlists, and collections

Aurora distinguishes real release metadata from user organization:

- **Album**: release metadata associated with the audio file.
- **Playlist**: ordered set of tracks, fully editable from S6, phone, or web.
- **Smart Playlist**: rule-based dynamic playlist where backend support permits.
- **Collection**: Aurora virtual grouping that can contain tracks/albums without rewriting their real album metadata.

A user may explicitly edit album tags to create a custom compilation, but the default UI should recommend a Collection or Playlist instead of corrupting source release metadata.

## Lyrics

Aurora must support:

- embedded lyrics;
- synchronized sidecars including TTML/ELRC/LRC/SRT/YAML where supported by the backend;
- unsynchronized text lyrics;
- optional lyrics-provider plugins;
- manual editing and replacement from the web UI;
- display on the S6 Now Playing screen;
- retention of multiple lyric variants/languages when possible.

Lyrics files should follow the final audio filename and be committed atomically with the track metadata.

## Artwork

Import may obtain artwork from embedded tags, approved metadata providers, or user uploads. The user can replace playlist/collection artwork from phone, S6, or web.

## Deduplication

Before library commit, Aurora checks at minimum:

- cryptographic file hash;
- acoustic fingerprint;
- MusicBrainz recording ID;
- artist/title/duration similarity.

Duplicate policy options:

- keep existing;
- replace existing;
- keep both editions;
- keep higher-quality file;
- manual decision.

No destructive replacement occurs without a recoverable history/backup path.

## Background scheduling on Galaxy S6

Import work is non-realtime and must not interfere with Atmos playback.

- fingerprinting, metadata lookup, artwork processing, transcoding, and bulk rescans run on background/A53-class scheduling;
- heavy jobs pause or throttle while the realtime movie pipeline is under load;
- no import job may obtain realtime priority;
- library rescans are debounced/batched rather than triggered per tiny metadata write.

## Remote access

Default remote-access posture is closed/local-only.

Supported deployment modes may include:

1. private VPN/overlay access such as WireGuard/Tailscale-class architecture;
2. explicitly enabled HTTPS reverse proxy with modern TLS and strong authentication.

Requirements:

- no unauthenticated admin endpoint;
- rate limiting for login/import endpoints;
- role-based permissions;
- revocable device sessions/tokens;
- audit log for uploads, deletes, metadata edits, and plugin actions;
- secrets never stored in plugin manifests or browser local storage in plaintext.

## User roles

Minimum roles:

- `admin`: system, users, plugins, import, delete, metadata edits;
- `library-manager`: upload/import/edit/playlist management;
- `listener`: browse/play/download where permitted;
- `guest`: optional restricted shared-library access.

## Plugin extension points

Music Hub plugin categories:

- `url_resolver`
- `metadata_provider`
- `lyrics_provider`
- `artwork_provider`
- `media_fetcher`
- `streaming_service_control`
- `remote_storage_import`

Plugins remain out-of-process, permission-scoped, versioned, and unable to touch STM32/amplifier hardware or the realtime callback directly.

## Required acceptance tests before production-ready status

1. Upload from Android browser and iPhone browser.
2. Upload from desktop browser.
3. Multi-file album import.
4. Share URL from a phone into Aurora import queue.
5. Metadata-only resolution for a provider link.
6. Lawful downloadable-source import using a test fixture/plugin.
7. Unknown file identified by AcoustID/MusicBrainz.
8. Wrong/ambiguous fingerprint requires user confirmation.
9. Duplicate import does not create an accidental duplicate.
10. Playlist create/edit/delete from S6, phone, and desktop.
11. Collection create/edit/delete from all three surfaces.
12. Lyrics import/display and synchronized timing.
13. Artwork replacement.
14. Interrupted upload resumes or fails cleanly without half-imported library entries.
15. Import service crash cannot interrupt active Aurora realtime audio.
16. Bulk import during movie playback stays within defined CPU/thermal budget or automatically throttles.
17. Remote login, logout, token revocation, and permission tests.
18. Backup/restore of music database, playlists, metadata, artwork, lyrics, and configuration.

Only after these tests and the physical S6 validation gates pass may Music Hub be marked production-ready in an Aurora release.
