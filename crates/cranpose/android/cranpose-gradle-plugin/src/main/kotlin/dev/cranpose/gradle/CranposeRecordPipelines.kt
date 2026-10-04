package dev.cranpose.gradle

import org.gradle.api.DefaultTask
import org.gradle.api.GradleException
import org.gradle.api.file.DirectoryProperty
import org.gradle.api.file.RegularFileProperty
import org.gradle.api.provider.Property
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.InputFile
import org.gradle.api.tasks.Internal
import org.gradle.api.tasks.TaskAction
import org.gradle.process.ExecOperations
import java.io.ByteArrayOutputStream
import javax.inject.Inject

/**
 * Records the pipelines an application draws on each connected device, for
 * its fresh installs.
 *
 * Runs the installed application on every device `adb` lists, with its
 * pipeline cache kept in its external files directory, and pulls each
 * device's cache into the application's `assets/cranpose_gpu/`, where the
 * next build ships it. A fresh install on a device with the same GPU and
 * driver then starts from those compiled pipelines; any other starts from
 * their records of what the application draws.
 */
abstract class CranposeRecordPipelines : DefaultTask() {
    @get:InputFile
    abstract val adb: RegularFileProperty

    @get:Input
    abstract val applicationId: Property<String>

    /** How long the application draws before its cache is pulled. */
    @get:Input
    abstract val drawSeconds: Property<Int>

    /** Where the caches go: the application's `assets/cranpose_gpu/`. */
    @get:Internal
    abstract val seedDir: DirectoryProperty

    @get:Inject
    abstract val exec: ExecOperations

    @TaskAction
    fun record() {
        val serials = adb("devices").lines().drop(1).mapNotNull { line ->
            line.split('\t').takeIf { it.size == 2 && it[1] == "device" }?.first()
        }
        if (serials.isEmpty()) {
            throw GradleException("no device is connected to record the pipelines on")
        }
        val app = applicationId.get()
        val remote = "/sdcard/Android/data/$app/files/cranpose_gpu"
        val seeds = seedDir.get().asFile
        seeds.mkdirs()
        for (serial in serials) {
            adb("-s", serial, "shell", "setprop", EXTERNAL_CACHE_PROPERTY, "1")
            try {
                adb("-s", serial, "shell", "am", "force-stop", app)
                adb("-s", serial, "shell", "rm", "-rf", remote)
                adb("-s", serial, "shell", "am", "start", "-n", "$app/$ACTIVITY")
                Thread.sleep(drawSeconds.get() * 1000L)
                adb("-s", serial, "shell", "am", "force-stop", app)
                adb("-s", serial, "pull", "$remote/.", seeds.absolutePath)
            } finally {
                // An empty value has to reach the device's shell quoted.
                adb("-s", serial, "shell", "setprop", EXTERNAL_CACHE_PROPERTY, "''")
            }
        }
    }

    private fun adb(vararg args: String): String {
        val output = ByteArrayOutputStream()
        exec.exec {
            commandLine(adb.get().asFile.absolutePath, *args)
            standardOutput = output
        }
        return output.toString()
    }

    private companion object {
        const val EXTERNAL_CACHE_PROPERTY = "debug.cranpose.pipeline_cache_external"
        const val ACTIVITY = "dev.cranpose.android.CranposeActivity"
    }
}
