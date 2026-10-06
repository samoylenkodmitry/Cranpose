plugins {
    id("com.android.application")
}

val lynx = "4.1.0"
// PrimJS, Lynx's JavaScript engine, has releases of its own.
val primjs = "4.1.1"

android {
    namespace = "dev.perfcompare.lynx"
    compileSdk = 37

    defaultConfig {
        applicationId = "dev.perfcompare.lynx"
        minSdk = 28
        targetSdk = 36
        versionCode = 1
        versionName = "1.0"
        // One arm64 build, as every app of the comparison.
        ndk { abiFilters += "arm64-v8a" }
    }

    buildTypes {
        // What ships: R8 full mode, resource shrinking, not debuggable.
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    sourceSets.named("main") {
        // The page `npm run build` bundles in `page/dist`.
        assets.directories += "../page/dist"
    }
}

dependencies {
    implementation("org.lynxsdk.lynx:lynx:$lynx")
    implementation("org.lynxsdk.lynx:lynx-jssdk:$lynx")
    implementation("org.lynxsdk.lynx:lynx-trace:$lynx")
    implementation("org.lynxsdk.lynx:primjs:$primjs")
    // Images through Fresco, the default image service.
    implementation("org.lynxsdk.lynx:lynx-service-image:$lynx")
    implementation("com.facebook.fresco:fresco:2.3.0")
    implementation("org.lynxsdk.lynx:lynx-service-log:$lynx")
    // `<svg>`, for the sparklines.
    implementation("org.lynxsdk.lynx:xelement:$lynx")
    implementation("org.lynxsdk.lynx:xelement-svg:$lynx")
    // Lynx asks for vectordrawable-animated 1.0.0, which shares its namespace
    // with vectordrawable and fails AGP's manifest check.
    implementation("androidx.vectordrawable:vectordrawable-animated:1.2.0")
    implementation("androidx.profileinstaller:profileinstaller:1.4.1")
}
