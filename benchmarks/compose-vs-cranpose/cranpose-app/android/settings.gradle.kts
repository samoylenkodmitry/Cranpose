// Locates the Cranpose Gradle plugin inside the `cranpose` crate cargo resolved,
// exactly as every Cranpose Android application does.
pluginManagement {
    val cranposePackage = (groovy.json.JsonSlurper().parseText(
        providers.exec {
            workingDir = rootDir.parentFile
            commandLine("cargo", "metadata", "--format-version=1")
        }.standardOutput.asText.get()
    ) as Map<*, *>)["packages"].let { it as List<*> }
        .map { it as Map<*, *> }
        .firstOrNull { it["name"] == "cranpose" }
        ?: error("cargo metadata reports no `cranpose` package")
    val cranposeDir = java.io.File(cranposePackage["manifest_path"] as String).parentFile
    includeBuild(cranposeDir.resolve("android/cranpose-gradle-plugin"))

    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
    plugins {
        id("com.android.application") version "9.4.1"
    }
}

dependencyResolutionManagement {
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "Perf Compare Cranpose"
include(":app")
