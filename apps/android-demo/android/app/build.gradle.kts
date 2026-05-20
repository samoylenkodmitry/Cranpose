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

            // Local release checks default to one fast ABI. CI/full release keeps all ABIs.
            ndk {
                abiFilters += releaseRustAbis
            }
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
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
    description = "Build Rust library for Android (local-fast by default, full release on CI)"
    group = "rust"

    inputs.files(fileTree("../../../../crates") {
        include("**/*.rs")
        include("**/Cargo.toml")
    })
    inputs.files(fileTree("../../../../apps/desktop-demo/src") {
        include("**/*.rs")
    })
    inputs.file("../../../../Cargo.toml")
    inputs.file("../../../../Cargo.lock")
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
