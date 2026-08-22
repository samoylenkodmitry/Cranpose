// The Cranpose Android library: the framework's Java classes and the manifest
// contributions every Cranpose application needs.
//
// The Java sources live beside the Rust that calls them, in
// `crates/cranpose/android/java`, so a JNI signature and its Java method are
// changed in one place. This module packages them; consumers depend on the
// artifact and never point a source set at the framework's tree.
plugins {
    id("com.android.library")
    `maven-publish`
}

android {
    namespace = "dev.cranpose.android"
    compileSdk = 36

    defaultConfig {
        minSdk = 24
        consumerProguardFiles("consumer-rules.pro")

        // The manifest declares the activity with placeholders an application
        // fills through the Gradle plugin. This module still has to link its
        // own resources, and `android:theme` must resolve to a real style to do
        // that - an unsubstituted `${cranposeTheme}` is not a resource
        // reference and `verifyReleaseResources` refuses it. These are the
        // values the plugin uses as its conventions; an application overrides
        // them through the `cranpose { }` block.
        manifestPlaceholders["cranposeTheme"] = "@android:style/Theme.NoTitleBar.Fullscreen"
        manifestPlaceholders["cranposeLabel"] = "Cranpose"
        manifestPlaceholders["cranposeLibName"] = "cranpose_app"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    sourceSets {
        getByName("main") {
            java.directories.clear()
            java.directories.add(
                rootProject.file("../crates/cranpose/android/java").absolutePath
            )
        }
    }

    publishing {
        singleVariant("release") {
            withSourcesJar()
        }
    }
}

dependencies {
    implementation("androidx.appcompat:appcompat:1.7.1")
}

tasks.withType<JavaCompile>().configureEach {
    options.compilerArgs.add("-Xlint:deprecation")
}

publishing {
    publications {
        register<MavenPublication>("release") {
            afterEvaluate { from(components["release"]) }
            artifactId = "cranpose-android"
        }
    }
}
