# Aurora Music Hub example user flow

This example is intentionally provider-agnostic and does not bypass DRM or service restrictions.

## From another phone

1. Open Aurora Music Hub PWA or use Android/iOS Share -> Aurora.
2. Choose either an audio file or paste/share a URL.
3. Aurora creates an import job and immediately shows status.
4. Files are staged and validated; links are resolved by provider plugins.
5. If audio is available through a lawful fetch path, Aurora imports it. Otherwise the link remains a metadata/control reference and Aurora can request a user-provided file.
6. Aurora fingerprints the audio, checks duplicates, matches MusicBrainz/AcoustID, proposes metadata/artwork/lyrics, and asks for confirmation only when confidence is low.
7. On commit, the track is moved into the managed library and Navidrome is rescanned.
8. The same item appears on the Galaxy S6 UI, phone clients, and desktop browser.

## Organizing music

From S6, phone, or desktop:

- create/edit/delete playlists;
- create Aurora Collections without rewriting real album metadata;
- edit track/release metadata when explicitly desired;
- replace artwork;
- add/edit synchronized or unsynchronized lyrics;
- star/rate tracks and albums;
- move tracks between playlists/collections;
- inspect import provenance and duplicate decisions.

## Realtime isolation

If a movie/immersive source is active, fingerprinting, transcoding, artwork processing, and bulk rescans are throttled or paused. Music Hub never receives realtime scheduling priority and cannot directly control STM32 or amplifiers.
