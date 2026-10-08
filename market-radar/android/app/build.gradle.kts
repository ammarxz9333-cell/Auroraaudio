plugins { id("com.android.application") }
android {
 namespace = "com.marketradar.chart"
 compileSdk = 35
 defaultConfig { applicationId = "com.marketradar.chart"; minSdk = 26; targetSdk = 35; versionCode = 1; versionName = "0.1.0" }
 buildTypes { release { isMinifyEnabled = false } }
 compileOptions { sourceCompatibility = JavaVersion.VERSION_17; targetCompatibility = JavaVersion.VERSION_17 }
}
