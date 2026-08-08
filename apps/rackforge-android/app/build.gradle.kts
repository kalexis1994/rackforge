plugins {
    id("com.android.application")
}

val portablePlugin = providers.gradleProperty("rfplugin").orElse(
    rootProject.layout.projectDirectory.file(
        "../../../rackforge-plugin-rf-xp10/artifacts/rf-xp10-0.1.3.rfplugin"
    ).asFile.absolutePath
)
val generatedPluginAssets = layout.buildDirectory.dir("generated/rfplugin-assets")
val generatedRustLibraries = layout.buildDirectory.dir("generated/rust-jni")

val packagePortablePlugin by tasks.registering(Copy::class) {
    from(portablePlugin)
    into(generatedPluginAssets.map { it.dir("plugins") })
    rename { "rf-xp10.rfplugin" }
    doFirst {
        val source = file(portablePlugin.get())
        check(source.isFile) {
            "Portable plugin not found at ${source.absolutePath}. Build RF-XP10 or pass -Prfplugin=PATH."
        }
    }
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
        getByName("main").assets.srcDir(generatedPluginAssets)
        getByName("main").jniLibs.srcDir(generatedRustLibraries)
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

tasks.named("preBuild").configure {
    dependsOn(packagePortablePlugin)
}
