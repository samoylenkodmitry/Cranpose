// The Cranpose Gradle plugin and the Cranpose Android library are built from
// this repository, so the demo consumes them straight from source: the
// composite build supplies the plugin and substitutes the `dev.cranpose`
// artifacts for the projects that produce them.
pluginManagement {
    includeBuild("../../../android")
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

includeBuild("../../../android")

rootProject.name = "ComposeRS Demo"
include(":app")
