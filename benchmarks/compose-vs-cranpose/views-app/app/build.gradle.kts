plugins {
    id("com.android.application")
}

android {
    namespace = "dev.perfcompare.views"
    compileSdk = 37

    defaultConfig {
        applicationId = "dev.perfcompare.views"
        minSdk = 28
        targetSdk = 36
        versionCode = 1
        versionName = "1.0"
    }

    buildTypes {
        // What ships: R8 full mode, resource shrinking, not debuggable.
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"))
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    sourceSets.named("main") {
        // The data generator the Android apps share.
        kotlin.directories += "../../shared-kotlin"
    }
}

dependencies {
    implementation("androidx.recyclerview:recyclerview:1.4.0")
    implementation("androidx.profileinstaller:profileinstaller:1.4.1")
}
