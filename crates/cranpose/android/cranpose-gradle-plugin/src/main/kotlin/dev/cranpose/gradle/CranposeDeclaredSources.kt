package dev.cranpose.gradle

import org.gradle.api.DefaultTask
import org.gradle.api.file.ConfigurableFileCollection
import org.gradle.api.file.DirectoryProperty
import org.gradle.api.provider.MapProperty
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.InputFiles
import org.gradle.api.tasks.OutputDirectory
import org.gradle.api.tasks.PathSensitive
import org.gradle.api.tasks.PathSensitivity
import org.gradle.api.tasks.TaskAction
import java.io.File

/**
 * What a service declared in Rust brings: its Java source directory, the
 * receivers it declares, and the manifest whose components it adds, each
 * relative to `androidRoot()` where it has one.
 */
internal data class DeclaredService(
    val javaSource: String? = null,
    val receivers: List<String> = emptyList(),
    val manifest: String? = null,
)

internal val DECLARED_SERVICES: Map<String, DeclaredService> = mapOf(
    "update" to DeclaredService(
        javaSource = "java-update",
        receivers = listOf("dev.cranpose.android.CranposeAppUpdate"),
    ),
    "heart-rate" to DeclaredService(javaSource = "java-heart-rate"),
    "device-auth" to DeclaredService(javaSource = "java-device-auth"),
    "device-motion" to DeclaredService(javaSource = "java-device-motion"),
    "microphone" to DeclaredService(javaSource = "java-microphone"),
    // One class asks for the run-time permissions of every tracking and room service
    "hand-tracking" to DeclaredService(javaSource = "java-permissions"),
    "body-tracking" to DeclaredService(javaSource = "java-permissions"),
    "face-tracking" to DeclaredService(javaSource = "java-permissions"),
    "eye-tracking" to DeclaredService(javaSource = "java-permissions"),
    "anchors" to DeclaredService(javaSource = "java-permissions"),
    "plane-tracking" to DeclaredService(javaSource = "java-permissions"),
    "background" to DeclaredService(manifest = "manifests/background.xml"),
    "media" to DeclaredService(manifest = "manifests/media.xml"),
)

/**
 * Copies the framework's Java for each service the application's build script
 * declares in Rust, and nothing for a service it does not declare.
 */
abstract class CranposeDeclaredSources : DefaultTask() {

    /** What the application's build script declared, as `Declaration::emit` wrote it. */
    @get:InputFiles
    @get:PathSensitive(PathSensitivity.NONE)
    abstract val declaration: ConfigurableFileCollection

    /** The Java source directory of each service that brings one, by service name. */
    @get:Input
    abstract val serviceSources: MapProperty<String, String>

    /** The contents of those directories. */
    @get:InputFiles
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val sources: ConfigurableFileCollection

    /** Where the declared services' Java goes. */
    @get:OutputDirectory
    abstract val outputDir: DirectoryProperty

    @TaskAction
    fun run() {
        val output = outputDir.get().asFile
        output.deleteRecursively()
        output.mkdirs()
        val declared = readRustDeclaration(declaration).services
        // Services may share a directory, which is copied once
        val directories = serviceSources.get()
            .filterKeys { service -> service in declared }
            .values
            .toSet()
        for (directory in directories) {
            File(directory).copyRecursively(output)
        }
    }
}
