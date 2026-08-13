plugins {
    id("com.android.application")
}

android {
    namespace = "com.cranpose.isolated.demo"
    compileSdk = 36

    defaultConfig {
        applicationId = "com.cranpose.isolated.demo"
        minSdk = 24
        targetSdk = 36
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
            jniLibs.directories.add("../target/android")
        }
        getByName("release") {
            jniLibs.directories.add("../target/android")
        }
    }
}

dependencies {
    implementation("androidx.appcompat:appcompat:1.7.1")
}

fun checkCargoNdk() {
    val result = providers.exec {
        commandLine("cargo", "ndk", "--version")
        isIgnoreExitValue = true
    }.result.get()

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

    // cargo-ndk writes the .so here and `jniLibs.directories` above reads it
    // back, so this directory is this task's output and has to say so.
    // Without it Gradle never learns the directory changed: the merge and
    // package tasks that depend on this one check the snapshot taken before
    // cargo ran, report UP-TO-DATE, and the APK ships the PREVIOUS build's
    // library. The failure is silent and reads as a device bug -- what runs on
    // screen is one build behind what is on disk, so a fix "does not work on
    // device" until some unrelated change forces a repackage.
    outputs.dir(rootProject.file("target/android"))

    // Separate from the output declaration above: this only forces THIS task
    // to run again, and says nothing about what the task changed on disk.
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

    // The directory cargo-ndk writes and `jniLibs.directories` reads. See
    // `buildRustDebug`: undeclared, it leaves the packaging tasks looking at a
    // pre-cargo snapshot and the APK ships the previous build's library.
    outputs.dir(rootProject.file("target/android"))
    // Cargo owns incrementality here; a task with a declared output would
    // otherwise be skipped whenever that output happened to be unchanged.
    outputs.upToDateWhen { false }

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
    // Both merge tasks read `target/android`: `mergeJniLibFolders` collects the
    // source directories and `mergeNativeLibs` collects the libraries in them.
    // Wiring only the second leaves the first racing the cargo build, which
    // Gradle reports as an implicit dependency once the cargo tasks declare the
    // directory as their output.
    tasks.matching {
        it.name.startsWith("merge") &&
            (it.name.contains("NativeLibs") || it.name.contains("JniLibFolders"))
    }.configureEach {
        if (name.contains("Debug", ignoreCase = true)) {
            dependsOn("buildRustDebug")
        } else if (name.contains("Release", ignoreCase = true)) {
            dependsOn("buildRustRelease")
        }
    }
}
