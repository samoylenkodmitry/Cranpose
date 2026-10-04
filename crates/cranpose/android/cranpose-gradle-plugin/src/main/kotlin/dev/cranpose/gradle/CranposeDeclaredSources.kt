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

internal data class DeclaredService(val javaSource: String, val receivers: List<String>)

internal val DECLARED_SERVICES: Map<String, DeclaredService> = mapOf(
    "update" to DeclaredService(
        javaSource = "java-update",
        receivers = listOf("dev.cranpose.android.CranposeAppUpdate"),
    ),
    "heart-rate" to DeclaredService(
        javaSource = "java-heart-rate",
        receivers = emptyList(),
    ),
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
        for ((service, directory) in serviceSources.get()) {
            if (service in declared) {
                File(directory).copyRecursively(output)
            }
        }
    }
}
