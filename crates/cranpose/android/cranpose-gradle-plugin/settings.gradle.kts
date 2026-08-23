// This is a standalone Gradle build so that `pluginManagement.includeBuild`
// (composite-build plugin resolution) can point at it directly from any
// consuming application, wherever cargo happens to have put this crate's
// source on disk -- see the `pluginManagement` block that every Cranpose
// Android application copies into its own `settings.gradle.kts`.
pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "cranpose-gradle-plugin"
