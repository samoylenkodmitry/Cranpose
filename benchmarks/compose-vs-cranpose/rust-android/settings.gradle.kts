pluginManagement {
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
rootProject.name = "Perf Compare Rust apps"
// Each app is its crate's folder, packaged here: `../<app>-app`.
for (app in listOf("egui", "slint")) {
    include(":$app")
    project(":$app").projectDir = file("../$app-app")
}
