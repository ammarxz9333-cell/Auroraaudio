plugins {
    id("com.android.application")
}

android {
    namespace = "com.daviewer.v2"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.daviewer.v2"
        minSdk = 26
        targetSdk = 35
        versionCode = 2
        versionName = "2.0"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}
