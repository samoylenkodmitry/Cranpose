import java.io.ByteArrayOutputStream

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
    }

    buildTypes {
        debug {
            // Debug builds: x86_64 only for emulator (faster builds, smaller APK)
            // Add "arm64-v8a" if testing on physical devices
            ndk {
                abiFilters.add("x86_64")
            }
        }
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
            signingConfig = signingConfigs.getByName("debug")

            // Release builds include all supported ABIs
            ndk {
                abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86", "x86_64")
            }
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }

    sourceSets {
        getByName("debug") {
            // Path relative to app/ directory. Cargo builds to android/target/android/
            jniLibs.srcDirs("../target/android")
        }
        getByName("release") {
            jniLibs.srcDirs("../target/android")
        }
    }
}

dependencies {
    implementation("androidx.appcompat:appcompat:1.6.1")
}

// Check if cargo-ndk is available
fun checkCargoNdk() {
    val result = exec {
        commandLine("cargo", "ndk", "--version")
        isIgnoreExitValue = true
        // Output suppression: version check is silent
        standardOutput = ByteArrayOutputStream()
        errorOutput = ByteArrayOutputStream()
    }

    if (result.exitValue != 0) {
        throw GradleException(
            "cargo-ndk is not installed. Install it with:\n" +
            "    cargo install cargo-ndk\n" +
            "See: https://github.com/bbqsrc/cargo-ndk"
        )
    }
}

// Task to build Rust library for Android debug builds
tasks.register<Exec>("buildRustDebug") {
    description = "Build Rust library for Android (debug, single ABI)"
    group = "rust"

    // Track Rust source files as inputs so Gradle rebuilds when code changes
    inputs.files(fileTree("../../../../crates") {
        include("**/*.rs")
        include("**/Cargo.toml")
    })
    inputs.files(fileTree("../../../../apps/desktop-demo/src") {
        include("**/*.rs")
    })
    inputs.file("../../../../Cargo.toml")
    inputs.file("../../../../Cargo.lock")
    
    // Always run this task - let Cargo handle its own incremental builds
    // This prevents Gradle/Cargo caching conflicts
    outputs.upToDateWhen { false }

    // Check cargo-ndk availability
    doFirst {
        checkCargoNdk()
    }

    workingDir = rootProject.projectDir

    // Debug builds: x86_64 only for emulator (faster iteration)
    commandLine("sh", "-c", """
        cargo ndk \
            -t x86_64 \
            -o target/android \
            build \
            -p desktop-app \
            --lib \
            --features android,renderer-wgpu \
            --no-default-features
    """)
}

// Task to build Rust library for Android release builds
tasks.register<Exec>("buildRustRelease") {
    description = "Build Rust library for Android (release, all ABIs)"
    group = "rust"

    // Check cargo-ndk availability
    doFirst {
        checkCargoNdk()
    }

    workingDir = rootProject.projectDir

    // Release builds: all supported ABIs
    commandLine("sh", "-c", """
        cargo ndk \
            -t arm64-v8a \
            -t armeabi-v7a \
            -t x86 \
            -t x86_64 \
            -o target/android \
            build \
            --release \
            -p desktop-app \
            --lib \
            --features android,renderer-wgpu \
            --no-default-features
    """)
}

// Wire Rust builds to Android build variants
afterEvaluate {
    // Wire Rust builds to merge native libs tasks
    tasks.matching { it.name.startsWith("merge") && it.name.contains("NativeLibs") }.configureEach {
        if (name.contains("Debug", ignoreCase = true)) {
            dependsOn("buildRustDebug")
        } else if (name.contains("Release", ignoreCase = true)) {
            dependsOn("buildRustRelease")
        }
    }
}
