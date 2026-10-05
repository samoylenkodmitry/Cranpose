import com.android.build.api.dsl.ApplicationExtension
import com.android.build.api.variant.ApplicationAndroidComponentsExtension

plugins {
    id("com.android.application") apply false
}

// Every Rust app the same way: its crate built for arm64 in release by
// cargo-ndk, loaded by a NativeActivity whose launch activity hands the
// native side the `am start` extras.
subprojects {
    apply(plugin = "com.android.application")
    val app = name
    val rust = layout.buildDirectory.dir("rust")
    extensions.configure<ApplicationExtension> {
        namespace = "dev.perfcompare.$app"
        compileSdk = 37
        ndkVersion = "27.0.12077973"
        defaultConfig {
            applicationId = "dev.perfcompare.$app"
            minSdk = 28
            targetSdk = 36
            versionCode = 1
            versionName = "1.0"
            manifestPlaceholders["libName"] = "perf_$app"
            manifestPlaceholders["label"] = "Perf $app"
        }
        buildTypes {
            getByName("release") {
                isMinifyEnabled = true
                proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"))
                signingConfig = signingConfigs.getByName("debug")
            }
        }
        compileOptions {
            sourceCompatibility = JavaVersion.VERSION_17
            targetCompatibility = JavaVersion.VERSION_17
        }
        sourceSets.named("main") {
            manifest.srcFile(rootProject.file("src/main/AndroidManifest.xml"))
            kotlin.directories += rootProject.file("src/main/java").path
            jniLibs.directories += rust.get().asFile.path
        }
    }
    val sdk = extensions.getByType<ApplicationAndroidComponentsExtension>().sdkComponents
    val cargoNdk = tasks.register<Exec>("cargoNdk") {
        workingDir = projectDir
        doFirst {
            environment("ANDROID_NDK_HOME", sdk.ndkDirectory.get().asFile.path)
            // Build scripts that compile Java, such as Slint's, use the app's platform.
            environment("ANDROID_JAR", sdk.bootClasspath.get().first().asFile.path)
        }
        commandLine(
            "cargo", "ndk", "--platform", "28", "-t", "arm64-v8a",
            "-o", rust.get().asFile.path, "build", "--release",
        )
    }
    tasks.named("preBuild") { dependsOn(cargoNdk) }
}
