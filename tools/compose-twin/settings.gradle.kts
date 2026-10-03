pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
    plugins {
        id("org.jetbrains.kotlin.jvm") version "2.4.20"
        id("org.jetbrains.kotlin.plugin.compose") version "2.4.20"
        id("org.jetbrains.compose") version "1.12.1"
    }
}

dependencyResolutionManagement {
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "compose-twin"
