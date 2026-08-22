// Manifest-only feature module: it carries the foreground service and
// permissions the `media` service needs and nothing else, so an application
// that plays nothing never declares a media service.
plugins {
    id("com.android.library")
    `maven-publish`
}

android {
    namespace = "dev.cranpose.android.media"
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
            artifactId = "cranpose-android-media"
        }
    }
}
