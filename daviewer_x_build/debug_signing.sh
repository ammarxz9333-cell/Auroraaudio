#!/usr/bin/env bash
set -euo pipefail
keytool -genkeypair -noprompt -keystore daviewer/android/app/upload-keystore.p12 -storetype PKCS12 -storepass android -keypass android -alias upload -keyalg RSA -keysize 2048 -validity 3650 -dname "CN=DAViewer X Debug,O=Local Build,C=DE"
cat > daviewer/android/key.properties <<'EOF'
storePassword=android
keyPassword=android
keyAlias=upload
storeFile=upload-keystore.p12
storeType=PKCS12
EOF
