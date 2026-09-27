// A Compose Desktop twin of desktop-demo scenes: renders each scene headless
// at density 1 to a PNG, for a pixel comparison with Cranpose's rendering of
// the same composable code (issue #905).
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

tasks.register<JavaExec>("renderScenes") {
    description = "Renders every twin scene to reference/, the frames Cranpose's captures are held to, or to -PoutDir."
    classpath = sourceSets["main"].runtimeClasspath
    mainClass.set("dev.cranpose.twin.MainKt")
    val repo = rootDir.parentFile.parentFile
    args(
        (project.findProperty("outDir") as String?) ?: projectDir.resolve("reference").path,
        repo.resolve("apps/desktop-demo/assets/NotoSansMerged.ttf").path,
        repo.resolve("apps/desktop-demo/assets/NotoSansBold.ttf").path,
    )
}
