plugins {
    id("com.android.application")
}

android {
    namespace = "com.compose_rs.demo"
    compileSdk = 34

    defaultConfig {
        applicationId = "com.compose_rs.demo"
        minSdk = 24
        targetSdk = 34
        versionCode = 1
        versionName = "1.0"

        ndk {
            abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86", "x86_64")
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }

    sourceSets {
        getByName("main") {
            jniLibs.srcDirs("../../../target/android")
        }
    }
}

dependencies {
    implementation("androidx.appcompat:appcompat:1.6.1")
}

// Task to build Rust library for Android
tasks.register<Exec>("buildRustAndroid") {
    workingDir = file("../../../")

    commandLine("sh", "-c", """
        cargo ndk -o target/android -t arm64-v8a -t armeabi-v7a -t x86 -t x86_64 build -p desktop-app --lib --release --features android,renderer-wgpu --no-default-features
    """)
}

// Make preBuild depend on Rust build
tasks.named("preBuild") {
    dependsOn("buildRustAndroid")
}
