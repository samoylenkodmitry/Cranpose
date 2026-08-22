// The `billing` service: Google Play Billing.
//
// Unlike the other feature modules this one carries code as well as a manifest.
// `CranposeBilling` is the one framework class that needs the Play Billing
// library, so it lives in its own source directory and its own artifact: an
// application that sells nothing never resolves the library, and one that does
// asks for the service by name rather than pointing a source set at the
// framework's tree.
plugins {
    id("com.android.library")
    `maven-publish`
}

android {
    namespace = "dev.cranpose.android.billing"
    compileSdk = 36

    defaultConfig {
        minSdk = 24
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    sourceSets {
        getByName("main") {
            java.directories.clear()
            java.directories.add(
                rootProject.file("../crates/cranpose/android/java-billing").absolutePath
            )
        }
    }

    publishing {
        singleVariant("release")
    }
}

dependencies {
    api(project(":cranpose-android"))
    api("com.android.billingclient:billing:9.1.0")
}

publishing {
    publications {
        register<MavenPublication>("release") {
            afterEvaluate { from(components["release"]) }
            artifactId = "cranpose-android-billing"
        }
    }
}
