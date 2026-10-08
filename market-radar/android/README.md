# Market Radar Android
Android WebView wrapper for the real-data chart dashboard. It loads the GitHub Pages URL `https://ammarxz9333-cell.github.io/Auroraaudio/`; configure GitHub Pages Actions deployment for the chart app before using the APK. Requires network connectivity. No fake/offline price data.

## Build
Use Android Studio with JDK 17 or `gradle :app:assembleDebug` from this directory (Gradle 8.9+). Debug APK: `app/build/outputs/apk/debug/app-debug.apk`. The GitHub Actions workflow builds and uploads this artifact.

This is a first Android shell, not yet a native chart implementation or verified release build.