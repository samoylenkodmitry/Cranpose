// The Cranpose demo, built through the Cranpose Gradle plugin.
//
// Everything a Cranpose application's Android build has in common — the native
// build, ABIs, Cargo profiles, JNI packaging, the framework's Java and its
// manifest contributions — lives in the plugin. What remains here is what is
// genuinely specific to this application.
plugins {
    id("com.android.application")
    id("dev.cranpose.android")
}

cranpose {
    cargoPackage.set("desktop-app-platform")
    libraryName.set("desktop_app")
    label.set("Compose Demo")
    // The demo draws an overlay window and plays designed haptics, and nothing
    // else optional. It posts no notifications, so it does not ask to.
    services.addAll("haptics", "overlay")
    // This repository declares `release-fast` in its own Cargo.toml, so its
    // demos may ask for it: a local release check builds quicker and keeps the
    // symbols a device profile or crash report needs. Continuous integration
    // builds what ships instead. The plugin defaults to `release` for every
    // application, because a profile is the application's to declare.
    releaseProfile.set(
        if (System.getenv("CI") != null || System.getenv("GITHUB_ACTIONS") != null) {
            "release"
        } else {
            "release-fast"
        }
    )
}

android {
    namespace = "com.compose_rs.demo"
    compileSdk = 36

    defaultConfig {
        applicationId = "com.compose_rs.demo"
        minSdk = 24
        targetSdk = 36
        versionCode = 1
        versionName = "1.0"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}
