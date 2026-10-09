plugins {
    id("com.android.application")
    id("dev.cranpose.android")
}

cranpose {
    workspaceRoot.set("../..")
    cargoPackage.set("perf-compare-cranpose")
    libraryName.set("perf_compare")
    label.set("Perf Cranpose")
    releaseAbis.set(listOf("arm64-v8a"))
    // The plugin builds without default features, which include `entry`: the
    // library this application packages exports the Android entry symbol.
    features.set(listOf("android", "renderer-wgpu", "entry"))
}

android {
    namespace = "dev.perfcompare.cranpose"
    compileSdk = 37

    defaultConfig {
        applicationId = "dev.perfcompare.cranpose"
        // A second build, such as the latest release beside main, installs
        // next to the first under its own package: -PperfCompareSuffix=.release
        applicationIdSuffix = providers.gradleProperty("perfCompareSuffix").orNull
        minSdk = 24
        targetSdk = 36
        versionCode = 1
        versionName = "1.0"
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"))
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}
