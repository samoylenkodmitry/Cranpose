package dev.cranpose.twin

import androidx.compose.ui.ImageComposeScene
import androidx.compose.ui.unit.Density
import org.jetbrains.skia.EncodedImageFormat
import java.io.File

/** The canvas every scene renders on, in pixels at density 1. */
const val SCENE_WIDTH = 520
const val SCENE_HEIGHT = 420

/**
 * Renders every twin scene to `<outDir>/<scene>.png`.
 * Arguments: outDir, the regular Noto Sans file, the bold one.
 */
fun main(args: Array<String>) {
    require(args.size == 3) { "usage: outDir regularFont boldFont" }
    val out = File(args[0]).apply { mkdirs() }
    DemoFonts.load(File(args[1]), File(args[2]))
    for ((name, content) in SCENES + MATRIX_FRAMES) {
        val scene = ImageComposeScene(SCENE_WIDTH, SCENE_HEIGHT, Density(1f)) {
            SceneFrame { content() }
        }
        val image = scene.render()
        val png = image.encodeToData(EncodedImageFormat.PNG) ?: error("could not encode $name")
        File(out, "$name.png").writeBytes(png.bytes)
        scene.close()
        println("rendered $name")
    }
}
