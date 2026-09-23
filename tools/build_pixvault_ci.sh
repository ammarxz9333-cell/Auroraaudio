#!/usr/bin/env bash
set -euo pipefail

echo '[pixvault] starting personal APK build'
ROOT="$PWD"
OUT="$ROOT/output/simulation-assurance"
WORK="$RUNNER_TEMP/pixvault-personal-build"
FLUTTER_DIR="$RUNNER_TEMP/flutter"
SOURCE_ARCHIVE="$RUNNER_TEMP/pixvault_source.tar.gz"
mkdir -p "$OUT"
rm -rf "$WORK" "$FLUTTER_DIR"

echo '[pixvault] fetching Flutter 3.47.1'
curl -fL --retry 3 --retry-delay 5   -o "$RUNNER_TEMP/flutter.tar.xz"   'https://storage.googleapis.com/flutter_infra_release/releases/stable/linux/flutter_linux_3.47.1-stable.tar.xz'
tar -xf "$RUNNER_TEMP/flutter.tar.xz" -C "$RUNNER_TEMP"
export PATH="$FLUTTER_DIR/bin:$PATH"
flutter --version

echo '[pixvault] creating Android Flutter shell'
flutter create --org app.pixvault --project-name pixvault --platforms=android "$WORK"
base64 -d "$ROOT/tools/pixvault_source.tar.gz.b64" > "$SOURCE_ARCHIVE"
tar -xzf "$SOURCE_ARCHIVE" -C "$WORK"
cd "$WORK"

echo '[pixvault] dependencies + formatting'
flutter pub get
dart format lib/main.dart
flutter analyze --no-fatal-infos --no-fatal-warnings

echo '[pixvault] preparing stable personal signing key'
mkdir -p "$HOME/.android"
rm -f "$HOME/.android/debug.keystore"
keytool -genkeypair -v   -keystore "$HOME/.android/debug.keystore"   -storepass android   -keypass android   -alias androiddebugkey   -keyalg RSA -keysize 2048 -validity 10000   -dname 'CN=PixVault Personal, OU=Personal, O=Personal, L=Bremen, ST=Bremen, C=DE'

echo '[pixvault] building release APK'
flutter build apk --release

APK='build/app/outputs/flutter-apk/app-release.apk'
test -s "$APK"
cp "$APK" "$OUT/PixVault-v0.1.0-personal.apk"
cp "$HOME/.android/debug.keystore" "$OUT/PixVault-personal-signing-key.keystore"
cp "$SOURCE_ARCHIVE" "$OUT/PixVault-v0.1.0-source.tar.gz"

python - "$OUT" <<'PY'
from pathlib import Path
import base64, hashlib, json, sys
out = Path(sys.argv[1])
names = [
    'PixVault-v0.1.0-personal.apk',
    'PixVault-personal-signing-key.keystore',
    'PixVault-v0.1.0-source.tar.gz',
]
checks = []
for name in names:
    p = out / name
    data = p.read_bytes()
    sha = hashlib.sha256(data).hexdigest()
    checks.append(f'{sha}  {name}')
    payload = {
        'filename': name,
        'sha256': sha,
        'size': len(data),
        'base64': base64.b64encode(data).decode('ascii'),
    }
    (out / f'{name}.json').write_text(json.dumps(payload), encoding='utf-8')
check_file = out / 'PixVault-SHA256.txt'
check_file.write_text('\n'.join(checks) + '\n', encoding='utf-8')
data = check_file.read_bytes()
(out / 'PixVault-SHA256.txt.json').write_text(json.dumps({
    'filename': 'PixVault-SHA256.txt',
    'sha256': hashlib.sha256(data).hexdigest(),
    'size': len(data),
    'base64': base64.b64encode(data).decode('ascii'),
}), encoding='utf-8')
print('packaged PixVault artifacts')
PY

rm -f "$OUT/PixVault-v0.1.0-personal.apk"
rm -f "$OUT/PixVault-personal-signing-key.keystore"
rm -f "$OUT/PixVault-v0.1.0-source.tar.gz"
rm -f "$OUT/PixVault-SHA256.txt"
echo '[pixvault] build complete'
