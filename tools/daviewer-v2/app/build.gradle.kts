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
        versionCode = 4
        versionName = "4.0"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    implementation("androidx.appcompat:appcompat:1.7.0")
    implementation("androidx.recyclerview:recyclerview:1.4.0")
    implementation("com.google.android.material:material:1.12.0")
    implementation("com.github.bumptech.glide:glide:4.16.0")
}
