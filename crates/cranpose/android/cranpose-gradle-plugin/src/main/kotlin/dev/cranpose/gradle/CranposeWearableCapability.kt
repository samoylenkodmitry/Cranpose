package dev.cranpose.gradle

import org.gradle.api.DefaultTask
import org.gradle.api.file.DirectoryProperty
import org.gradle.api.provider.Property
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.OutputDirectory
import org.gradle.api.tasks.TaskAction

/**
 * Writes the Wear OS capability that tells the paired phone or watch this
 * application is installed: `cranpose_app_` and the application id, which the
 * phone and watch applications share. `CranposeWearable` asks for the same
 * name, so each side sees the other as soon as it is installed.
 */
abstract class CranposeWearableCapability : DefaultTask() {

    @get:Input
    abstract val applicationId: Property<String>

    @get:OutputDirectory
    abstract val outputDir: DirectoryProperty

    @TaskAction
    fun run() {
        val values = outputDir.get().asFile.resolve("values")
        values.mkdirs()
        val capability = "cranpose_app_" + applicationId.get().replace('.', '_')
        values.resolve("cranpose_wearable.xml").writeText(
            """
            |<?xml version="1.0" encoding="utf-8"?>
            |<resources>
            |    <string-array name="android_wear_capabilities" translatable="false">
            |        <item>$capability</item>
            |    </string-array>
            |</resources>
            |""".trimMargin()
        )
    }
}
