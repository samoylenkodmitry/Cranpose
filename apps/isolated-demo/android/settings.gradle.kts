// The standalone starter consumes the published Cranpose artifacts, the same
// way a project copied out of this repository does: the Gradle plugin and the
// `dev.cranpose` libraries are resolved by version, not from the framework's
// source tree.
//
// `mavenLocal()` is listed first because the Android distribution is not on a
// remote repository yet; publish it once with `../../../android/gradlew
// publishToMavenLocal` and this build resolves it.
pluginManagement {
    repositories {
        mavenLocal()
        google()
        mavenCentral()
        gradlePluginPortal()
    }
    plugins {
        id("com.android.application") version "9.2.1"
        id("dev.cranpose.android") version "0.1.95"
    }
}

dependencyResolutionManagement {
    repositories {
        mavenLocal()
        google()
        mavenCentral()
    }
}

rootProject.name = "Cranpose Isolated Demo"
include(":app")
