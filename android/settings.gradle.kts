// Plugin versions live here rather than in a root `plugins { ... apply false }`
// block. That block resolves the Android plugin onto the root project's script
// classpath, which every subproject inherits -- including the Gradle plugin
// module, whose `kotlin-dsl` then finds the Kotlin version the Android plugin
// dragged in instead of the one Gradle embeds, and warns that the two disagree.
pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
    plugins {
        id("com.android.library") version "9.2.1"
    }
}

dependencyResolutionManagement {
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "cranpose-android-build"

include(":cranpose-android")
include(":cranpose-android-background")
include(":cranpose-android-camera")
include(":cranpose-android-haptics")
include(":cranpose-android-media")
include(":cranpose-android-billing")
include(":cranpose-android-notifications")
include(":cranpose-android-overlay")
include(":cranpose-android-update")
include(":cranpose-gradle-plugin")
