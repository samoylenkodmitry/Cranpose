// Manifest-only feature module: it carries the permissions and components the
// `notifications` service needs and nothing else, so an application that does not use
// the service never asks the user for its permission.
plugins {
    id("com.android.library")
    `maven-publish`
}

android {
    namespace = "dev.cranpose.android.notifications"
    compileSdk = 36

    defaultConfig {
        minSdk = 24
    }

    publishing {
        singleVariant("release")
    }
}

dependencies {
    api(project(":cranpose-android"))
}

publishing {
    publications {
        register<MavenPublication>("release") {
            afterEvaluate { from(components["release"]) }
            artifactId = "cranpose-android-notifications"
        }
    }
}
