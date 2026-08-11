plugins {
    id("com.android.application")
}

val generatedRustLibraries = layout.buildDirectory.dir("generated/rust-jni")
val generatedNotices = layout.buildDirectory.dir("generated/notices")
val copyThirdPartyNotices by tasks.registering(Copy::class) {
    from(rootProject.layout.projectDirectory.file("../../THIRD_PARTY_NOTICES.md"))
    into(generatedNotices)
}

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
        getByName("main").assets.srcDir(generatedNotices)
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

tasks.named("preBuild").configure {
    dependsOn(copyThirdPartyNotices)
}
