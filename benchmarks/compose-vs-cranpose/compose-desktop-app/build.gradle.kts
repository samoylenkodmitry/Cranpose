// The gauntlet in Compose Multiplatform on the desktop: the composables the
// Android app draws, from `shared-compose`, in a desktop window.
plugins {
    id("org.jetbrains.kotlin.jvm")
    id("org.jetbrains.kotlin.plugin.compose")
    id("org.jetbrains.compose")
}

kotlin {
    jvmToolchain(17)
    sourceSets.named("main") {
        kotlin.srcDir("../shared-kotlin")
        kotlin.srcDir("../shared-compose")
    }
}

// The same stability configuration as the Android app.
composeCompiler {
    stabilityConfigurationFiles.add(layout.projectDirectory.file("../compose-app/compose_stability.conf"))
}

dependencies {
    implementation(compose.desktop.currentOs)
}

compose.desktop {
    application {
        mainClass = "dev.perfcompare.compose.MainKt"
        nativeDistributions.packageName = "PerfCompose"
    }
}
