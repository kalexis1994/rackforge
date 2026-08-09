plugins {
    id("com.android.application")
}

val generatedRustLibraries = layout.buildDirectory.dir("generated/rust-jni")

android {
    namespace = "org.rackforge.android"
    compileSdk = 36

    defaultConfig {
        applicationId = "org.rackforge.android"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0-prototype"
    }

    sourceSets {
        getByName("main").jniLibs.srcDir(generatedRustLibraries)
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}
