#!/usr/bin/env bash
set -euo pipefail

echo '[pixvault] starting Android build'
ROOT="$PWD"
OUT="$ROOT/output/simulation-assurance"
WORK="$RUNNER_TEMP/pixvault-build"
FLUTTER_DIR="$RUNNER_TEMP/flutter"
mkdir -p "$OUT"
rm -rf "$WORK" "$FLUTTER_DIR"

echo '[pixvault] fetching Flutter 3.47.1'
curl -fL --retry 3 --retry-delay 5   -o "$RUNNER_TEMP/flutter.tar.xz"   'https://storage.googleapis.com/flutter_infra_release/releases/stable/linux/flutter_linux_3.47.1-stable.tar.xz'
tar -xf "$RUNNER_TEMP/flutter.tar.xz" -C "$RUNNER_TEMP"
export PATH="$FLUTTER_DIR/bin:$PATH"
flutter --version

echo '[pixvault] creating app'
flutter create   --platforms=android   --org app.pixvault   --project-name pixvault   "$WORK"
cd "$WORK"

cp "$ROOT/tools/pixvault_main.dart" lib/main.dart
rm -rf test

python - <<'PY'
from pathlib import Path
p = Path('pubspec.yaml')
s = p.read_text()
s = s.replace(
    '  cupertino_icons: ^1.0.8\n',
    '  cupertino_icons: ^1.0.8\n'
    '  cached_network_image: ^3.4.1\n'
    '  flutter_inappwebview: ^6.1.5\n'
    '  http: ^1.5.0\n'
    '  path_provider: ^2.1.5\n'
    '  shared_preferences: ^2.5.3\n'
    '  url_launcher: ^6.3.2\n'
    '  video_player: ^2.10.0\n'
)
p.write_text(s)

m = Path('android/app/src/main/AndroidManifest.xml')
x = m.read_text()
if 'android.permission.INTERNET' not in x:
    x = x.replace(
        '<manifest xmlns:android="http://schemas.android.com/apk/res/android">',
        '<manifest xmlns:android="http://schemas.android.com/apk/res/android">\n'
        '    <uses-permission android:name="android.permission.INTERNET"/>'
    )
x = x.replace('android:label="pixvault"', 'android:label="PixVault"')
m.write_text(x)

g = Path('android/app/build.gradle.kts')
s = g.read_text()
prefix = '''import java.io.FileInputStream
import java.util.Properties

val keystoreProperties = Properties()
val keystorePropertiesFile = rootProject.file("key.properties")
if (keystorePropertiesFile.exists()) {
    keystoreProperties.load(FileInputStream(keystorePropertiesFile))
}

'''
if not s.startswith('import java.io.FileInputStream'):
    s = prefix + s
needle = '    buildTypes {\n'
signing = '''    signingConfigs {
        create("release") {
            keyAlias = keystoreProperties["keyAlias"] as String
            keyPassword = keystoreProperties["keyPassword"] as String
            storeFile = file(keystoreProperties["storeFile"] as String)
            storePassword = keystoreProperties["storePassword"] as String
        }
    }

'''
if 'create("release")' not in s:
    s = s.replace(needle, signing + needle)
s = s.replace(
    'signingConfig = signingConfigs.getByName("debug")',
    'signingConfig = signingConfigs.getByName("release")'
)
s = s.replace(
    'applicationId = "app.pixvault.pixvault"',
    'applicationId = "app.pixvault.viewer"'
)
g.write_text(s)
PY

echo '[pixvault] dependencies'
flutter pub get

echo '[pixvault] format and analyze'
dart format lib/main.dart
flutter analyze --no-fatal-infos

echo '[pixvault] generating signing key'
STORE_PASS="$(python - <<'PY'
import secrets
print(secrets.token_urlsafe(24))
PY
)"
KEY_ALIAS='pixvaultpersonal'
keytool -genkeypair -v   -keystore android/app/pixvault-key.p12   -storetype PKCS12   -storepass "$STORE_PASS"   -keypass "$STORE_PASS"   -alias "$KEY_ALIAS"   -keyalg RSA -keysize 2048 -validity 10000   -dname 'CN=PixVault Personal, OU=Personal, O=Personal, L=Bremen, ST=Bremen, C=DE'

cat > android/key.properties <<EOF
storePassword=$STORE_PASS
keyPassword=$STORE_PASS
keyAlias=$KEY_ALIAS
storeFile=pixvault-key.p12
storeType=PKCS12
EOF

echo '[pixvault] building release APK'
flutter build apk --release

APK='build/app/outputs/flutter-apk/app-release.apk'
test -s "$APK"
cp "$APK" "$OUT/PixVault-0.1.0-personal.apk"
cp android/app/pixvault-key.p12 "$OUT/PixVault-personal-signing-key.p12"
cp android/key.properties "$OUT/PixVault-personal-key.properties"

echo '[pixvault] packaging source'
rm -rf build .dart_tool
zip -qr "$OUT/PixVault-0.1.0-source.zip"   lib pubspec.yaml pubspec.lock android README.md

echo '[pixvault] encoding binary outputs for existing artifact uploader'
python - "$OUT" <<'PY'
from pathlib import Path
import base64, hashlib, json, sys
out = Path(sys.argv[1])
names = [
    'PixVault-0.1.0-personal.apk',
    'PixVault-personal-signing-key.p12',
    'PixVault-personal-key.properties',
    'PixVault-0.1.0-source.zip',
]
for name in names:
    p = out / name
    data = p.read_bytes()
    payload = {
        'filename': name,
        'sha256': hashlib.sha256(data).hexdigest(),
        'size': len(data),
        'base64': base64.b64encode(data).decode('ascii'),
    }
    (out / f'{name}.json').write_text(json.dumps(payload), encoding='utf-8')
print('packaged', names)
PY

rm -f   "$OUT/PixVault-0.1.0-personal.apk"   "$OUT/PixVault-personal-signing-key.p12"   "$OUT/PixVault-personal-key.properties"   "$OUT/PixVault-0.1.0-source.zip"

echo '[pixvault] build complete'
