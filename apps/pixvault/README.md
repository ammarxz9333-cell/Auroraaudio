# PixVault 0.3

PixVault is an Android Flutter media browser maintained inside the Auroraaudio repository.

## What changed from the prototype

- The application is now a real source tree under `apps/pixvault/` instead of one generated CI file.
- Domain model, safety rules, repositories, local storage/downloads and UI pages are separated.
- The verified source behaviour from v0.2.2 is preserved.
- The gallery grid is responsive instead of being hard-coded to two columns.
- CI runs formatting, static analysis, tests, source API smoke probes and builds installable APKs.
- The hard safety block for minor-indicating terms remains non-disableable.

## Sources represented by the data layer

Pixiv, Rule34Vault/XYZ, Gelbooru, Danbooru, yande.re and Konachan are implemented in the repository layer. The UI only exposes sources that are sufficiently reliable in the current build; Pixiv requires an authenticated web session.

## Build

The GitHub Actions workflow `.github/workflows/build-pixvault-v030.yml` creates an Android shell, injects this source tree, tests it, and uploads debug/release APK artifacts.
