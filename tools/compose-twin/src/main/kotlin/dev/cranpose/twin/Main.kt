package dev.cranpose.twin

import androidx.compose.ui.ImageComposeScene
import androidx.compose.ui.unit.Density
import org.jetbrains.skia.EncodedImageFormat
import java.io.File
import kotlin.math.roundToInt

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
    val frames = SCENES.map { (name, content) -> TwinFrame(name, 1f, content) } + MATRIX_FRAMES
    for (frame in frames) {
        val name = frame.name
        val scene = ImageComposeScene(
            (SCENE_WIDTH * frame.density).roundToInt(),
            (SCENE_HEIGHT * frame.density).roundToInt(),
            Density(frame.density),
        ) {
            SceneFrame { frame.content() }
        }
        // A second frame, so an effect the first launched (a focus request)
        // shows in what is captured.
        scene.render(0)
        val image = scene.render(16_000_000)
        val png = image.encodeToData(EncodedImageFormat.PNG) ?: error("could not encode $name")
        File(out, "$name.png").writeBytes(png.bytes)
        scene.close()
        println("rendered $name")
    }
}
