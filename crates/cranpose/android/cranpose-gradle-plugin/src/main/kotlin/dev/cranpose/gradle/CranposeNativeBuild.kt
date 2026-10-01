package dev.cranpose.gradle

import org.gradle.api.DefaultTask
import org.gradle.api.file.DirectoryProperty
import org.gradle.api.services.BuildService
import org.gradle.api.services.BuildServiceParameters
import org.gradle.api.tasks.OutputDirectory
import org.gradle.process.ExecOperations
import javax.inject.Inject

/** Builds the JNI libraries and Rust capability declarations for one Android variant. */
abstract class CranposeNativeBuild : DefaultTask() {
    /** Native libraries arranged by Android ABI for the variant's APK. */
    @get:OutputDirectory
    abstract val outputDir: DirectoryProperty

    /** A staged capability declaration for each ABI this variant packages. */
    @get:OutputDirectory
    abstract val declarationDir: DirectoryProperty

    /** Executes the variant's Cargo builds. */
    @get:Inject
    abstract val execOperations: ExecOperations
}

internal abstract class CranposeCargoBuildService : BuildService<BuildServiceParameters.None>
