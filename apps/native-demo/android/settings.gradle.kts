pluginManagement { repositories { google(); mavenCentral(); gradlePluginPortal() } }
dependencyResolutionManagement { repositories { google(); mavenCentral() } }
rootProject.name = "Cranpose Native Host"
include(":app")
include(":cranpose")
project(":cranpose").projectDir = file("../../../platforms/android")
