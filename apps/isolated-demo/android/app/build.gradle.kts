import java.io.ByteArrayOutputStream

plugins {
    id("com.android.application")
}

android {
    namespace = "com.cranpose.isolated.demo"
    compileSdk = 34

    defaultConfig {
        applicationId = "com.cranpose.isolated.demo"
        minSdk = 24
        targetSdk = 34
        versionCode = 1
        versionName = "1.0"
    }

    buildTypes {
        debug {
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

fun checkCargoNdk() {
    val result = exec {
        commandLine("cargo", "ndk", "--version")
        isIgnoreExitValue = true
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

tasks.register<Exec>("buildRustDebug") {
    description = "Build Rust library for Android (debug, single ABI)"
    group = "rust"

    inputs.files(fileTree("../../src") {
        include("**/*.rs")
    })
    inputs.file("../../Cargo.toml")
    inputs.file("../../Cargo.lock")

    outputs.upToDateWhen { false }

    doFirst {
        checkCargoNdk()
    }

    workingDir = rootProject.projectDir

    commandLine("sh", "-c", """
        cargo ndk \
            -t x86_64 \
            -o target/android \
            build \
            -p isolated-demo \
            --lib \
            --manifest-path ../Cargo.toml \
            --features android,renderer-wgpu \
            --no-default-features
    """)
}

tasks.register<Exec>("buildRustRelease") {
    description = "Build Rust library for Android (release, all ABIs)"
    group = "rust"

    doFirst {
        checkCargoNdk()
    }

    workingDir = rootProject.projectDir

    commandLine("sh", "-c", """
        cargo ndk \
            -t arm64-v8a \
            -t armeabi-v7a \
            -t x86 \
            -t x86_64 \
            -o target/android \
            build \
            --release \
            -p isolated-demo \
            --lib \
            --manifest-path ../Cargo.toml \
            --features android,renderer-wgpu \
            --no-default-features
    """)
}

afterEvaluate {
    tasks.matching { it.name.startsWith("merge") && it.name.contains("NativeLibs") }.configureEach {
        if (name.contains("Debug", ignoreCase = true)) {
            dependsOn("buildRustDebug")
        } else if (name.contains("Release", ignoreCase = true)) {
            dependsOn("buildRustRelease")
        }
    }
}
