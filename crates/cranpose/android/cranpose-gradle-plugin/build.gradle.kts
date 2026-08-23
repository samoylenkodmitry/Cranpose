// The Cranpose Gradle plugin: everything a Cranpose Android application needs
// its build to do, so that an application's own build file states only what is
// specific to that application.
//
// This build is never published anywhere. A consuming application's
// `settings.gradle.kts` locates the `cranpose` crate source that cargo already
// resolved (registry cache, git checkout, or workspace path) and
// `pluginManagement.includeBuild`s this directory, so the plugin always runs
// straight from the same source tree as the Rust it configures.
plugins {
    `kotlin-dsl`
}

dependencies {
    compileOnly("com.android.tools.build:gradle:9.2.1")
}

// The plugin contributes the framework's Java sources, manifest fragments and
// ProGuard rules to a consuming application by reading them straight out of
// this crate's `android/` directory -- the directory this build's project sits
// in. That path is not knowable from the classes alone (a composite build has
// no jar to resolve a "next to me" location from), so it is captured once, at
// this build's own configuration time, into a resource the plugin reads at
// apply() time. It is always correct because the plugin is always recompiled
// from this exact checkout on the machine currently building it -- never
// fetched as a prebuilt artifact from another machine.
val cranposeAndroidRootResource = tasks.register("cranposeAndroidRootResource") {
    description = "Writes this crate's android/ directory where the plugin reads it at runtime"
    val androidRoot = providers.provider { projectDir.parentFile.absolutePath }
    val output = layout.buildDirectory.dir("generated/cranpose-android-root")
    inputs.property("androidRoot", androidRoot)
    outputs.dir(output)
    doLast {
        val file = output.get().file("dev/cranpose/gradle/android-root.txt").asFile
        file.parentFile.mkdirs()
        file.writeText(androidRoot.get())
    }
}

sourceSets.named("main") {
    resources.srcDir(cranposeAndroidRootResource)
}

gradlePlugin {
    plugins {
        register("cranposeAndroid") {
            id = "dev.cranpose.android"
            implementationClass = "dev.cranpose.gradle.CranposeAndroidPlugin"
            displayName = "Cranpose Android application"
            description =
                "Configures an Android application built on Cranpose: the native build, " +
                    "ABIs, profiles, JNI packaging, the framework's Java, manifest and " +
                    "ProGuard contributions, and the optional service modules."
        }
    }
}
