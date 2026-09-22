#!/usr/bin/env bash
set -euo pipefail

ROOT="$(pwd)"
WORK="$ROOT/.daviewer-build"
OUT="$ROOT/out"
DA_VIEWER_SHA="862ec4571adaccdd4feb0d70cec7ad78b98b96b4"
DAKIT_SHA="f226cef261a5fa5e4030d84ceb3f55aac1c86afa"

command -v git >/dev/null
command -v flutter >/dev/null
command -v dart >/dev/null
command -v python3 >/dev/null

rm -rf "$WORK"
mkdir -p "$WORK" "$OUT"

echo "== Flutter =="
flutter --version

echo "== Clone pinned sources =="
git clone https://github.com/redtidev1918/DAViewer.git "$WORK/DAViewer"
git -C "$WORK/DAViewer" checkout "$DA_VIEWER_SHA"
git clone https://github.com/redtidev1918/DAKit.git "$WORK/DAKit"
git -C "$WORK/DAKit" checkout "$DAKIT_SHA"

echo "== Apply resilient API patch =="
cp ci/daviewer_patch/api_config.dart   "$WORK/DAKit/packages/dakit_api/lib/src/http/api_config.dart"
cp ci/daviewer_patch/official_api_client.dart   "$WORK/DAKit/packages/dakit_api/lib/src/http/official_api_client.dart"
cp ci/daviewer_patch/rate_limit_resilience_test.dart   "$WORK/DAKit/packages/dakit_api/test/rate_limit_resilience_test.dart"

echo "== Apply mature + entitlement fallback =="
(
  cd "$WORK"
  python3 "$ROOT/ci/daviewer_patch/apply_mature_entitlement_fallback.py"
  python3 "$ROOT/ci/daviewer_patch/apply_external_oauth_android.py"
  python3 "$ROOT/ci/daviewer_patch/fix_upstream_mature_query_test.py"
)

sed -i.bak '/^resolution: workspace$/d'   "$WORK/DAKit/packages/dakit_api/pubspec.yaml" || true
sed -i.bak '/^resolution: workspace$/d'   "$WORK/DAKit/packages/dakit_web/pubspec.yaml" || true

cat > "$WORK/DAViewer/pubspec_overrides.yaml" <<EOF
dependency_overrides:
  dakit_api:
    path: ../DAKit/packages/dakit_api
  dakit_web:
    path: ../DAKit/packages/dakit_web
EOF

echo "== DAKit API tests =="
(
  cd "$WORK/DAKit/packages/dakit_api"
  dart pub get
  dart format --output=none --set-exit-if-changed lib test
  dart test
)

echo "== DAKit Web entitlement tests =="
(
  cd "$WORK/DAKit/packages/dakit_web"
  dart pub get
  dart format --output=none --set-exit-if-changed lib test
  dart test
)

echo "== DAViewer analyze/tests =="
(
  cd "$WORK/DAViewer"
  flutter pub get
  flutter analyze
  flutter test
)

echo "== Configure custom release signing =="
python3 - "$WORK/DAViewer/android/app/build.gradle.kts" <<'PY'
from pathlib import Path
import sys

p = Path(sys.argv[1])
s = p.read_text()
old = '''            signingConfig = if (keystorePropertiesFile.exists()) {
                          signingConfigs.getByName("release")
                      } else {
                          error(
                              "Release signing is not configured. " +
                              "Create android/key.properties and " +
                              "android/app/upload-keystore.p12 before building a release APK. " +
                              "See README.md / CI signing secrets."
                          )
                      }'''
new = '''            signingConfig = if (keystorePropertiesFile.exists()) {
                          signingConfigs.getByName("release")
                      } else {
                          signingConfigs.getByName("debug")
                      }'''
if old not in s:
    raise SystemExit("release signing block changed upstream")
p.write_text(s.replace(old, new))
PY

echo "== Build APK =="
(
  cd "$WORK/DAViewer"
  flutter build apk --release
)

APK="$WORK/DAViewer/build/app/outputs/flutter-apk/app-release.apk"
DEST="$OUT/DAViewer-v0.5.0-mature-entitled-resilient-20260922.apk"
cp "$APK" "$DEST"

if command -v sha256sum >/dev/null; then
  sha256sum "$DEST" > "$OUT/SHA256SUMS.txt"
elif command -v shasum >/dev/null; then
  shasum -a 256 "$DEST" > "$OUT/SHA256SUMS.txt"
fi

cat > "$OUT/BUILD-INFO.txt" <<EOF
DAViewer upstream: $DA_VIEWER_SHA
DAKit upstream: $DAKIT_SHA
Access policy: Mature content + Premium/Subscription media only when DeviantArt reports entitlement for the logged-in web session.
Android OAuth: system browser + AppLinks callback.
Payment/entitlement bypass: none.
EOF

echo
echo "SUCCESS: $DEST"
