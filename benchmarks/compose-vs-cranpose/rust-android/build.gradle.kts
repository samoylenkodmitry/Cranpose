import com.android.build.api.dsl.ApplicationExtension
import com.android.build.api.variant.ApplicationAndroidComponentsExtension

plugins {
    id("com.android.application") apply false
}

/** The activity each app's framework runs in: `game` or `native`. */
val activities = mapOf("egui" to "game", "slint" to "native")

// Every Rust app the same way: its crate built for arm64 in release by
// cargo-ndk, loaded by the activity its framework runs in, whose launch
// activity hands the native side the `am start` extras.
subprojects {
    apply(plugin = "com.android.application")
    val app = name
    val activity = activities.getValue(app)
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
            manifestPlaceholders["theme"] =
                if (activity == "game") "@style/LaunchTheme" else "@android:style/Theme.NoTitleBar.Fullscreen"
        }
        buildTypes {
            getByName("release") {
                isMinifyEnabled = true
                proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"))
                if (activity == "game") proguardFiles(rootProject.file("src/game/proguard-rules.pro"))
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
            kotlin.directories += rootProject.file("src/$activity/java").path
            res.directories += rootProject.file("src/$activity/res").path
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
    if (activity == "game") {
        // The GameActivity release android-activity 0.6 is written against.
        dependencies.add("implementation", "androidx.games:games-activity:4.4.0")
        dependencies.add("implementation", "androidx.appcompat:appcompat:1.7.1")
    }
}
