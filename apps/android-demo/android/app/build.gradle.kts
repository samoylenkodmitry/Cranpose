plugins {
    id("com.android.application")
}

fun parseBooleanGradleProperty(raw: String?, propertyName: String): Boolean? {
    val value = raw?.trim()?.lowercase()?.takeIf { it.isNotEmpty() } ?: return null
    return when (value) {
        "true", "1", "yes", "on" -> true
        "false", "0", "no", "off" -> false
        else -> throw GradleException("Invalid boolean for $propertyName: $raw")
    }
}

fun environmentFlag(name: String): Boolean {
    return parseBooleanGradleProperty(
        providers.environmentVariable(name).orNull,
        name
    ) == true
}

val isCiBuild = environmentFlag("CI") || environmentFlag("GITHUB_ACTIONS")
val defaultReleaseAbis = listOf("arm64-v8a", "armeabi-v7a", "x86", "x86_64")
val releaseRustFast = parseBooleanGradleProperty(
    providers.gradleProperty("rustFastRelease").orNull,
    "rustFastRelease"
) ?: !isCiBuild
val releaseRustAbis = providers.gradleProperty("rustAbis")
    .orNull
    ?.split(',')
    ?.map(String::trim)
    ?.filter(String::isNotEmpty)
    ?.takeIf { it.isNotEmpty() }
    ?: if (releaseRustFast) {
        listOf("arm64-v8a")
    } else {
        defaultReleaseAbis
    }
val releaseRustProfileFlag = providers.gradleProperty("rustCargoProfile")
    .orNull
    ?.takeIf { it.isNotBlank() }
    ?.let { "--profile $it" }
    ?: if (releaseRustFast) {
        "--profile release-fast"
    } else {
        "--release"
    }
val releaseRustTargetArgs = releaseRustAbis.joinToString(" \\\n            ") { "-t $it" }

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

    // CI release APKs ship one ABI each (arm64 phones don't pay for x86_64
    // emulator bytes); outputs are app-<abi>-release.apk. Local builds keep
    // the single fast-ABI flow via ndk.abiFilters (splits and abiFilters
    // are mutually exclusive in AGP).
    splits {
        abi {
            isEnable = isCiBuild
            reset()
            include(*releaseRustAbis.toTypedArray())
            isUniversalApk = false
        }
    }

    buildTypes {
        debug {
            // Debug builds: x86_64 only for emulator (faster builds, smaller APK)
            // Add "arm64-v8a" if testing on physical devices. CI uses ABI
            // splits, which exclude per-buildtype abiFilters.
            if (!isCiBuild) {
                ndk {
                    abiFilters.add("x86_64")
                }
            }
        }
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
            signingConfig = signingConfigs.getByName("debug")

            // Local release checks default to one fast ABI; CI uses ABI
            // splits instead (see splits above).
            if (!isCiBuild) {
                ndk {
                    abiFilters += releaseRustAbis
                }
            }
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    sourceSets {
        getByName("main") {
            java.directories.add("../../../../crates/cranpose/android/java")
        }
        getByName("debug") {
            // Path relative to app/ directory. Cargo builds to android/target/android/
            jniLibs.directories.add("../target/android")
        }
        getByName("release") {
            jniLibs.directories.add("../target/android")
        }
    }

    packaging {
        jniLibs {
            if (releaseRustFast) {
                keepDebugSymbols += "**/libdesktop_app.so"
            }
        }
    }
}

dependencies {
    implementation("androidx.appcompat:appcompat:1.7.1")
}

tasks.withType<JavaCompile>().configureEach {
    options.compilerArgs.add("-Xlint:deprecation")
}

// Check if cargo-ndk is available
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
    inputs.files(fileTree("../../../../apps/desktop-demo-platform/src") {
        include("**/*.rs")
    })
    inputs.file("../../../../apps/desktop-demo-platform/Cargo.toml")
    inputs.file("../../../../Cargo.toml")
    inputs.file("../../../../Cargo.lock")

    // cargo-ndk writes the .so here and `jniLibs.directories` above reads it
    // back, so this directory is this task's output and has to say so.
    // Without it Gradle never learns the directory changed: the merge and
    // package tasks that depend on this one check the snapshot taken before
    // cargo ran, report UP-TO-DATE, and the APK ships the PREVIOUS build's
    // library. The failure is silent and reads as a device bug -- what runs on
    // screen is one build behind what is on disk, so a fix "does not work on
    // device" until some unrelated change forces a repackage.
    outputs.dir(rootProject.file("target/android"))

    // Always run this task - let Cargo handle its own incremental builds.
    // This prevents Gradle/Cargo caching conflicts, and is separate from the
    // output declaration above: it only forces THIS task to run again, and
    // says nothing about what the task changed on disk.
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
            -p desktop-app-platform \
            --lib \
            --features android,renderer-wgpu \
            --no-default-features
    """)
}

// Task to build Rust library for Android release builds
tasks.register<Exec>("buildRustRelease") {
    description = "Build Rust library for Android (local-fast by default, full release on CI)"
    group = "rust"

    inputs.files(fileTree("../../../../crates") {
        include("**/*.rs")
        include("**/Cargo.toml")
    })
    inputs.files(fileTree("../../../../apps/desktop-demo/src") {
        include("**/*.rs")
    })
    inputs.files(fileTree("../../../../apps/desktop-demo-platform/src") {
        include("**/*.rs")
    })
    inputs.file("../../../../apps/desktop-demo-platform/Cargo.toml")
    inputs.file("../../../../Cargo.toml")
    inputs.file("../../../../Cargo.lock")
    // The directory cargo-ndk writes and `jniLibs.directories` reads. See
    // `buildRustDebug`: undeclared, it leaves the packaging tasks looking at a
    // pre-cargo snapshot and the APK ships the previous build's library.
    outputs.dir(rootProject.file("target/android"))
    outputs.upToDateWhen { false }

    // Check cargo-ndk availability
    doFirst {
        checkCargoNdk()
        val mode = if (releaseRustFast) "fast local release check" else "optimized release"
        println("Rust Android build mode: $mode")
        println("Rust Android ABIs: ${releaseRustAbis.joinToString(", ")}")
        println("Rust cargo profile: $releaseRustProfileFlag")
    }

    workingDir = rootProject.projectDir

    // Local release checks default to release-fast + arm64-v8a.
    // CI or explicit flags switch back to full optimized multi-ABI release.
    commandLine("sh", "-c", """
        cargo ndk \
            $releaseRustTargetArgs \
            -o target/android \
            build \
            $releaseRustProfileFlag \
            -p desktop-app-platform \
            --lib \
            --features android,renderer-wgpu \
            --no-default-features
    """)
}

// Wire Rust builds to Android build variants
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
