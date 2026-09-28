plugins { id("com.android.application"); id("org.jetbrains.kotlin.android") }
android {
 namespace = "com.ammar.marketradar"; compileSdk = 35
 defaultConfig { applicationId = "com.ammar.marketradar"; minSdk = 26; targetSdk = 35; versionCode = 2; versionName = "0.2.0" }
}
dependencies {
 implementation("androidx.core:core-ktx:1.15.0")
 implementation("androidx.appcompat:appcompat:1.7.0")
 implementation("com.google.android.material:material:1.12.0")
 implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.7")
 implementation("org.json:json:20240303")
}
