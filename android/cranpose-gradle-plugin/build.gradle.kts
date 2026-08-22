// The Cranpose Gradle plugin: everything a Cranpose Android application needs
// its build to do, so that an application's own build file states only what is
// specific to that application.
plugins {
    `kotlin-dsl`
    `maven-publish`
}

dependencies {
    compileOnly("com.android.tools.build:gradle:9.2.1")
}

// The plugin resolves the `dev.cranpose` artifacts that match itself, so it has
// to know its own version at runtime. A jar manifest attribute is not enough:
// it is absent from a composite build, where the plugin runs straight from its
// classes. Carrying the version as a resource makes both cases read the same
// value and keeps the release number out of the Kotlin source.
val cranposeVersionResource = tasks.register("cranposeVersionResource") {
    description = "Writes the plugin's version where the plugin reads it at runtime"
    val version = providers.provider { project.version.toString() }
    val output = layout.buildDirectory.dir("generated/cranpose-version")
    inputs.property("version", version)
    outputs.dir(output)
    doLast {
        val file = output.get().file("dev/cranpose/gradle/version.txt").asFile
        file.parentFile.mkdirs()
        file.writeText(version.get())
    }
}

sourceSets.named("main") {
    resources.srcDir(cranposeVersionResource)
}

gradlePlugin {
    plugins {
        register("cranposeAndroid") {
            id = "dev.cranpose.android"
            implementationClass = "dev.cranpose.gradle.CranposeAndroidPlugin"
            displayName = "Cranpose Android application"
            description =
                "Configures an Android application built on Cranpose: the native build, " +
                    "ABIs, profiles, JNI packaging, optional service modules and manifest metadata."
        }
    }
}
