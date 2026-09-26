plugins {
    id("com.android.application")
}

val generatedRustLibraries = layout.buildDirectory.dir("generated/rust-jni")
val generatedNotices = layout.buildDirectory.dir("generated/notices")
val generatedBundledPlugins = layout.buildDirectory.dir("generated/bundled-plugins")
val generatedWebUi = layout.buildDirectory.dir("generated/web-ui")
val rackforgeRevision = providers.environmentVariable("RACKFORGE_REVISION")
    .getOrElse("development")
    .replace("\"", "")
val previewApp = providers.environmentVariable("RACKFORGE_ANDROID_PREVIEW")
    .getOrElse("0") == "1"
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
        versionCode = 26
        versionName = "0.1.24-preview"
        buildConfigField("String", "RACKFORGE_REVISION", "\"$rackforgeRevision\"")
        manifestPlaceholders["rackforgeAppLabel"] = "RackForge"
    }

    buildTypes {
        getByName("debug") {
            if (previewApp) {
                // CI uses a fresh debug key on every run. A separate package
                // avoids a signature conflict with the user's installed app
                // and leaves its presets and racks untouched.
                applicationIdSuffix = ".preview"
                versionNameSuffix = "-ci"
                manifestPlaceholders["rackforgeAppLabel"] = "RackForge Preview"
            }
        }
    }

    sourceSets {
        getByName("main").jniLibs.srcDir(generatedRustLibraries)
        getByName("main").assets.srcDir(generatedNotices)
        getByName("main").assets.srcDir(generatedBundledPlugins)
        getByName("main").assets.srcDir(generatedWebUi)
    }

    buildFeatures {
        buildConfig = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    testImplementation("junit:junit:4.13.2")
}

tasks.named("preBuild").configure {
    dependsOn(copyThirdPartyNotices)
}
