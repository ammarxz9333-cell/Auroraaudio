#!/usr/bin/env bash
set -euo pipefail

echo '[daviewer] starting personal APK build'
ROOT="$PWD"
OUT="$ROOT/output/simulation-assurance"
WORK="$RUNNER_TEMP/daviewer-personal-build"
FLUTTER_DIR="$RUNNER_TEMP/flutter"
mkdir -p "$OUT"
rm -rf "$WORK" "$FLUTTER_DIR"

echo '[daviewer] fetching Flutter 3.47.1'
curl -fL --retry 3 --retry-delay 5 \
  -o "$RUNNER_TEMP/flutter.tar.xz" \
  'https://storage.googleapis.com/flutter_infra_release/releases/stable/linux/flutter_linux_3.47.1-stable.tar.xz'
tar -xf "$RUNNER_TEMP/flutter.tar.xz" -C "$RUNNER_TEMP"
export PATH="$FLUTTER_DIR/bin:$PATH"
flutter --version

echo '[daviewer] fetching DAViewer v0.5.3'
git clone --depth 1 --branch v0.5.3 https://github.com/redtidev1918/DAViewer.git "$WORK"
cd "$WORK"
python "$ROOT/tools/daviewer_v053_patch.py"
dart format lib/core/auth/webview_oauth_bridge.dart lib/features/web_login/web_login_screen.dart lib/features/home/home_providers.dart
flutter pub get

echo '[daviewer] static analysis'
flutter analyze

echo '[daviewer] tests'
flutter test

echo '[daviewer] generating personal signing key'
STORE_PASS="$(python - <<'PY'
import secrets
print(secrets.token_urlsafe(24))
PY
)"
KEY_PASS="$STORE_PASS"
KEY_ALIAS='daviewerpersonal'
keytool -genkeypair -v \
  -keystore android/app/personal-upload-keystore.p12 \
  -storetype PKCS12 \
  -storepass "$STORE_PASS" \
  -keypass "$KEY_PASS" \
  -alias "$KEY_ALIAS" \
  -keyalg RSA -keysize 2048 -validity 10000 \
  -dname 'CN=DAViewer Personal, OU=Personal, O=Personal, L=Bremen, ST=Bremen, C=DE'
cat > android/key.properties <<EOF
storePassword=$STORE_PASS
keyPassword=$KEY_PASS
keyAlias=$KEY_ALIAS
storeFile=personal-upload-keystore.p12
storeType=PKCS12
EOF

echo '[daviewer] building release APK'
flutter build apk --release

APK='build/app/outputs/flutter-apk/app-release.apk'
test -s "$APK"
cp "$APK" "$OUT/DAViewer-v0.5.3-personal-google-persistent.apk"
cp android/app/personal-upload-keystore.p12 "$OUT/DAViewer-personal-signing-key.p12"
cp android/key.properties "$OUT/DAViewer-personal-key.properties"

echo '[daviewer] packaging binary artifacts into JSON for existing CI uploader'
python - "$OUT" <<'PY'
from pathlib import Path
import base64, hashlib, json, sys
out = Path(sys.argv[1])
for name in [
    'DAViewer-v0.5.3-personal-google-persistent.apk',
    'DAViewer-personal-signing-key.p12',
    'DAViewer-personal-key.properties',
]:
    p = out / name
    data = p.read_bytes()
    payload = {
        'filename': name,
        'sha256': hashlib.sha256(data).hexdigest(),
        'size': len(data),
        'base64': base64.b64encode(data).decode('ascii'),
    }
    (out / f'{name}.json').write_text(json.dumps(payload), encoding='utf-8')
print('packaged', [p.name for p in out.glob('DAViewer-*.json')])
PY

rm -f "$OUT/DAViewer-v0.5.3-personal-google-persistent.apk"
rm -f "$OUT/DAViewer-personal-signing-key.p12"
rm -f "$OUT/DAViewer-personal-key.properties"
echo '[daviewer] build complete'