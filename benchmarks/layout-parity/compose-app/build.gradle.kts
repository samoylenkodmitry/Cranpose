plugins {
    id("org.jetbrains.kotlin.jvm")
    id("org.jetbrains.kotlin.plugin.compose")
    id("org.jetbrains.compose")
}

kotlin {
    jvmToolchain(17)
}

dependencies {
    implementation(compose.desktop.currentOs)
    implementation(compose.foundation)
}

compose.desktop {
    application {
        mainClass = "dev.parity.MainKt"
    }
}

tasks.withType<JavaExec>().configureEach {
    val fontsDir = rootDir.parentFile.parentFile.resolve("compose-vs-cranpose/fonts")
    systemProperty("parity.fontsDir", fontsDir.absolutePath)
}
